# Upstream Parity Ledger

This ledger is maintained by long-running Codex `/goal` sessions. It is the
source of truth for deciding what remains before this repository can claim full
portable parity with upstream [`vercel/ai`](https://github.com/vercel/ai).

Non-negotiable test floor: EVERY portable original upstream TypeScript test/case
must exist as an equivalent Rust test in the matching 1:1 crate. Rust may add
more tests for stronger typing, extra edge cases, live-provider proof, and
Rust-specific failures, but it must never have fewer mapped original TypeScript
tests. A package with even one missing portable upstream test/case is not
parity-complete.
This is a containment rule, not a coverage preference or raw count comparison:
the complete original TypeScript test inventory must be enumerated and mapped
into Rust first, and only then may Rust add extra tests on top. For every
matching crate, the required relationship is `original TypeScript tests <=
mapped Rust tests`; any missing original portable TypeScript test keeps the
crate incomplete even if Rust has broader or more numerous Rust-only tests.

Handoff note for future iterations: EVERY portable test/case from the original
TypeScript package must exist as a Rust test in the matching crate before
parity can be claimed. Rust can and should have potentially more tests, but no
less: extra Rust tests are additive only and cannot offset one missing original
portable TypeScript test/case.

Future-iteration test note: the matching Rust crate must contain EVERY portable
test/case from the original TypeScript package as a named Rust counterpart.
Rust may include potentially more tests for Rust-specific proof, but never
fewer mapped original TypeScript tests; extra Rust tests are additive only and
do not offset or replace any missing upstream case. No package can be marked
complete until the full original TypeScript test inventory is represented in
Rust, test-for-test or case-for-case, or explicitly documented as
JavaScript-only.

Future iterations must read that literally: EVERY test from the original
TypeScript package must exist in Rust, Rust may add potentially more tests, but
there must never be less mapped upstream test coverage than the original
TypeScript package.

## Crate Boundary Gate

Full parity requires a strict 1:1 mapping between upstream TypeScript packages
and Rust crates: one Rust crate for each portable upstream package, and no Rust
crate implementing APIs from more than one upstream package. This is not a
future cleanup note; it is a merge-blocking acceptance gate for every iteration
from now on. Crate ownership is part of parity, not packaging polish.

Each portable `packages/*` entry that has Rust API must have its matching Rust
workspace crate before that API is added. That crate owns the package's public
API, options, implementation, docs, tests, and provider/model surfaces. If the
matching crate does not exist yet, creating it is the first step of the slice. A
slice that implements package-owned behavior in the wrong crate is blocked,
incomplete, and cannot count toward verified parity even if the behavior works
and its tests pass.

The current root crate already merges multiple upstream TypeScript packages into
one Rust crate. That is active architecture debt being created by today's
porting choices, and continuing to add package-owned code there makes the
eventual split harder, more coupled, and more breaking for users. Existing
package-owned code in the root is extraction debt. New package-owned code in the
root, or in any crate that already owns a different upstream package, is a
regression.

The root crate may act only as the Rust equivalent of upstream `packages/ai`:
facade APIs, aggregate re-exports, and compatibility shims. Provider contracts,
provider utilities, provider packages, MCP, Workflow, telemetry, adapters, and
other separately packaged upstream surfaces must move to, or start in, their own
matching crates.

## Test Parity Gate

Full parity requires every portable test from the original TypeScript package
to exist as an equivalent Rust test in the matching Rust crate. This is a
minimum bar, not an aspirational coverage target: Rust may add more tests for
stronger typing, additional edge cases, live-provider proof, or Rust-specific
failure modes, but it must never have fewer portable tests than upstream.
Future iterations must treat the original TypeScript test inventory as
mandatory: EVERY original portable TypeScript test/case must exist in Rust,
and the Rust suite may only be larger, never smaller.
The required comparison is equality-plus: every original portable TypeScript
test must exist in Rust, and any Rust-only tests are additive coverage only.
The upstream TypeScript test inventory is the floor and the Rust inventory must
be a superset, not a sampled or reduced suite.
Future iterations must start from the original TypeScript test list and port it
one-to-one into Rust before counting any Rust-only coverage as additive. Extra
Rust tests are welcome, but they cannot replace, collapse, or hide a missing
upstream test case.
Test parity is therefore a Rust superset of the original TypeScript tests:
EVERY portable original TypeScript test/case must exist in the matching Rust
crate, and Rust may have more tests, but never fewer mapped original tests.

Every original portable TypeScript test case must be accounted for, including
table-driven cases, helper-backed scenarios, fixtures, snapshot-equivalent
assertions, streaming edge cases, error paths, and type-level tests. A Rust test
may combine several upstream assertions only when the ledger records the exact
upstream tests/cases it covers. The inventory must be tracked at the individual
test/case level, not only at the test-file, package-feature, or broad behavior
level; otherwise, each upstream case needs a named Rust
counterpart in the owning crate.

Missing upstream tests are missing parity. A package row cannot be marked
`verified` while any portable upstream `*.test.ts`, `*.test.tsx`,
`*.test-d.ts`, `*.test-d.tsx`, `*.spec.ts`, or `*.spec.tsx` case lacks a Rust
counterpart or an explicit `js-only-documented` justification. Passing a
smaller hand-picked Rust suite is not enough, even if the implemented behavior
looks correct.

In practical terms, the matching Rust crate must contain every original
portable upstream TypeScript test/case and may contain additional Rust-specific
tests on top. More tests are encouraged; fewer tests than the original
TypeScript package is a parity failure. Do not merge or mark a package
`verified` until the ledger shows that the upstream test list is fully mapped
one-to-one or explicitly documented as non-portable.
Acceptance is equality-plus only: EVERY original portable TypeScript test/case
must exist as a Rust test in the matching crate, and Rust may add more tests but
must never have a smaller portable test inventory than the original TypeScript
package.
The minimum passing Rust suite for a package is therefore the full portable
original TypeScript test inventory, plus any additional Rust-only tests; no
Rust-only coverage can offset a missing upstream test.
In short: potentially more Rust tests, never fewer, and every portable original
TypeScript test/case must remain individually visible in the ledger until it has
a Rust counterpart or an explicit non-portable justification.
Do not treat a larger Rust-only test suite as parity if even one original
portable TypeScript test/case is missing from the matching crate.

Future-iteration note: the original upstream TypeScript test list is the
non-negotiable minimum for each matching Rust crate. EVERY original portable
upstream test/case must exist as a named Rust counterpart in the matching crate.
Rust may add potentially more tests for stronger typing, extra edge cases,
live-provider proof, or Rust-specific failures, but no less mapped original
TypeScript tests. Extra Rust tests are additive only. This is an
inventory-containment rule, not a count-only rule: every original portable
TypeScript test must exist in Rust, and then Rust may add more tests on top,
but never less.
Every original TypeScript test is assumed required until it is ported or
explicitly documented as JavaScript-only/non-portable. The Rust suite must be a
superset of the original portable TypeScript test inventory: potentially more
Rust tests, but no missing original portable TypeScript tests and no smaller
mapped upstream inventory.
Read EVERY literally: future iterations must enumerate the original TypeScript
tests first, port each portable case into Rust, document any JavaScript-only
exception, and only then count extra Rust-specific tests as additive coverage.
Count parity from the original upstream TypeScript test list, not from the
number of Rust tests. A Rust crate with extra Rust-specific tests but even one
missing original portable upstream test/case remains incomplete. The only
acceptable end state is every original portable TypeScript test/case existing in
the matching Rust crate, with optional Rust-specific tests added on top.

## Required Work Order

The queue is two-phase and gated:

1. Finish ALL common/core SDK packages together with Vercel AI Gateway provider
   coverage.
2. Resume unrelated standalone provider packages only after that first phase is
   verified or intentionally documented as non-portable.

The first phase includes `packages/ai`, `packages/provider`,
`packages/provider-utils`, `packages/openai-compatible`,
`packages/open-responses`, `packages/gateway`, the Vercel AI Gateway
OpenAI-compatible and Open Responses routes, and portable non-provider rows such
as MCP, OTel, Workflow, telemetry, logger, UI transport, chat/completion/object
transport, and test-server support. Treat Vercel AI Gateway as part of the
first phase, not as one of the later standalone providers. A standalone provider
slice is blocked while any first-phase row is `not-started` or `in-progress`.
Gateway progress alone does not unblock other providers; the whole common/core
plus Vercel AI Gateway phase must be finished first.

## Inventory Rules

- Record the upstream commit SHA/date used for each inventory pass.
- List every upstream package, provider package, framework adapter, example,
  testable behavior, public API, and feature.
- Use one of these statuses for each row: `not-started`, `in-progress`,
  `ported`, `verified`, `js-only-documented`.
- A row may be `verified` only when there is a Rust equivalent plus tests,
  examples, or documented validation evidence.
- The Rust test inventory for a portable package must be at least as complete
  as the upstream TypeScript test inventory. Record exact Rust test names for
  every ported upstream behavior, keep unmatched upstream tests in the remaining
  work notes, and treat every missing portable upstream test as blocking
  `verified` status. A package cannot be considered complete until every
  portable upstream test case is either mapped to Rust or explicitly documented
  as JavaScript-only/non-portable.
- Provider-backed rows need deterministic tests plus live-provider proof before
  they can be `verified`. The live proof must be an ignored credential-gated
  test or runnable example that skips cleanly without credentials, never prints
  secrets, and is recorded in this ledger with the test/example name and date.
  If credentials or a live API are unavailable, keep the row `in-progress` and
  document that gap instead of treating fake/mock tests as real-provider proof.
- OTel/telemetry rows need deterministic span assertions plus local OTLP/HTTP
  export proof before they can be `verified`. The local proof must run against
  the loopback OTLP receiver or a local OpenTelemetry Collector endpoint and
  assert the emitted wire payload. For `packages/otel`, dependency-free mock
  exporter proof is not sufficient by itself; the row also needs a real
  `opentelemetry` SDK/exporter probe against the same receiver or collector.
  Once root telemetry wiring exists, provider live tests should run with
  telemetry enabled and verify emitted OTLP data through that same local
  receiver or collector.
- A portable package row may not be `verified` unless its public API,
  implementation, docs, and tests are owned by the Rust crate that maps
  one-to-one to the upstream TypeScript package. Passing tests in the wrong
  crate are still package-boundary debt, not verified parity.
- When updating a package row, record the matching Rust crate as the ownership
  target. A root module or consolidated crate path is evidence of incomplete
  extraction unless it is only a facade re-export or compatibility shim.
- A row may be `js-only-documented` only when the behavior is truly not
  portable to Rust and the Rust-facing alternative is documented in the row.
- Do not remove upstream items just because they are hard or large.

## Latest Upstream Inventory

| Field | Value |
| --- | --- |
| Upstream repo | `vercel/ai` |
| Inventory command | `npx opensrc@latest path github:vercel/ai` |
| Local source path | `/Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel/ai/main` |
| Upstream commit | `aa5a1e539643c2a7162a141502eee63c665a9544` |
| Upstream commit date | `2026-05-16T06:55:10Z` |
| Inventory date | `2026-05-18` |
| Upstream package count | 56 packages under `packages/*/package.json` |
| Upstream package test files | 521 `*.test.ts`, `*.test.tsx`, `*.test-d.ts`, `*.test-d.tsx`, `*.spec.ts`, and `*.spec.tsx` files under `packages/*` |
| Upstream examples | 22 top-level example apps/directories under `examples/*` |

## Package And Provider Inventory

Every upstream package is listed here. Provider implementation packages are
`not-started` until this crate has a Rust provider module or crate with typed
settings, model contracts, provider-specific serialization tests, and model-call
behavior tests. Framework adapters are marked `js-only-documented` only for
browser or JavaScript framework bindings; portable model, transport, message,
and stream behavior used by those adapters is tracked separately in the API
inventory.

| Upstream item | Kind | Status | Rust path | Evidence | Notes |
| --- | --- | --- | --- | --- | --- |
| `packages/ai` (`ai`) | core package | in-progress | `src/lib.rs`, `src/agent.rs`, `src/generate_text.rs`, `src/stream_text.rs`, `src/generate_object.rs`, `src/stream_object.rs`, `src/embed.rs`, `src/generate_image.rs`, `src/generate_speech.rs`, `src/generate_video.rs`, `src/transcribe.rs`, `src/rerank.rs`, `src/upload_file.rs`, `src/upload_skill.rs`, `src/registry.rs`, `src/provider_middleware.rs`, `src/mock_models.rs`, `src/text_stream_response.rs`, `src/ui_message_stream.rs`, `src/chat_transport.rs`, `src/completion_transport.rs`, `src/object_transport.rs`, `src/logger.rs`, `src/telemetry.rs`, `src/retry.rs`, `src/util.rs`, `src/prompt.rs` | Unit tests in matching modules; `tool_loop_agent_exposes_version_id_and_tools`; `tool_loop_agent_generate_forwards_settings_and_instructions`; `tool_loop_agent_generate_passes_string_instructions`; `tool_loop_agent_generate_passes_system_message_instructions`; `tool_loop_agent_generate_passes_array_of_system_message_instructions`; `tool_loop_agent_generate_forwards_temperature_to_generate_text`; `tool_loop_agent_generate_forwards_max_output_tokens_to_generate_text`; `tool_loop_agent_generate_forwards_top_p_to_generate_text`; `tool_loop_agent_generate_forwards_top_k_to_generate_text`; `tool_loop_agent_generate_forwards_presence_penalty_to_generate_text`; `tool_loop_agent_generate_forwards_frequency_penalty_to_generate_text`; `tool_loop_agent_generate_forwards_stop_sequences_to_generate_text`; `tool_loop_agent_generate_forwards_seed_to_generate_text`; `tool_loop_agent_generate_forwards_headers_to_generate_text`; `tool_loop_agent_generate_forwards_include_request_messages_to_generate_text`; `tool_loop_agent_prepare_call_can_shape_provider_options`; `tool_loop_agent_generate_rejects_invalid_call_options_schema_before_model_call`; `tool_loop_agent_generate_passes_valid_call_options_schema`; `tool_loop_agent_generate_passes_sandbox_to_prepare_call`; `tool_loop_agent_generate_passes_sandbox_to_tool_execution`; `tool_loop_agent_generate_honors_tool_approval`; `tool_loop_agent_generate_calls_on_start_from_constructor`; `tool_loop_agent_generate_calls_on_start_from_method`; `tool_loop_agent_generate_on_start_passes_event_information`; `tool_loop_agent_generate_on_start_passes_messages_option`; `tool_loop_agent_generate_passes_abort_signal_to_generate_text`; `tool_loop_agent_generate_passes_timeout_to_tool_execution`; `tool_loop_agent_merges_generate_start_callbacks_in_order`; `tool_loop_agent_generate_calls_on_step_start_from_constructor`; `tool_loop_agent_generate_calls_on_step_start_from_method`; `tool_loop_agent_generate_merges_on_step_start_callbacks_in_order`; `tool_loop_agent_generate_on_step_start_passes_event_information`; `tool_loop_agent_generate_calls_on_step_finish_from_constructor`; `tool_loop_agent_generate_calls_on_step_finish_from_method`; `tool_loop_agent_generate_merges_on_step_finish_callbacks_in_order`; `tool_loop_agent_generate_on_step_finish_passes_step_result_to_callback`; `tool_loop_agent_generate_calls_on_finish_from_constructor`; `tool_loop_agent_generate_calls_on_finish_from_method`; `tool_loop_agent_generate_merges_on_finish_callbacks_in_order`; `tool_loop_agent_generate_on_finish_passes_event_information`; `tool_loop_agent_uses_upstream_twenty_step_default_for_tool_loop`; `tool_loop_agent_generate_calls_on_tool_execution_start_from_constructor`; `tool_loop_agent_generate_calls_on_tool_execution_start_from_method`; `tool_loop_agent_generate_merges_on_tool_execution_start_callbacks_in_order`; `tool_loop_agent_generate_on_tool_execution_start_passes_event_information`; `tool_loop_agent_generate_calls_on_tool_execution_end_from_constructor`; `tool_loop_agent_generate_calls_on_tool_execution_end_from_method`; `tool_loop_agent_generate_merges_on_tool_execution_end_callbacks_in_order`; `tool_loop_agent_generate_on_tool_execution_end_passes_event_information_on_success`; `tool_loop_agent_merges_tool_execution_callbacks_in_order`; `tool_loop_agent_stream_delegates_to_stream_text`; `tool_loop_agent_stream_passes_string_instructions`; `tool_loop_agent_stream_passes_system_message_instructions`; `tool_loop_agent_stream_forwards_include_raw_chunks_to_stream_text`; `tool_loop_agent_stream_prepare_call_can_shape_provider_options`; `tool_loop_agent_stream_passes_sandbox_to_prepare_call`; `tool_loop_agent_stream_passes_sandbox_to_tool_execution`; `tool_loop_agent_stream_honors_tool_approval`; `tool_loop_agent_stream_calls_on_start_from_constructor`; `tool_loop_agent_stream_calls_on_start_from_method`; `tool_loop_agent_stream_on_start_passes_event_information`; `tool_loop_agent_stream_passes_abort_signal_to_stream_text`; `tool_loop_agent_stream_passes_timeout_to_tool_execution`; `tool_loop_agent_stream_calls_on_tool_execution_start_from_constructor`; `tool_loop_agent_stream_calls_on_tool_execution_start_from_method`; `tool_loop_agent_stream_merges_on_tool_execution_start_callbacks_in_order`; `tool_loop_agent_stream_on_tool_execution_start_passes_event_information`; `tool_loop_agent_stream_calls_on_tool_execution_end_from_constructor`; `tool_loop_agent_stream_calls_on_tool_execution_end_from_method`; `tool_loop_agent_stream_merges_on_tool_execution_end_callbacks_in_order`; `tool_loop_agent_stream_on_tool_execution_end_passes_event_information_on_success`; `tool_loop_agent_generate_calls_per_call_integration_listeners_for_all_lifecycle_events`; `tool_loop_agent_stream_calls_per_call_integration_listeners_for_all_lifecycle_events`; `tool_loop_agent_generate_calls_globally_registered_integration_listeners`; `tool_loop_agent_stream_calls_globally_registered_integration_listeners`; `tool_loop_agent_generate_includes_configured_runtime_context_properties_in_telemetry`; `tool_loop_agent_stream_includes_configured_runtime_context_properties_in_telemetry`; `tool_loop_agent_generate_calls_integration_listeners_alongside_agent_callbacks`; `tool_loop_agent_stream_calls_integration_listeners_alongside_agent_callbacks`; `tool_loop_agent_generate_does_not_break_when_an_integration_listener_panics`; `tool_loop_agent_stream_does_not_break_when_an_integration_listener_panics`; `tool_loop_agent_merges_stream_finish_callbacks_in_order`; `mock_models::tests::*`; `logger::tests::*`; `telemetry::tests::*`; `generate_text_dispatches_telemetry_lifecycle_events`; `generate_text_dispatches_tool_execution_telemetry_events`; `stream_text_dispatches_telemetry_lifecycle_events`; `stream_text_dispatches_tool_execution_telemetry_events`; `generate_object_dispatches_telemetry_lifecycle_events`; `stream_object_dispatches_telemetry_lifecycle_events`; `generate_object_messages_with_url_file_calls_model_supported_urls`; `generate_text_messages_with_url_file_calls_model_supported_urls`; `stream_text_messages_with_url_file_calls_model_supported_urls`; `stream_object_messages_with_url_file_calls_model_supported_urls`; `embed_dispatches_telemetry_lifecycle_events`; `embed_many_dispatches_telemetry_lifecycle_events`; `rerank_dispatches_telemetry_lifecycle_events`; `ui_message_chunk_serializes_portable_tool_source_and_file_chunks`; `process_ui_message_stream_preserves_portable_non_text_chunks_as_parts`; `direct_chat_transport_streams_text_response_from_agent`; `direct_chat_transport_passes_prepared_agent_options`; `direct_chat_transport_applies_ui_message_stream_options`; `direct_chat_transport_converts_ui_messages_to_model_messages_in_order`; `direct_chat_transport_rejects_invalid_ui_message_part_shape`; `direct_chat_transport_reconnect_returns_none`; `convert_ui_messages_maps_static_tool_output_available_to_assistant_and_tool_messages`; `convert_ui_messages_maps_tool_output_error_raw_input_to_error_text`; `convert_ui_messages_maps_dynamic_tool_output_available_tool_name`; `convert_ui_messages_preserves_step_start_blocks_as_assistant_tool_pairs`; `convert_ui_messages_places_provider_executed_tool_result_in_assistant`; `convert_ui_messages_maps_denied_approval_response_to_execution_denied_result`; `convert_ui_messages_skips_unconverted_data_parts`; `convert_ui_messages_maps_file_provider_reference_and_metadata_parts`; `completion_transport_builds_default_request`; `completion_transport_builds_prepared_request_with_overrides`; `completion_transport_processes_text_stream`; `completion_transport_processes_data_event_stream`; `completion_transport_reports_data_event_error_chunks`; `completion_transport_reports_invalid_data_event_chunks`; `object_transport_builds_post_request_with_input_body`; `object_transport_processes_distinct_partial_json_updates`; `object_transport_skips_duplicate_partial_objects`; `object_transport_ignores_empty_chunks_until_json_can_be_repaired`; `object_transport_parses_final_json_for_validation_boundary`; `chat_transport::tests::*`; `retry::tests::*`; `util::tests::*`; `prompt::tests::*`; `examples/kitchen_sink.rs` | Non-streaming generation, streamed text collection with local tool continuation and textStream filtering, object, image, speech, video, transcription, embeddings, reranking, upload, registry, provider-level middleware wrapping, initial `ToolLoopAgent` wrapper over `generate_text`/`stream_text` with shared settings, prepare-call shaping, configured tools, instructions and instruction-shape forwarding, streaming delegation, model/request option forwarding, include request-message retention, prepare-call sandbox propagation, sandbox propagation into local tool execution, user-approval blocking, onStart callback/event forwarding, step-start, step-finish, finish, and tool-execution callback/event forwarding, stream prepare-call provider-option shaping, per-call abort/timeout request controls, upstream's default twenty-step tool loop, and constructor/per-call lifecycle and tool-execution callback merging, tool input examples middleware, extract JSON middleware, simulate streaming middleware, text-stream response helpers, UI-message stream SSE/read/process/text-transform helpers including portable tool/source/file/approval chunk contracts, initial chat transport request contracts plus in-process `DirectChatTransport` agent streaming, UI-message text conversion, assistant tool-history conversion for static and dynamic tools, step-start splitting, provider-executed tool-result placement, output-error raw input, denied approval responses, skipped unconverted data parts, file/provider-reference mapping, and custom/reasoning provider metadata, high-level URL-file message supported-URL hook parity across generate/stream text/object APIs, agent call option forwarding, UI-message stream options, invalid-message validation, reconnect-null behavior, request timeout helper extraction, high-level language model call option preparation, prompt standardization, file-part data conversion, tool preparation, tool-choice preparation, completion API request shaping, completion text/data stream accumulation/error handling, object transport request shaping, partial JSON stream repair/change filtering, and final object JSON parse boundary, retry/backoff utility parity, dependency-free warning logger formatting/state, root telemetry options/registry/dispatcher/diagnostic channel, `generate_text`, `stream_text`, `generate_object`, `stream_object`, `embed`, `embed_many`, and `rerank` telemetry dispatch for operation/step/language-model/tool/object/end events where applicable, mergeObjects/splitArray/fixJson/getPotentialStartIndex/mergeAbortSignals/setAbortTimeout/mergeCallbacks/notify/SerialJobExecutor/prepareRetries/requestTimeout/prepareLanguageModelCallOptions/standardizePrompt/convertToLanguageModelV4FilePart/prepareTools/prepareToolChoice/stopCondition utility parity, tool loops, public mock provider-v4 models, and many provider-v4 shapes exist. Remaining UI-message-to-model-message edge coverage for broader approval states, agent call-options type-level parity and remaining stream/UI edge cases remain unported. |
| `packages/provider` (`@ai-sdk/provider`) | provider contracts | in-progress | `crates/ai-sdk-provider`; root facade shims in `src/provider.rs`, `src/language_model.rs`, `src/embedding_model.rs`, `src/image_model.rs`, `src/speech_model.rs`, `src/transcription_model.rs`, `src/reranking_model.rs`, `src/video_model.rs`, `src/files.rs`, `src/skills.rs`, `src/json.rs`, `src/warning.rs`, `src/file_data.rs`, `src/headers.rs` | Contract and serialization tests in `crates/ai-sdk-provider/src/*`; one-to-one `get_error_message_*` split for upstream `get-error-message.test.ts`; `call_options_carries_abort_signal_without_serializing_it`; non-language call-option abort signal serialization coverage; root facade compile coverage | Provider-v4 contracts, shared JSON/header shapes, warnings, file/provider references, model call/result contracts, language-model and non-language-model call-option abort signal propagation, abort wake support, and provider trait surfaces now live in the matching `ai-sdk-provider` crate. The upstream provider `getErrorMessage` test file is represented case-for-case in Rust; upstream v2/v3 compatibility surfaces and exact stream abstractions remain unported. |
| `packages/provider-utils` (`@ai-sdk/provider-utils`) | provider support library | in-progress | `crates/ai-sdk-provider-utils`; root facade shim in `src/provider_utils.rs` | 760 upstream-shape unit tests in `crates/ai-sdk-provider-utils/src/provider_utils.rs`, including one-to-one `asArray`, `stripFileExtension`, `addAdditionalPropertiesToJsonSchema`, `filterNullable`, `removeUndefinedEntries`, complete portable `validateTypes`/`safeValidateTypes`, complete portable `secureJsonParse`, complete portable `parseJSON`/`safeParseJSON`/`isParsableJson`, complete portable `injectJsonInstruction`, exact `mediaTypeToExtension` table-row test splits, complete portable `detectMediaType`/`getTopLevelMediaType`/`isFullMediaType` case splits, complete portable `resolveFullMediaType` case splits, complete portable `isUrlSupported`, `validateDownloadUrl`, `downloadBlob`/`DownloadError`, `getFromApi`, `readResponseWithSizeLimit`, `responseHandler`, `handleFetchError`, `convertAsyncIteratorToReadableStream`, `isJSONSerializable`, complete portable `delay`, complete portable `executeTool`/`isExecutableTool`, portable `asSchema`/`StandardSchema`, `StreamingToolCallTracker`, and `serializeModelOptions` case splits, complete portable `convertToFormData` and `convertImageModelFileToDataUri` case splits, complete portable `normalizeHeaders` case splits, complete portable `mapReasoningToProvider*` case splits, complete portable `resolve` case splits, complete portable `createToolNameMapping` case splits, complete portable `prepareTools` case splits for root AI usage, complete portable `withUserAgentSuffix` and `getRuntimeEnvironmentUserAgent` case splits, complete portable `createIdGenerator`/`generateId` case splits, complete portable `DelayedPromise` case splits, portable `isProviderReference` case splits, complete portable `resolveProviderReference` case splits, complete portable `types/content-part.test-d.ts` case splits, `tool_execution_options_include_execution_metadata_context_abort_signal_and_sandbox`, `sandbox_command_options_include_abort_signal_without_serializing_it`, `tool_execute_function_accepts_input_output_and_execution_options`, `tool_needs_approval_function_accepts_input_options_and_returns_boolean`, `tool_to_model_output_accepts_untyped_output_without_execute`, `tool_to_model_output_accepts_execute_function_output`, `tool_to_model_output_accepts_output_schema_result_output`, `tool_needs_approval_function_accepts_input_schema_options`, `tool_needs_approval_function_accepts_execute_tool_options`, `tool_needs_approval_function_accepts_context_schema_context`, `dynamic_tool_upstream_should_include_dynamic_tools_in_the_tool_union`, `dynamic_tool_upstream_should_allow_function_style_properties`, `dynamic_tool_upstream_should_reject_provider_only_properties`, `dynamic_tool_upstream_should_create_dynamic_tools_with_the_dynamic_discriminator`, `provider_defined_tool_upstream_should_include_provider_defined_tools_in_the_tool_union`, `provider_defined_tool_upstream_should_require_provider_specific_properties`, `provider_defined_tool_upstream_should_allow_user_execution_or_an_output_schema`, `provider_defined_tool_upstream_rejects_function_only_properties`, `provider_executed_tool_upstream_should_include_provider_executed_tools_in_the_tool_union`, `provider_executed_tool_upstream_should_require_provider_specific_properties`, `provider_executed_tool_upstream_should_allow_deferred_result_support`, `provider_executed_tool_upstream_rejects_function_only_properties`, `function_tool_upstream_should_expose_the_function_tool_discriminator`, `function_tool_upstream_should_include_function_tools_in_the_tool_union`, `function_tool_upstream_should_allow_omitted_and_explicit_function_discriminators`, `function_tool_upstream_should_reject_dynamic_and_provider_only_properties`, `tool_union_upstream_should_expose_all_tool_variants_and_type_discriminators`, `tool_union_upstream_should_narrow_tools_by_type`, `tool_constructor_input_type_upstream_should_infer_input_type_from_zod_input_schema`, `tool_constructor_input_type_upstream_should_preserve_input_type_from_flexible_schema`, `tool_constructor_input_type_upstream_should_infer_input_type_with_optional_default_examples`, `tool_constructor_input_type_upstream_should_infer_input_type_with_refined_schema_examples`, `tool_constructor_context_type_upstream_should_infer_context_type_from_context_schema_in_execute`, `tool_constructor_context_type_upstream_should_infer_context_type_in_input_lifecycle_callbacks`, `tool_constructor_output_type_upstream_should_infer_output_type_from_execute_function`, `tool_constructor_output_type_upstream_should_infer_output_type_from_async_generator_execute_function`, `tool_input_lifecycle_callbacks_receive_upstream_execution_options`, `function_tool_retains_output_schema_without_provider_serialization`, `post_json_to_api_options_carries_abort_signal_without_serializing_it`, `post_form_data_to_api_options_carries_abort_signal_without_serializing_it`, `post_to_api_options_carries_abort_signal_without_serializing_it`, `post_json_to_api_aborts_before_transport_call`, and `post_json_to_api_aborts_pending_transport_when_signal_fires`; root facade compile coverage | Schema/validation including JSON Schema, lazy schema, and Standard Schema v1 conversion/validation, JSON parsing, header normalization/combination, resolvable value/function/future handling, nullish filtering, delayed externally resolved futures, abortable delay semantics, JSON instruction injection, reasoning provider mapping, media type detection/resolution, media/base64/form-data helpers, download and response-handler contracts, async iterator readable-stream cancellation, provider API request helpers including GET, JSON, form-data, and generic POST abort-signal propagation plus abort-aware pending transport cancellation, user-agent helpers, provider reference resolution, provider tool-name mapping, provider-utils content-part model-message/tool-result output contracts including legacy file/image variants, tool factories/types, function/dynamic tool output-schema retention for local execution typing, complete execute-tool and executable-tool helper parity, Rust abort-signal counterparts for tool execution, sandbox command options, tool input lifecycle callbacks, tool constructor input/context/output contracts, provider-facing prepareTools conversion including provider-defined tools, provider options, strict mode, input examples, and context/sandbox-derived descriptions, tool model-output callbacks, function-form approval callbacks, high-level tool variant contracts, and provider-only property exclusion, complete streaming tool-call tracking, and ID generation now live in the matching `ai-sdk-provider-utils` crate. The root crate retains a compatibility re-export shim only. `src/retry.rs` remains `packages/ai` ownership because upstream retry lives under `packages/ai/src/util`, not `packages/provider-utils`. Exact browser `ReadableStream`, Web fetch/runtime integration, the remaining provider-utils `*.test-d.ts` type-level inventory, and true mid-socket preemption for the current blocking `ureq` default transports remain incomplete or JavaScript-runtime-specific. Zod v3/v4 JSON-schema adapter snapshots are now explicitly inventoried as JavaScript/Zod-runtime-specific below instead of being counted as hidden Rust parity debt. |
| `packages/gateway` (`@ai-sdk/gateway`) | provider package | verified | `crates/ai-sdk-gateway`; root facade shims in `src/gateway.rs`, `src/gateway_error.rs`, `src/gateway_tools.rs`, and `src/vercel_ai_gateway.rs` | 380 Gateway provider/error/tool/OpenAI-compatible facade tests in `crates/ai-sdk-gateway`, including `vercel_ai_gateway_openai_compatible_factory_uses_default_base_url`, `vercel_ai_gateway_openai_compatible_implements_provider_trait`, and `vercel_ai_gateway_openai_compatible_auth_token_matches_gateway_precedence`; root high-level Gateway tests in `src/gateway.rs`; `gateway_model_generates_text_through_generate_text`; `gateway_model_generates_object_through_generate_object`; `gateway_model_maps_standard_generate_content_parts`; `gateway_model_maps_standard_generate_content_parts_through_generate_text`; `gateway_model_runs_generate_text_tool_loop_end_to_end`; `gateway_model_streams_text_through_stream_text`; `gateway_model_streams_object_through_stream_object`; `gateway_model_streams_standard_content_parts_through_stream_text`; `gateway_model_runs_stream_text_tool_loop_end_to_end`; `gateway_model_filters_raw_stream_parts_unless_requested`; `gateway_model_encodes_language_prompt_file_bytes_for_generate`; `gateway_model_encodes_language_prompt_file_bytes_for_stream`; `gateway_provider_options_serialize_upstream_shape`; `gateway_provider_options_validation_matches_timeout_schema`; `gateway_model_passes_typed_gateway_provider_options_for_generate`; `gateway_model_passes_typed_gateway_provider_options_for_stream`; `gateway_embedding_model_embeds_through_embed`; `gateway_embedding_model_maps_gateway_error_to_metadata`; `gateway_image_model_generates_through_generate_image`; `gateway_image_model_maps_upstream_request_response_and_metadata`; `gateway_image_model_preserves_metadata_entries_without_images`; `gateway_image_model_encodes_files_and_mask`; `gateway_image_model_maps_gateway_error_to_metadata`; `gateway_reranking_model_reranks_through_rerank`; `gateway_reranking_model_omits_optional_body_fields`; `gateway_reranking_model_maps_gateway_error_to_metadata`; `gateway_video_model_generates_through_generate_video`; `gateway_video_model_preserves_empty_and_nested_provider_metadata`; `gateway_video_model_encodes_image_inputs_and_returns_url_videos`; `gateway_video_model_maps_sse_error_to_metadata`; `gateway_error_types_expose_upstream_names_status_and_retryability`; `gateway_authentication_error_matches_default_and_custom_upstream_values`; `gateway_authentication_contextual_error_matches_upstream_matrix`; `gateway_invalid_request_error_matches_default_custom_and_variant_checks`; `gateway_rate_limit_error_matches_default_and_variant_checks`; `gateway_model_not_found_error_matches_default_custom_and_variant_checks`; `gateway_internal_server_error_matches_default_custom_and_variant_checks`; `gateway_retryability_matches_upstream_status_matrix`; `gateway_response_error_matches_default_custom_and_variant_checks`; `create_gateway_error_from_response_maps_gateway_error_types`; `create_gateway_error_from_response_preserves_empty_auth_messages_with_context`; `create_gateway_error_from_response_uses_default_message_for_null_message`; `create_gateway_error_from_response_handles_null_error_type_as_internal`; `create_gateway_error_from_response_includes_cause_message`; `create_gateway_error_from_response_maps_malformed_responses`; `create_gateway_error_from_response_handles_model_not_found_param_edges`; `create_gateway_error_from_response_ignores_extra_fields`; `create_gateway_error_from_response_preserves_error_properties`; `create_gateway_error_from_response_maps_generation_id_to_error_variants`; `create_gateway_error_from_response_creates_contextual_auth_errors`; `extract_gateway_api_call_response_prefers_data_then_json_then_raw_body`; `extract_gateway_api_call_response_prefers_explicit_data_even_null_or_empty`; `extract_gateway_api_call_response_parses_json_or_returns_raw_text`; `extract_gateway_api_call_response_returns_empty_object_without_body`; `extract_gateway_api_call_response_parses_scalar_and_array_bodies`; `as_gateway_error_detects_all_undici_timeout_codes`; `as_gateway_error_maps_non_timeout_original_errors_to_response_errors`; `gateway_auth_method_header_matches_upstream_name`; `parse_gateway_auth_method_accepts_only_gateway_values`; `parse_gateway_auth_method_accepts_valid_values_and_extra_headers`; `parse_gateway_auth_method_rejects_invalid_values`; `parse_gateway_auth_method_returns_none_for_missing_or_nullish_headers`; `parse_gateway_auth_method_rejects_whitespace`; `get_gateway_auth_token_matches_upstream_precedence`; `get_gateway_auth_token_ignores_empty_values_without_trimming_whitespace`; `get_gateway_auth_token_handles_no_auth_at_all`; `get_gateway_auth_token_handles_valid_oidc_invalid_api_key`; `get_gateway_auth_token_handles_invalid_oidc_valid_api_key`; `get_gateway_auth_token_handles_no_oidc_invalid_api_key`; `get_gateway_auth_token_handles_no_oidc_valid_api_key`; `get_gateway_auth_token_handles_valid_oidc_no_api_key`; `get_gateway_auth_token_handles_valid_oidc_valid_api_key`; `get_gateway_auth_token_handles_valid_oidc_valid_options_api_key`; `get_gateway_auth_token_handles_invalid_oidc_invalid_api_key`; `get_gateway_auth_token_treats_empty_environment_variables_as_missing`; `get_gateway_auth_token_uses_whitespace_environment_api_key`; `get_gateway_auth_token_prioritizes_options_api_key_over_all_environment_variables`; `get_gateway_auth_token_prefers_options_api_key_over_ai_gateway_api_key`; `get_gateway_auth_token_prefers_ai_gateway_api_key_over_oidc_token`; `get_gateway_auth_token_falls_back_to_oidc_when_no_api_keys_are_available`; `gateway_provider_headers_support_oidc_auth_method`; `gateway_observability_headers_map_vercel_environment`; `gateway_observability_headers_skip_empty_values_and_use_request_env_fallback`; `create_gateway_language_model_uses_custom_configuration`; `create_gateway_language_model_uses_oidc_when_api_key_is_absent`; `gateway_provider_language_model_handles_model_specification_errors`; `gateway_provider_language_model_accepts_any_model_id`; `gateway_provider_language_model_accepts_non_existent_model_id`; `create_gateway_embedding_model_returns_gateway_embedding_model`; `create_gateway_image_model_uses_custom_base_url`; `create_gateway_image_model_reuses_headers_transport_and_observability`; `create_gateway_video_model_uses_custom_base_url`; `create_gateway_video_model_reuses_headers_transport_and_observability`; `create_gateway_reranking_model_uses_custom_base_url`; `create_gateway_reranking_alias_returns_gateway_reranking_model`; `create_gateway_fetches_available_models_with_custom_base_url`; `create_gateway_caches_metadata_for_configured_refresh_interval`; `create_gateway_uses_default_five_minute_metadata_refresh_interval`; `create_gateway_language_model_passes_observability_headers_from_environment`; `create_gateway_language_model_omits_missing_observability_headers`; `default_gateway_export_exposes_provider_instance`; `create_gateway_uses_default_base_url_when_none_is_provided`; `create_gateway_accepts_empty_options`; `default_gateway_export_constructs_image_model`; `default_gateway_export_constructs_video_model`; `create_gateway_overrides_default_base_url_when_provided`; `create_gateway_prefers_api_key_over_oidc_token`; `gateway_provider_real_world_vercel_deployment_uses_oidc_authentication`; `gateway_provider_real_world_local_development_uses_api_key_authentication`; `gateway_provider_real_world_explicit_api_key_override_wins_over_environment`; `create_gateway_authentication_handles_no_auth_at_all`; `create_gateway_authentication_handles_valid_oidc_invalid_api_key`; `create_gateway_authentication_handles_invalid_oidc_valid_api_key`; `create_gateway_authentication_handles_no_oidc_invalid_api_key`; `create_gateway_authentication_handles_no_oidc_valid_api_key`; `create_gateway_authentication_handles_valid_oidc_no_api_key`; `create_gateway_authentication_handles_valid_oidc_valid_api_key`; `create_gateway_authentication_handles_valid_oidc_valid_options_api_key`; `create_gateway_authentication_handles_invalid_oidc_invalid_api_key`; `gateway_provider_exposes_gateway_tools`; `perplexity_search_tool_factory_matches_gateway_provider_tool_contract`; `parallel_search_tool_factory_matches_gateway_provider_tool_contract`; `gateway_tools_create_provider_executed_perplexity_search_tool`; `gateway_tools_create_provider_executed_parallel_search_tool`; `gateway_fetch_metadata_fetches_available_models_from_correct_endpoint`; `gateway_fetch_metadata_handles_models_with_pricing_information`; `gateway_fetch_metadata_maps_cache_pricing_fields_to_sdk_names`; `gateway_fetch_metadata_handles_models_without_pricing_information`; `gateway_fetch_metadata_handles_mixed_models_with_and_without_pricing`; `gateway_fetch_metadata_handles_models_with_description`; `gateway_fetch_metadata_accepts_top_level_model_type_when_present`; `gateway_fetch_metadata_filters_unknown_model_type_values`; `gateway_fetch_metadata_preserves_all_known_model_type_values`; `gateway_fetch_metadata_keeps_known_models_and_filters_unknown_from_mixed_response`; `gateway_fetch_metadata_passes_headers_correctly`; `gateway_fetch_metadata_handles_api_errors`; `gateway_fetch_metadata_converts_api_call_errors_to_gateway_errors`; `gateway_fetch_metadata_handles_malformed_json_error_responses`; `gateway_fetch_metadata_handles_malformed_response_data`; `gateway_fetch_metadata_rejects_models_with_invalid_pricing_format`; `gateway_fetch_metadata_does_not_double_wrap_existing_gateway_errors`; `gateway_fetch_metadata_handles_rate_limit_server_errors`; `gateway_fetch_metadata_handles_internal_server_errors`; `gateway_fetch_metadata_preserves_error_cause_chain`; `gateway_fetch_metadata_uses_custom_fetch_function_when_provided`; `gateway_fetch_metadata_handles_empty_response`; `gateway_fetch_metadata_fetches_credits_from_correct_endpoint`; `gateway_fetch_metadata_passes_headers_correctly_to_credits_endpoint`; `gateway_fetch_metadata_handles_api_errors_for_credits_endpoint`; `gateway_fetch_metadata_handles_rate_limit_errors_for_credits_endpoint`; `gateway_fetch_metadata_handles_internal_server_errors_for_credits_endpoint`; `gateway_fetch_metadata_handles_malformed_credits_response`; `gateway_fetch_metadata_uses_custom_fetch_function_for_credits`; `gateway_fetch_metadata_converts_credits_api_call_errors_to_gateway_errors`; `gateway_fetch_metadata_handles_credits_malformed_json_error_responses`; `gateway_fetch_metadata_does_not_double_wrap_existing_credit_gateway_errors`; `gateway_fetch_metadata_preserves_credits_error_cause_chain`; `gateway_fetch_metadata_handles_empty_credits_response`; `gateway_provider_creates_embedding_model_aliases`; `gateway_provider_creates_image_model_aliases`; `gateway_provider_creates_reranking_model_aliases`; `gateway_provider_creates_video_model_aliases`; `gateway_provider_implements_provider_traits`; `gateway_provider_fetches_available_models_metadata`; `gateway_provider_caches_available_models_until_refresh`; `gateway_provider_refreshes_available_models_after_refresh_interval`; `gateway_provider_uses_default_metadata_cache_refresh_interval`; `gateway_provider_refreshes_available_models_when_cache_disabled`; `gateway_provider_fetches_credits_from_gateway_origin`; `gateway_provider_get_credits_includes_upstream_headers`; `gateway_provider_get_credits_surfaces_endpoint_errors`; `gateway_provider_get_credits_fetches_successfully`; `gateway_provider_get_credits_handles_authentication_errors`; `gateway_provider_get_credits_uses_custom_base_url`; `gateway_provider_get_credits_uses_oidc_authentication_headers`; `gateway_provider_get_credits_is_available_on_provider_interface`; `gateway_provider_account_methods_use_default_gateway_urls`; `gateway_provider_fetches_spend_report_with_query_params`; `gateway_provider_get_spend_report_fetches_successfully`; `gateway_provider_get_spend_report_passes_params_through`; `gateway_provider_get_spend_report_uses_custom_base_url`; `gateway_provider_get_spend_report_uses_custom_transport`; `gateway_provider_get_spend_report_is_available_on_provider_interface`; `default_gateway_export_get_spend_report_is_available`; `gateway_provider_get_spend_report_surfaces_endpoint_errors`; `gateway_provider_fetches_generation_info_and_unwraps_data`; `gateway_provider_metadata_surfaces_api_errors`; `gateway_provider_metadata_fetch_errors_convert_to_gateway_errors`; `gateway_provider_metadata_gateway_errors_are_not_double_wrapped`; `gateway_provider_account_apis_surface_malformed_json_error_responses`; `gateway_model_maps_gateway_error_to_error_finish_reason`; ignored `live_gateway_openai_generate_text`; ignored `live_gateway_openai_generate_object`; ignored `live_gateway_openai_stream_text`; ignored `live_gateway_openai_stream_object`; ignored `live_gateway_openai_embed`; ignored `live_gateway_openai_generate_image`; ignored `live_gateway_rerank`; ignored `live_gateway_generate_video`; ignored `live_gateway_available_models` | Gateway provider implementation, Gateway OpenAI-compatible/Responses provider factory implementation, error types/classification, portable `gateway-error-types`, `parse-auth-method`, `create-gateway-error`, `extract-api-call-response`, and `as-gateway-error` edge-case mappings, account metadata APIs, model request/response mappers, and provider-executed Parallel Search/Perplexity Search tools now live in the matching `ai-sdk-gateway` crate. The root crate retains compatibility re-export shims plus high-level SDK integration tests for those surfaces. Native AI SDK Gateway language model generation, createGateway language model configuration including OIDC fallback when no API key is configured and language-model arbitrary/non-existent id construction, createGateway embedding/image/video/reranking model factories, image/video header/transport/observability reuse, reranking alias, metadata fetch/cache/default-base/error routing, default provider image/video construction, observability header resolution, API-key precedence, and portable auth scenario/environment edge-case coverage, provider-v4 generated content parsing for text/reasoning/source/file/tool-result/custom parts, high-level generated/streamed content-part mapping, high-level `generate_text`, `generate_object`, `stream_text`, and `stream_object` local tool-loop/object output slices, streaming, language prompt file byte encoding, typed Gateway provider options for routing/BYOK/compliance/quota/timeouts with an upstream-minimum validation helper for `providerTimeouts.byok`, exact language-model raw-chunk filtering, response-metadata timestamp parsing, provider-option passthrough, transport-failure error mapping, and cause-message metadata, language-model abort-signal forwarding to prepared provider API requests, provider-v4 trait lookups for language/embedding/image plus optional reranking/video models, embedding generation, image generation request/response/warnings/usage/provider-metadata parity, reranking, video generation, image/video provider metadata edges, API-key/OIDC auth resolution, Vercel observability headers, available-model metadata discovery with refresh cache expiry, default Gateway metadata/account routing, credit balance success, authentication error handling, metadata and credits endpoint cause preservation, custom-base, OIDC-header, and provider-interface credit cases, credit request headers, spend report success, parameter forwarding, custom-base, custom-transport, provider-interface, and default-export spend report cases, generation info, credit/spend endpoint error propagation, and malformed account API error handling slices now run from `crates/ai-sdk-gateway`. Upstream mocked `getVercelOidcToken` rejection-only cases and `GatewayAuthenticationError` thrown-instance identity checks are JavaScript runtime/mock-specific and documented as non-portable for Rust, which reads the configured token source directly and returns typed transport errors instead of thrown JS class instances. The current upstream Gateway package test corpus is fully mapped: 372 portable upstream cases are represented by the 380-test `ai-sdk-gateway` crate inventory, with JavaScript request-context and class-instance identity cases documented as non-portable. |
| `packages/openai` (`@ai-sdk/openai`) | provider package | in-progress | `src/openai.rs`, `src/open_responses.rs`, `src/openai_compatible.rs` | `openai_provider_creates_chat_model_with_headers_and_base_url`; `openai_provider_language_model_uses_responses_endpoint`; `open_responses_provider_prepares_openai_hosted_tools`; `open_responses_provider_adds_hosted_tool_include_options`; `open_responses_provider_maps_openai_hosted_tool_outputs`; `open_responses_provider_maps_additional_response_tool_items`; `open_responses_provider_maps_text_sources_and_compaction_metadata`; `open_responses_provider_streams_text_sources_reasoning_and_compaction_metadata`; `open_responses_provider_generates_phase_fixture_metadata`; `open_responses_provider_streams_phase_fixture_metadata`; `open_responses_provider_streams_hosted_tool_outputs`; `open_responses_provider_maps_web_search_api_sources`; `open_responses_provider_maps_web_search_missing_action`; `open_responses_provider_streams_web_search_action_query`; `open_responses_provider_streams_web_search_missing_action`; `open_responses_provider_maps_openai_numeric_error_code`; `open_responses_provider_streams_openai_error_event_without_synthetic_message`; `open_responses_provider_streams_additional_tool_items`; `open_responses_provider_streams_tool_input_delta_refinements`; `open_responses_provider_generates_apply_patch_create_file_fixture_request_body`; `open_responses_provider_generates_apply_patch_create_file_fixture_content`; `open_responses_provider_streams_apply_patch_create_file_fixture`; `open_responses_provider_streams_apply_patch_delete_file_fixture`; `open_responses_provider_maps_openai_responses_provider_options_to_request_body`; `open_responses_provider_streams_context_management_options`; `open_responses_provider_warns_for_conversation_with_previous_response_id`; `open_responses_provider_maps_openai_passthrough_option_edges`; `open_responses_provider_falls_back_to_openai_options_for_azure_requests`; `open_responses_provider_prefers_azure_options_over_openai_fallback`; `open_responses_provider_uses_azure_metadata_key_for_text_result`; `open_responses_provider_uses_azure_metadata_key_for_function_call_content`; `open_responses_provider_streams_azure_metadata_key_for_reasoning_and_finish`; `open_responses_provider_adds_encrypted_reasoning_include_for_reasoning_store_false`; `open_responses_provider_omits_encrypted_reasoning_include_for_non_reasoning_store_false`; `open_responses_provider_omits_encrypted_reasoning_include_for_store_true`; `open_responses_provider_allows_force_reasoning_for_unrecognized_model_ids`; `open_responses_provider_sends_xhigh_reasoning_effort_for_codex_max_model`; `open_responses_provider_warns_for_reasoning_effort_on_non_reasoning_models`; `open_responses_provider_applies_openai_model_capability_rules`; `open_responses_provider_validates_openai_service_tier_model_capabilities`; `open_responses_provider_maps_openai_system_message_modes`; `open_responses_provider_reconstructs_hosted_tool_search_history_with_store_false`; `open_responses_provider_reconstructs_client_tool_search_output_with_store_false`; `open_responses_provider_warns_for_unstored_hosted_tool_results`; `open_responses_provider_reconstructs_local_shell_history_with_store_false`; `open_responses_provider_reconstructs_shell_history_with_store_false`; `open_responses_provider_reconstructs_stored_assistant_shell_outputs`; `open_responses_provider_reconstructs_apply_patch_history_with_store_false`; `open_responses_provider_reconstructs_stored_apply_patch_outputs`; `open_responses_provider_reconstructs_custom_tool_calls`; `open_responses_provider_reconstructs_custom_tool_outputs`; `open_responses_provider_converts_tool_result_file_content_outputs`; `openai_provider_creates_embedding_model_aliases`; `openai_provider_creates_completion_and_image_models`; `openai_completion_should_extract_text_response`; `openai_completion_should_extract_usage`; `openai_completion_should_send_request_body`; `openai_completion_should_send_additional_response_information`; `openai_completion_should_extract_logprobs`; `openai_completion_should_extract_finish_reason`; `openai_completion_should_support_unknown_finish_reason`; `openai_completion_should_expose_the_raw_response_headers`; `openai_completion_should_pass_the_model_and_the_prompt`; `openai_completion_should_pass_headers`; `openai_completion_stream_should_stream_text_deltas`; `openai_completion_stream_should_handle_error_stream_parts`; `openai_completion_stream_should_handle_unparsable_stream_parts`; `openai_completion_stream_should_send_request_body`; `openai_completion_stream_should_expose_the_raw_response_headers`; `openai_completion_stream_should_pass_the_model_and_the_prompt`; `openai_completion_stream_should_pass_headers`; `openai_embedding_should_extract_embedding`; `openai_embedding_should_expose_the_raw_response_headers`; `openai_embedding_should_expose_the_raw_response_body`; `openai_embedding_should_extract_usage`; `openai_embedding_should_pass_the_model_and_the_values`; `openai_embedding_should_pass_the_dimensions_setting`; `openai_embedding_should_pass_headers`; `openai_provider_uses_default_base_url_name_override_and_provider_trait`; `openai_provider_settings_serde_accepts_upstream_base_url_name`; `openai_provider_uses_the_default_openai_base_url_when_not_provided`; `openai_provider_uses_openai_base_url_when_set`; `openai_provider_prefers_the_base_url_option_over_openai_base_url`; `openai_files_should_send_correct_multipart_request_with_purpose`; `openai_files_should_return_provider_reference_with_openai_key`; `openai_files_should_return_provider_metadata_from_response`; `openai_files_should_default_purpose_to_assistants_when_not_provided`; `openai_files_should_pass_expires_after_when_provided`; `openai_files_should_pass_auth_headers`; `openai_files_should_handle_base64_string_data`; `openai_files_should_set_specification_version_and_provider`; `openai_skills_should_send_files_as_multipart_form_data`; `openai_skills_should_pass_authorization_headers`; `openai_skills_should_map_response_to_provider_reference`; `openai_skills_should_emit_unsupported_warning_for_display_title`; `openai_skills_should_return_no_warnings_when_display_title_is_not_set`; `openai_skills_should_handle_uint8array_file_content`; `openai_skills_should_set_specification_version_and_provider`; `openai_speech_should_pass_the_model_and_text`; `openai_speech_should_pass_headers`; `openai_speech_should_pass_options`; `openai_speech_should_return_audio_data_with_correct_content_type`; `openai_speech_should_include_response_data_with_timestamp_model_id_and_headers`; `openai_speech_should_use_real_date_when_no_custom_date_provider_is_specified`; `openai_speech_should_handle_different_audio_formats`; `openai_speech_should_include_warnings_if_any_are_generated`; `openai_speech_should_set_specification_version_and_provider`; `openai_transcription_should_pass_the_model`; `openai_transcription_should_pass_headers`; `openai_transcription_should_extract_the_transcription_text`; `openai_transcription_should_include_response_data_with_timestamp_model_id_and_headers`; `openai_transcription_should_use_real_date_when_no_custom_date_provider_is_specified`; `openai_transcription_should_pass_response_format_when_timestamp_granularities_is_set`; `openai_transcription_should_not_set_verbose_json_for_gpt_4o_transcribe`; `openai_transcription_should_pass_timestamp_granularities_when_specified`; `openai_transcription_should_work_when_no_words_language_or_duration_are_returned`; `openai_transcription_should_parse_segments_when_provided_in_response`; `openai_transcription_should_fallback_to_words_when_segments_are_not_available`; `openai_transcription_should_handle_empty_segments_array`; `openai_transcription_should_handle_segments_with_missing_optional_fields`; `openai_transcription_should_set_specification_version_and_provider`; `openai_image_should_pass_the_model_and_the_settings`; `openai_image_should_map_provider_options_to_snake_case_for_images_generations`; `openai_image_should_pass_headers`; `openai_image_should_extract_the_generated_images`; `openai_image_should_return_warnings_for_unsupported_settings`; `openai_image_should_respect_max_images_per_call_setting`; `openai_image_should_include_response_data_with_timestamp_model_id_and_headers`; `openai_image_should_use_real_date_when_no_custom_date_provider_is_specified`; `openai_image_should_not_include_response_format_for_gpt_image_1`; `openai_image_should_not_include_response_format_for_gpt_image_2`; `openai_image_should_not_include_response_format_for_chatgpt_image_latest`; `openai_image_should_not_include_response_format_for_date_suffixed_gpt_image_model_ids`; `openai_image_should_handle_null_revised_prompt_responses`; `openai_image_should_include_response_format_for_dall_e_3`; `openai_image_should_return_image_meta_data`; `openai_image_should_map_openai_usage_to_usage`; `openai_image_should_distribute_input_token_details_evenly_across_images`; `openai_image_should_call_images_edits_endpoint_when_files_are_provided`; `openai_image_should_send_image_as_form_data_with_uint8array_input`; `openai_image_should_send_image_as_form_data_with_base64_string_input`; `openai_image_should_send_multiple_images_as_form_data_array`; `openai_image_should_pass_provider_options_in_form_data`; `openai_image_should_map_provider_options_to_snake_case_for_images_edits`; `openai_image_should_extract_the_edited_images_from_response`; `openai_image_should_include_response_metadata_for_edited_images`; `openai_image_should_return_warnings_for_unsupported_settings_in_edit_mode`; `openai_image_should_return_usage_information_for_edited_images` | Initial provider foundation mirrors upstream `createOpenAI` settings for default/custom base URL, `OPENAI_BASE_URL`, `OPENAI_API_KEY`, organization/project/custom headers, provider name override, OpenAI user-agent suffix, `openai(...)`/`languageModel`/`responses` over the Responses endpoint, OpenAI Responses hosted/provider-defined tool request preparation with automatic `include` additions for hosted web-search sources and code-interpreter outputs, OpenAI Responses provider-option request-key normalization including `instructions`, multi-value `include`, `user`, `conversation`, `metadata`, `store`, `truncation`, numeric `logprobs`, streaming `contextManagement` compaction forwarding, conversation/previous-response conflict warnings, Azure fallback to `providerOptions.openai` when `providerOptions.azure` is absent with Azure metadata retained and Azure-specific options taking precedence, Azure provider-metadata key coverage for non-streaming text results, non-streaming function calls, plus streaming reasoning and finish events, OpenAI Responses error data mapping for numeric error codes plus stream error-event finish reasons, OpenAI Responses model capability rules for `forceReasoning`, Codex Max `xhigh` reasoning effort, and reasoning-model temperature/topP stripping, `reasoningEffort`/`reasoningSummary` rejection on non-reasoning models, dedicated upstream non-reasoning model matrix coverage, dedicated `store: false`/`store: true` encrypted reasoning include request tests, service-tier support validation, and OpenAI system-message request shaping including reasoning-model `developer` role defaults plus `systemMessageMode` overrides/removal warnings, non-streaming Responses output mapping for web search including API-typed sources and missing-action resilience, file search, code interpreter, image generation, tool search, local/shell calls and outputs, apply-patch calls, MCP calls and approval requests, computer calls, custom tool calls, text/reasoning provider metadata including phase, annotation sources, compaction custom content, streaming text/source/reasoning/phase/compaction metadata edges, streaming hosted web-search/file-search/code-interpreter/image-generation tool calls and results including API-typed sources and missing-action resilience, streaming custom/function tool input deltas, streamed tool item provider metadata (`itemId`/`namespace`), code-interpreter code deltas, apply-patch diff deltas, image-generation preliminary partial-image results, streaming custom/tool-search/local-shell/shell/apply-patch/MCP/computer-use item mapping, prompt-history reconstruction for server/client `tool_search`, local-shell, shell, apply-patch, and custom provider tool calls/outputs, stored assistant shell-output and apply-patch output reconstruction, tool-result image-detail provider option forwarding, hosted-result warning/skip behavior for unsupported provider-executed tools when not stored, chat/completion/embedding aliases over the existing OpenAI-compatible transport, OpenAI completion `/completions` non-stream and streaming request/result/logprobs/header parity, plus a dedicated OpenAI image model for `/images/generations` and `/images/edits`, and Files plus Skills upload multipart request/result parity, plus Speech `/audio/speech` JSON request/binary response parity and Transcription `/audio/transcriptions` multipart request/JSON response parity, plus OpenAI image generation/edit request shaping, max image limits, response-format defaults, warnings, response metadata, provider metadata, and usage-token distribution parity. Full Responses streaming/tool matrix, and provider-specific error mappings outside Responses remain unported. |
| `packages/openai-compatible` (`@ai-sdk/openai-compatible`) | provider base package | verified | `crates/ai-sdk-openai-compatible`; root facade shim in `src/openai_compatible.rs`; Vercel AI Gateway integration through `crates/ai-sdk-gateway/src/vercel_ai_gateway.rs` with root tests in `src/vercel_ai_gateway.rs` | `openai_compatible_provider_configures_headers_urls_and_model_aliases`; `openai_compatible_provider_lists_models`; `openai_compatible_provider_retrieves_model_by_id`; `openai_compatible_chat_generates_text_through_generate_text`; `openai_compatible_chat_streams_text_through_stream_text`; `openai_compatible_chat_streams_reasoning_raw_chunks_and_parse_errors`; `openai_compatible_chat_passes_tools_tool_choice_and_provider_options`; `openai_compatible_chat_converts_multimodal_user_messages`; `openai_compatible_chat_rejects_unsupported_file_messages_before_transport`; `openai_compatible_chat_converts_assistant_tool_history`; `openai_compatible_chat_runs_generate_text_tool_loop_end_to_end`; `openai_compatible_chat_runs_stream_text_tool_loop_end_to_end`; `openai_compatible_chat_maps_tool_calls_from_generate`; `openai_compatible_chat_streams_tool_calls`; `openai_compatible_embedding_model_embeds_through_embed_many`; `openai_compatible_embedding_model_passes_options_and_errors`; `openai_compatible_completion_generates_text_through_generate_text`; `openai_compatible_completion_streams_text_through_stream_text`; `openai_compatible_completion_passes_options_warnings_and_errors`; `openai_compatible_image_model_generates_through_generate_image`; `openai_compatible_image_model_edits_with_files_and_mask`; `openai_compatible_image_model_passes_options_warnings_and_errors`; `to_camel_case_upstream_should_convert_hyphenated_names_to_camel_case`; `to_camel_case_upstream_should_convert_underscored_names_to_camel_case`; `to_camel_case_upstream_should_handle_multiple_separators`; `to_camel_case_upstream_should_return_same_string_when_already_camel_case`; `to_camel_case_upstream_should_return_same_string_when_no_separators`; `to_camel_case_upstream_should_handle_empty_string`; `resolve_provider_options_key_upstream_should_return_camel_case_key_when_camel_case_options_present`; `resolve_provider_options_key_upstream_should_return_raw_key_when_only_raw_options_present`; `resolve_provider_options_key_upstream_should_return_camel_case_key_when_both_are_present`; `resolve_provider_options_key_upstream_should_return_raw_key_when_no_options_are_present`; `resolve_provider_options_key_upstream_should_return_raw_key_when_provider_options_is_undefined`; `resolve_provider_options_key_upstream_should_return_raw_key_when_name_has_no_separators`; `deprecated_provider_options_key_upstream_should_push_warning_when_raw_key_is_used_and_differs`; `deprecated_provider_options_key_upstream_should_not_warn_when_only_camel_case_key_is_used`; `deprecated_provider_options_key_upstream_should_not_warn_when_raw_name_is_already_camel_case`; `deprecated_provider_options_key_upstream_should_not_warn_when_raw_key_is_not_present`; `deprecated_provider_options_key_upstream_should_not_warn_when_provider_options_is_undefined`; `openai_compatible_chat_maps_response_formats_and_warnings`; `openai_compatible_chat_injects_json_instruction_when_response_format_body_is_disabled`; `vercel_ai_gateway_openai_compatible_generates_text_through_openai_chat`; `vercel_ai_gateway_openai_compatible_generates_object_through_openai_chat`; `vercel_ai_gateway_openai_compatible_streams_object_through_openai_chat`; `vercel_ai_gateway_openai_compatible_runs_generate_text_tool_loop_end_to_end`; `vercel_ai_gateway_openai_compatible_streams_text_through_openai_chat`; `vercel_ai_gateway_openai_compatible_runs_stream_text_tool_loop_end_to_end`; `vercel_ai_gateway_openai_compatible_embeds_through_openai_embeddings`; `vercel_ai_gateway_openai_compatible_generates_images_through_openai_images_endpoint`; `vercel_ai_gateway_openai_compatible_maps_chat_image_outputs_through_generate_text`; `vercel_ai_gateway_openai_compatible_streams_chat_image_outputs_through_stream_text`; `vercel_ai_gateway_openai_compatible_implements_provider_trait`; ignored `live_vercel_ai_gateway_openai_compatible_generate_text`; ignored `live_vercel_ai_gateway_openai_compatible_generate_text_tool_loop`; ignored `live_vercel_ai_gateway_openai_compatible_stream_text`; ignored `live_vercel_ai_gateway_openai_compatible_generate_object`; ignored `live_vercel_ai_gateway_openai_compatible_stream_object`; ignored `live_vercel_ai_gateway_openai_compatible_embed`; ignored `live_vercel_ai_gateway_openai_compatible_generate_image`; ignored `live_vercel_ai_gateway_openai_compatible_generate_text_with_image_output` | Initial provider foundation, model factory aliases, request headers/URLs/query params, non-streaming chat `/chat/completions`, chat provider options (`user`, `reasoningEffort`, `textVerbosity`, `strictJsonSchema`, custom provider body passthrough), OpenAI-compatible prompt conversion for multimodal user content, assistant reasoning/tool-call history, tool-result messages, provider metadata, Google thought signatures, high-level `generate_text`, `generate_object`, `stream_object`, and `stream_text` tool-loop continuation over OpenAI-compatible chat, function-tool/tool-choice request shaping, non-streaming tool-call response mapping, chat SSE streaming for text/reasoning/raw chunks/usage/tool calls, JSON instruction injection for providers that reject the OpenAI `response_format` body field, embedding `/embeddings` calls, completion `/completions` generate/stream calls, image `/images/generations` plus `/images/edits`, and exact upstream `to-camel-case.test.ts` raw/camel provider-option key normalization tests for chat/completion/embedding/image provider-option paths exist. Vercel AI Gateway's OpenAI-compatible base URL is covered by text, local tool-loop continuation, structured object parsing, streamed object parsing, streaming text with local tool-loop continuation, embedding slices, image generation over `/images/generations`, chat image-output mapping from `message.images` and `delta.images` into generated files, and provider-v4 trait integration for `openai/...` and image-capable Gateway model ids with optional `.env.local` live validation for text, tools, objects, streaming objects, streaming text, embeddings, and images. All 8 current upstream `packages/openai-compatible` test files are enumerated in verified detailed rows below, covering 230 portable upstream cases with named Rust counterparts and additional Rust integration/live Gateway coverage on top. The OpenAI-compatible provider implementation and direct provider tests live in the matching `ai-sdk-openai-compatible` crate, with the root module reduced to a compatibility re-export shim plus existing root API integration tests. |
| `packages/open-responses` (`@ai-sdk/open-responses`) | provider package | in-progress | `crates/ai-sdk-open-responses`; root facade shim in `src/open_responses.rs` | `open_responses_provider_generates_text_with_request_and_response_metadata`; `open_responses_provider_converts_user_file_prompt_parts`; `open_responses_provider_sends_pdf_input_file_request_body`; `open_responses_provider_produces_pdf_input_file_content`; `open_responses_provider_extracts_pdf_input_file_usage`; `open_responses_provider_streams_pdf_input_file_fixture`; `open_responses_provider_sends_lmstudio_basic_request_body`; `open_responses_provider_produces_lmstudio_basic_content`; `open_responses_provider_extracts_lmstudio_basic_usage`; `open_responses_provider_streams_lmstudio_basic_content`; `open_responses_provider_streams_text_with_request_and_response_metadata`; `open_responses_provider_generates_object_with_json_schema_response_format`; `open_responses_provider_prepares_openai_hosted_tools`; `open_responses_provider_adds_hosted_tool_include_options`; `open_responses_provider_maps_openai_hosted_tool_outputs`; `open_responses_provider_maps_additional_response_tool_items`; `open_responses_provider_maps_text_sources_and_compaction_metadata`; `open_responses_provider_streams_text_sources_reasoning_and_compaction_metadata`; `open_responses_provider_generates_phase_fixture_metadata`; `open_responses_provider_streams_phase_fixture_metadata`; `open_responses_provider_streams_hosted_tool_outputs`; `open_responses_provider_maps_web_search_api_sources`; `open_responses_provider_maps_web_search_missing_action`; `open_responses_provider_streams_web_search_action_query`; `open_responses_provider_streams_web_search_missing_action`; `open_responses_provider_maps_openai_numeric_error_code`; `open_responses_provider_streams_openai_error_event_without_synthetic_message`; `open_responses_provider_streams_additional_tool_items`; `open_responses_provider_streams_tool_input_delta_refinements`; `open_responses_provider_generates_apply_patch_create_file_fixture_request_body`; `open_responses_provider_generates_apply_patch_create_file_fixture_content`; `open_responses_provider_streams_apply_patch_create_file_fixture`; `open_responses_provider_streams_apply_patch_delete_file_fixture`; `open_responses_provider_maps_openai_responses_provider_options_to_request_body`; `open_responses_provider_streams_context_management_options`; `open_responses_provider_warns_for_conversation_with_previous_response_id`; `open_responses_provider_maps_openai_passthrough_option_edges`; `open_responses_provider_falls_back_to_openai_options_for_azure_requests`; `open_responses_provider_prefers_azure_options_over_openai_fallback`; `open_responses_provider_uses_azure_metadata_key_for_text_result`; `open_responses_provider_uses_azure_metadata_key_for_function_call_content`; `open_responses_provider_streams_azure_metadata_key_for_reasoning_and_finish`; `open_responses_provider_adds_encrypted_reasoning_include_for_reasoning_store_false`; `open_responses_provider_omits_encrypted_reasoning_include_for_non_reasoning_store_false`; `open_responses_provider_omits_encrypted_reasoning_include_for_store_true`; `open_responses_provider_allows_force_reasoning_for_unrecognized_model_ids`; `open_responses_provider_sends_xhigh_reasoning_effort_for_codex_max_model`; `open_responses_provider_warns_for_reasoning_effort_on_non_reasoning_models`; `open_responses_provider_applies_openai_model_capability_rules`; `open_responses_provider_validates_openai_service_tier_model_capabilities`; `open_responses_provider_sends_instructions_from_system_message`; `open_responses_provider_joins_multiple_system_messages_with_newlines`; `open_responses_provider_converts_openai_message_chain_with_system_input_items`; `open_responses_provider_maps_openai_system_message_modes`; `open_responses_provider_skips_conversation_history_items`; `open_responses_provider_reconstructs_hosted_tool_search_history_with_store_false`; `open_responses_provider_reconstructs_client_tool_search_output_with_store_false`; `open_responses_provider_warns_for_unstored_hosted_tool_results`; `open_responses_provider_reconstructs_local_shell_history_with_store_false`; `open_responses_provider_reconstructs_shell_history_with_store_false`; `open_responses_provider_reconstructs_stored_assistant_shell_outputs`; `open_responses_provider_reconstructs_apply_patch_history_with_store_false`; `open_responses_provider_reconstructs_stored_apply_patch_outputs`; `open_responses_provider_reconstructs_custom_tool_calls`; `open_responses_provider_reconstructs_custom_tool_outputs`; `open_responses_provider_converts_standard_tool_result_outputs`; `open_responses_provider_sends_lmstudio_request_parameters_body`; `open_responses_provider_sends_lmstudio_tools_request_body`; `open_responses_provider_sends_tool_choice_auto`; `open_responses_provider_sends_tool_choice_none`; `open_responses_provider_sends_tool_choice_required`; `open_responses_provider_sends_tool_choice_specific_tool`; `open_responses_provider_maps_allowed_tools_required_mode`; `open_responses_provider_maps_function_call_response_and_usage`; `open_responses_provider_resolves_provider_reference_file_parts`; `open_responses_provider_rejects_missing_provider_reference_file_part`; `open_responses_provider_converts_tool_result_file_content_outputs`; `open_responses_provider_maps_api_error_data_to_metadata_and_response`; `open_responses_provider_preserves_stream_error_event_data`; `open_responses_provider_runs_generate_text_tool_loop_end_to_end`; `open_responses_streams_function_call_argument_deltas`; `open_responses_provider_reports_unsupported_embedding_and_image`; `open_responses_provider_maps_top_level_reasoning_high_to_effort`; `open_responses_provider_maps_top_level_reasoning_minimal_to_low`; `open_responses_provider_maps_top_level_reasoning_none_to_none`; `open_responses_provider_passes_top_level_reasoning_xhigh_directly`; `open_responses_provider_omits_reasoning_when_not_specified`; `open_responses_provider_sends_detailed_reasoning_summary_from_provider_options`; `open_responses_provider_combines_top_level_reasoning_with_summary`; `open_responses_provider_sends_concise_reasoning_summary_from_provider_options`; `open_responses_provider_omits_reasoning_for_empty_provider_options`; additive `open_responses_provider_filters_non_reasoning_generic_provider_options`; `open_responses_finish_reason_undefined_with_tool_calls_maps_tool_calls`; `open_responses_finish_reason_null_with_tool_calls_maps_tool_calls`; `open_responses_finish_reason_undefined_without_tool_calls_maps_stop`; `open_responses_finish_reason_null_without_tool_calls_maps_stop`; `open_responses_finish_reason_max_output_tokens_maps_length`; `open_responses_finish_reason_content_filter_maps_content_filter`; `open_responses_finish_reason_unknown_with_tool_calls_maps_tool_calls`; `open_responses_finish_reason_unknown_without_tool_calls_maps_other`; additive `open_responses_finish_reason_maps_legacy_max_tokens_to_length`; `open_responses_provider_maps_upstream_multi_turn_tool_conversation_fixture`; `open_responses_provider_streams_mcp_approval_request_fixture_turn_1`; `open_responses_provider_streams_mcp_approval_denial_fixture_turn_2`; `open_responses_provider_streams_mcp_approval_retry_fixture_turn_3`; `open_responses_provider_streams_mcp_approval_result_fixture_turn_4`; `openai_provider_language_model_uses_responses_endpoint` | Initial Open Responses provider settings, provider id, generic system instructions plus OpenAI/Gateway system/developer/removal system-message modes, basic user/assistant/tool message-chain conversion, upstream multi-turn tool conversation request shaping, standard tool-result text/JSON/error/denied output conversion, LMStudio request parameters including presence/frequency penalties and JSON schema text-format mapping, generic LMStudio function tool request shaping, basic function tool-choice modes including `allowed_tools` required mode, non-streaming function-call response and usage-detail mapping, user-agent suffix, bearer/custom headers, non-streaming and SSE `/responses` request body for text, image, and file prompts including provider-reference image/PDF file ids with missing-provider errors plus upstream PDF input-file fixture response and stream deltas, LMStudio basic generation request-body, content, usage, and basic streaming fixture mapping, generic provider-options filtering plus OpenAI/Azure/Gateway body passthrough with OpenAI wrapper request-key normalization and request option edges for `instructions`, `include`, `user`, `conversation`, `metadata`, `store`, `truncation`, and `logprobs`, streaming `contextManagement` compaction forwarding, conversation/previous-response conflict warnings, Azure fallback to `providerOptions.openai` when `providerOptions.azure` is absent with Azure metadata retained and Azure-specific options taking precedence, Azure provider-metadata key coverage for non-streaming text results, non-streaming function calls, plus streaming reasoning and finish events, top-level/provider reasoning request option matrix, OpenAI/Azure/Gateway model capability rules for `forceReasoning`, Codex Max `xhigh` reasoning effort, and reasoning-model temperature/topP stripping, non-reasoning reasoning-option warnings with dedicated upstream non-reasoning model matrix coverage, dedicated `store: false`/`store: true` encrypted reasoning include request tests, and service-tier validation, JSON schema response-format request shaping, high-level object generation, function tool request shaping including OpenAI hosted/provider-defined tool request preparation, hosted web-search/code-interpreter automatic include options, and hosted tool-choice name mapping, `tool_choice`, tool-result `function_call_output` continuation, response text/reasoning/tool-call extraction, non-streaming provider-executed hosted tool-call/tool-result output mapping for web search including API-typed sources and missing-action resilience, file search, code interpreter, image generation, tool search, local/shell calls and outputs, apply-patch calls, MCP calls and approval requests, computer calls, custom tool calls, text/reasoning provider metadata including phase, annotation sources, compaction custom content, server/client `tool_search` prompt-history reconstruction for `store: false`, local-shell prompt-history reconstruction for `local_shell_call` and `local_shell_call_output`, shell prompt-history reconstruction for `shell_call` and `shell_call_output`, apply-patch prompt-history reconstruction for `apply_patch_call` and `apply_patch_call_output`, custom provider-tool prompt-history reconstruction for `custom_tool_call` and `custom_tool_call_output`, stored assistant shell-output and apply-patch output reconstruction, tool-result image-detail provider option forwarding, hosted-result warning/skip behavior for unsupported provider-executed tools when not stored, streamed function-call/custom-tool argument deltas, streamed function/custom/tool-search/local-shell/shell/apply-patch/MCP provider metadata (`itemId`/`namespace`), MCP approval request/denial/retry/approval streaming fixtures, streaming text/reasoning/source/phase metadata, streaming compaction custom content, streaming hosted web-search/file-search/code-interpreter/image-generation tool calls and results, web-search API-typed source and missing-action resilience, code-interpreter code deltas, and preliminary partial-image results, streaming apply-patch diff input deltas, streaming custom/tool-search/local-shell/shell/apply-patch/MCP/computer-use item mapping, raw chunks, parse/error stream parts with provider error payloads, usage/cached/reasoning token mapping, finish reason mapping, response metadata, API error metadata (`type`/`param`/string-or-numeric `code`/status/retryability) and SSE error event raw finish-reason preservation, unsupported embedding/image lookups, and completed function-call stream item mapping are represented. The Open Responses provider implementation and direct package-owned tests now live in the matching `ai-sdk-open-responses` crate. The root module is reduced to a compatibility re-export shim plus root high-level API integration tests for `generate_text`, `generate_object`, and `stream_text`. The remaining structured output/tools and broader Responses streaming matrices remain unported. |
| `packages/anthropic` (`@ai-sdk/anthropic`) | provider package | not-started | none | none | Needs language model, files, cache control, prompt conversion, tool preparation, usage conversion, and error mapping. |
| `packages/amazon-bedrock` (`@ai-sdk/amazon-bedrock`) | provider package | not-started | none | none | Needs chat, embeddings, image, event-stream response handling, SigV4 fetch, tool prep, usage conversion, and model settings. |
| `packages/google` (`@ai-sdk/google`) | provider package | not-started | none | none | Needs Gemini language, embedding, image, video, files, interactions, schema conversion, URL support, tools, and JSON accumulator behavior. |
| `packages/google-vertex` (`@ai-sdk/google-vertex`) | provider package | not-started | none | none | Needs Vertex language, embedding, image, video, Anthropic-on-Vertex, auth variants, and provider tests. |
| `packages/xai` (`@ai-sdk/xai`) | provider package | not-started | none | none | Needs chat/responses, image, video, files, usage conversion, tools, and error mapping. |
| `packages/alibaba` (`@ai-sdk/alibaba`) | provider package | not-started | none | none | Needs chat/video provider, usage conversion, cache control, and message conversion. |
| `packages/assemblyai` (`@ai-sdk/assemblyai`) | provider package | in-progress | `crates/ai-sdk-assemblyai` | `assemblyai_provider_transcribes_audio_with_headers_options_and_response`; `assemblyai_transcription_duration_falls_back_to_last_word_end`; `assemblyai_transcription_model_maps_api_errors_to_metadata`; `assemblyai_provider_reports_unsupported_model_families_and_trait_transcription`; `assemblyai_provider_settings_serde_accepts_upstream_shape` | Initial provider-owned crate mirrors upstream `createAssemblyAI` settings for `ASSEMBLYAI_API_KEY`, custom headers, AssemblyAI user-agent suffix, `assemblyai(...)`/`create_assemblyai`, `transcription`/`transcription_model`, provider-v4 trait integration, `/v2/upload` binary audio upload, `/v2/transcript` submit, transcript polling, upstream AssemblyAI provider options, response text/segments/language/duration metadata including last-word duration fallback, final response headers/body, error metadata, and unsupported language/embedding/image lookups. Workflow serialization hooks, abort handling, live AssemblyAI validation, and broader exact thrown-error parity remain unported. |
| `packages/azure` (`@ai-sdk/azure`) | provider package | in-progress | `crates/ai-sdk-azure`, `src/openai_compatible.rs`, `src/open_responses.rs` | `azure_provider_creates_responses_model_with_resource_url_headers_and_api_version`; `azure_provider_creates_chat_model_with_deployment_based_url`; `azure_provider_creates_completion_model_with_default_v1_url`; `azure_provider_creates_embedding_and_image_models_with_upstream_urls`; `azure_provider_uses_default_aliases_and_provider_trait`; `azure_provider_settings_serde_accepts_upstream_shape` | Initial provider-owned crate mirrors upstream `createAzure` settings for `resourceName`, `baseURL`, `apiKey`, custom headers, `apiVersion`, and `useDeploymentBasedUrls`; `AZURE_API_KEY` and `AZURE_RESOURCE_NAME` lookup; Azure `api-key` auth; Azure user-agent suffix; callable-style `azure(...)`; `language_model`/`responses` over Open Responses with `assistant-` file id prefix; OpenAI-compatible chat, completion, embedding, and image model factories; deployment-based and `/v1` URL construction; custom base URL trimming; provider-v4 trait integration; and upstream `azure.embeddings` provider id. Azure speech, transcription, Azure-specific tools, provider metadata fixtures, broader Responses fixture matrix, SSE edge coverage, and exact Azure error schema mapping remain unported. |
| `packages/baseten` (`@ai-sdk/baseten`) | provider package | in-progress | `src/baseten.rs`, `src/openai_compatible.rs` | `baseten_provider_creates_default_chat_model_with_headers_and_base_url`; `baseten_provider_routes_custom_sync_chat_model_url_and_rejects_predict`; `baseten_provider_creates_embedding_model_for_sync_urls`; `baseten_provider_reports_unsupported_embedding_routes_and_images`; `baseten_provider_uses_default_base_url_and_function_alias`; `baseten_provider_implements_provider_trait`; `baseten_provider_settings_serde_accepts_upstream_urls` | Initial provider foundation mirrors upstream `createBaseten` settings for default/custom Model API base URL, dedicated `modelURL`, `BASETEN_API_KEY`, custom headers, Baseten user-agent suffix, callable-style `baseten(...)`, `languageModel`/`chatModel`, provider-v4 trait integration, OpenAI-compatible `/chat/completions` on default Model APIs and `/sync/v1` custom endpoints, embedding `/embeddings` routing for `/sync` and `/sync/v1` custom endpoints, and unsupported image lookups. Upstream's Baseten-specific top-level error structure and Performance Client embedding override are not fully represented yet; the Rust slice uses the local OpenAI-compatible embedding transport directly. |
| `packages/black-forest-labs` (`@ai-sdk/black-forest-labs`) | provider package | in-progress | `crates/ai-sdk-black-forest-labs` | `black_forest_labs_provider_creates_image_model_with_headers_body_and_metadata`; `black_forest_labs_image_model_derives_aspect_ratio_and_passes_files_mask_and_options`; `black_forest_labs_image_model_maps_api_and_poll_errors_to_metadata`; `black_forest_labs_provider_reports_unsupported_model_families_and_trait_image`; `black_forest_labs_provider_settings_serde_accepts_upstream_shape` | Initial provider-owned crate mirrors upstream `createBlackForestLabs` settings for default/custom base URL, `BFL_API_KEY`, custom `x-key`/provider/request headers, Black Forest Labs user-agent suffix, `image`/`imageModel`, provider-v4 trait integration, unsupported language/embedding lookups, JSON submit to `/{modelId}`, poll URL `id` handling, `status`/`state` ready responses, binary final image fetch, `size` to `aspect_ratio` compatibility warnings, input image and mask conversion, provider-specific image options, response headers, timestamp/model metadata, image provider metadata for seed/timing/cost/megapixels, and API/poll error metadata. Pending-status retry timing, workflow serialization hooks, broader provider-option validation parity, live BFL validation, and exact thrown-error behavior remain unported. |
| `packages/bytedance` (`@ai-sdk/bytedance`) | provider package | in-progress | `crates/ai-sdk-bytedance` | `bytedance_video_model_generates_video_with_headers_body_and_metadata`; `bytedance_video_model_passes_unmapped_resolution_and_url_image`; `bytedance_video_model_maps_api_and_status_errors_to_metadata`; `bytedance_provider_reports_unsupported_model_families_and_trait_video`; `bytedance_provider_settings_serde_accepts_upstream_shape` | Initial provider-owned crate mirrors upstream `createByteDance` settings for default/custom base URL, `ARK_API_KEY`, bearer JSON headers, custom provider/request headers, `byteDance`/`createByteDance` via Rust `byte_dance`/`create_byte_dance`, `video`/`video_model`, provider-v4 trait integration, unsupported language/embedding/image lookups, task creation at `/contents/generations/tasks`, status polling, URL video results, response headers, task id and usage provider metadata, standard video prompt/image/aspect/duration/seed/resolution request shaping with upstream resolution mapping, unsupported `fps`/`n` warnings, ByteDance provider options for watermark/audio/camera/last-frame/service-tier/draft/reference media/poll timing, passthrough options, and API/status error metadata. Abort handling, exact thrown-error behavior, timeout edge tests, and live ByteDance validation remain unported. |
| `packages/cerebras` (`@ai-sdk/cerebras`) | provider package | in-progress | `src/cerebras.rs`, `src/openai_compatible.rs` | `cerebras_provider_creates_chat_model_with_headers_base_url_and_structured_outputs`; `cerebras_provider_uses_default_base_url_and_function_alias`; `cerebras_provider_reports_unsupported_model_families`; `cerebras_provider_implements_provider_trait`; `cerebras_provider_settings_serde_accepts_upstream_base_url` | Initial provider foundation mirrors upstream `createCerebras` settings for default/custom base URL, `CEREBRAS_API_KEY`, custom headers, Cerebras user-agent suffix, callable-style `cerebras(...)`, `languageModel`/`chat`, provider-v4 trait integration, OpenAI-compatible `/chat/completions` with structured JSON schema output support, and unsupported embedding/image lookups. Cerebras-specific top-level error-structure mapping and broader provider package tests remain unported. |
| `packages/cohere` (`@ai-sdk/cohere`) | provider package | not-started | none | none | Needs chat, embeddings, reranking, prompt conversion, and tool preparation. |
| `packages/deepgram` (`@ai-sdk/deepgram`) | provider package | in-progress | `crates/ai-sdk-deepgram` | `deepgram_speech_model_sends_headers_body_query_options_and_metadata`; `deepgram_speech_model_maps_format_and_warnings`; `deepgram_transcription_model_sends_audio_query_headers_and_maps_response`; `deepgram_models_map_api_errors_to_metadata`; `deepgram_provider_reports_unsupported_model_families_and_traits`; `deepgram_provider_settings_serde_accepts_upstream_shape_and_default_factory` | Initial provider-owned crate mirrors upstream `createDeepgram` settings for `DEEPGRAM_API_KEY`, `authorization: Token ...`, custom headers, Deepgram user-agent suffix, callable-style default transcription factory, `speech`/`speech_model`, `transcription`/`transcription_model`, provider-v4 speech and transcription trait integration, unsupported language/embedding/text-embedding/image lookups with upstream messages, `/v1/speak` JSON TTS request body plus model/output-format/provider-option query mapping, unsupported voice/speed/language/instructions warnings, `/v1/listen` raw-audio transcription request with media-type header, default `diarize=true`, runtime-sent Deepgram transcription provider options with schema-only fields accepted but not sent, transcript text/word segments/language/duration mapping, response headers/body, and Deepgram error metadata. Workflow serialization hooks, abort handling, live Deepgram validation, exact thrown-error classes, full zod-equivalent validation/warning matrix, and broader option-combination fixture coverage remain unported. |
| `packages/deepinfra` (`@ai-sdk/deepinfra`) | provider package | in-progress | `src/deepinfra.rs`, `src/openai_compatible.rs` | `deepinfra_provider_creates_chat_model_with_headers_and_base_url`; `deepinfra_chat_corrects_reasoning_usage_when_reasoning_exceeds_completion_tokens`; `deepinfra_chat_corrects_stream_finish_reasoning_usage`; `deepinfra_provider_creates_completion_model`; `deepinfra_provider_creates_embedding_model_aliases`; `deepinfra_provider_creates_image_model_and_generates_images`; `deepinfra_image_model_edits_with_files_mask_and_provider_options`; `deepinfra_image_model_maps_generation_api_error_to_metadata`; `deepinfra_image_model_maps_edit_api_error_to_metadata`; `deepinfra_provider_uses_default_base_url_and_function_alias`; `deepinfra_provider_implements_provider_trait`; `deepinfra_provider_settings_serde_accepts_upstream_base_url` | Initial provider foundation mirrors upstream `createDeepInfra` settings for default/custom base URL, `DEEPINFRA_API_KEY`, custom headers, DeepInfra user-agent suffix, callable-style `deepinfra(...)`, provider-v4 trait integration, OpenAI-compatible `/openai/chat/completions`, `/openai/completions`, and `/openai/embeddings` models with provider ids `deepinfra.chat`, `deepinfra.completion`, and `deepinfra.embedding`, plus upstream DeepInfra chat reasoning-token usage correction for non-streaming and streaming responses. DeepInfra image generation covers the custom `/inference/{modelId}` JSON request boundary, and image editing covers the derived `/openai/images/edits` multipart form-data boundary with repeated `image` fields, mask, provider options, base64 result mapping, response headers, and API error metadata. |
| `packages/deepseek` (`@ai-sdk/deepseek`) | provider package | in-progress | `crates/ai-sdk-deepseek` | `deepseek_provider_creates_chat_model_with_headers_and_base_url`; `deepseek_provider_uses_default_base_url_and_function_aliases`; `deepseek_provider_reports_unsupported_model_families`; `deepseek_provider_implements_provider_trait`; `deepseek_provider_settings_serde_accepts_upstream_base_url` | Initial provider-owned crate mirrors upstream `createDeepSeek` settings for default/custom base URL, `DEEPSEEK_API_KEY`, custom headers, DeepSeek user-agent suffix, `deep_seek()`/deprecated `deepseek()` aliases, `language_model`/`chat`, provider-v4 trait integration, OpenAI-compatible `/chat/completions` request routing, and unsupported embedding/image lookups. DeepSeek-specific chat message conversion, reasoning/thinking options, tool preparation, usage/cache metadata, SSE stream parsing, and error schema mapping remain unported. |
| `packages/elevenlabs` (`@ai-sdk/elevenlabs`) | provider package | not-started | none | none | Needs speech, transcription, and error mapping. |
| `packages/fal` (`@ai-sdk/fal`) | provider package | not-started | none | none | Needs image, speech, transcription, video, provider settings, and error mapping. |
| `packages/fireworks` (`@ai-sdk/fireworks`) | provider package | not-started | none | none | Needs image provider and settings. |
| `packages/gladia` (`@ai-sdk/gladia`) | provider package | not-started | none | none | Needs transcription provider and error mapping. |
| `packages/groq` (`@ai-sdk/groq`) | provider package | not-started | none | none | Needs chat, transcription, browser-search tool, usage conversion, message conversion, and tool preparation. |
| `packages/huggingface` (`@ai-sdk/huggingface`) | provider package | in-progress | `src/huggingface.rs` | `huggingface_provider_generates_text_with_request_and_response_metadata`; `huggingface_responses_maps_system_provider_options_and_structured_output`; `huggingface_responses_converts_images_tool_messages_and_content_parts`; `huggingface_responses_reports_unsupported_provider_references`; `huggingface_responses_maps_warnings_errors_and_stream_deferral`; `huggingface_provider_reports_unsupported_embedding_and_image`; `huggingface_provider_uses_default_base_url_and_function_alias`; `huggingface_provider_implements_provider_trait`; `huggingface_provider_settings_serde_accepts_upstream_base_url` | Initial provider foundation mirrors upstream `createHuggingFace` settings for default/custom base URL, `HUGGINGFACE_API_KEY`, custom headers, Hugging Face user-agent suffix, callable-style `huggingface(...)`, `responses`/`languageModel`, provider-v4 trait integration, non-streaming `/responses` text generation, system/user/assistant message conversion, inline and URL image prompt parts, tool-message warnings, response text/reasoning/source/provider-executed tool content extraction, usage conversion, structured output `text.format`, provider-specific `metadata`/`instructions`/`reasoningEffort` options, unsupported embedding/image messages, and explicit streaming deferral. Full SSE streaming, tool preparation/tool choice, full provider-option validation, MCP edge cases, and live Hugging Face validation remain unported. |
| `packages/hume` (`@ai-sdk/hume`) | provider package | in-progress | `crates/ai-sdk-hume` | `hume_provider_creates_speech_model_with_headers_options_and_body`; `hume_speech_model_defaults_voice_format_and_warns_for_unsupported_inputs`; `hume_speech_model_maps_generation_context_and_api_errors_to_metadata`; `hume_provider_reports_unsupported_model_families_and_trait_speech`; `hume_provider_settings_serde_accepts_upstream_shape`; `hume_default_factory_creates_speech_model` | Initial provider-owned crate mirrors upstream `createHume` settings for `HUME_API_KEY`, `X-Hume-Api-Key`, custom headers, Hume user-agent suffix, default `hume()` speech factory, provider-v4 trait integration, unsupported language/embedding/image lookups, `/v0/tts/file` JSON request shaping, upstream default voice id, `mp3`/`pcm`/`wav` output formats, unsupported output-format and language warnings, provider `context.generationId` and context utterance mapping, binary audio response mapping, response headers, timestamp/model metadata, and Hume error metadata. Workflow serialization hooks, abort handling, live Hume validation, exact thrown-error classes, and broader option schema fixture coverage remain unported. |
| `packages/klingai` (`@ai-sdk/klingai`) | provider package | not-started | none | none | Needs auth, provider, and video model. |
| `packages/lmnt` (`@ai-sdk/lmnt`) | provider package | in-progress | `crates/ai-sdk-lmnt` | `lmnt_provider_creates_speech_model_with_headers_options_and_body`; `lmnt_speech_model_defaults_voice_format_and_warns_for_unsupported_format`; `lmnt_speech_model_maps_error_response_to_metadata`; `lmnt_provider_reports_unsupported_model_families_and_trait_speech`; `lmnt_default_factory_creates_speech_model` | Initial provider-owned crate mirrors upstream `createLMNT` settings for `LMNT_API_KEY`, custom headers, LMNT user-agent suffix, `lmnt()`/`create_lmnt`, `speech`/`speech_model`, provider-v4 trait integration, `/v1/ai/speech/bytes` request shaping, default voice/output format, supported output formats, unsupported format warnings, LMNT provider options, binary audio response mapping, response headers, error metadata, and unsupported language/embedding/image lookups. This is the first concrete package-owned provider crate slice under the 1:1 crate/package rule; the root crate is used only for shared provider traits and utilities. |
| `packages/luma` (`@ai-sdk/luma`) | provider package | in-progress | `crates/ai-sdk-luma` | `luma_provider_creates_image_model_with_headers_body_and_metadata`; `luma_image_model_maps_reference_images_and_warnings`; `luma_image_model_reports_editing_validation_errors_to_metadata`; `luma_image_model_maps_api_and_status_errors_to_metadata`; `luma_provider_reports_unsupported_model_families_and_trait_image`; `luma_provider_settings_serde_accepts_upstream_shape` | Initial provider-owned crate mirrors upstream `createLuma` settings for default/custom base URL, `LUMA_API_KEY`, bearer auth, custom provider/request headers, Luma user-agent suffix, `luma()`/`create_luma`, `image`/`image_model`, provider-v4 trait integration, unsupported language/embedding lookups, async image generation submit to `/dream-machine/v1/generations/image`, polling `/dream-machine/v1/generations/{id}`, generated image download without provider auth headers, response header/timestamp/model metadata, unsupported seed/size warnings, Luma provider option passthrough, poll override extraction, URL-only reference images for `image`/`style`/`character`/`modify_image`, reference weighting/identity mapping, editing validation errors, and API/status error metadata. Workflow serialization hooks, exact thrown-error classes, abort handling, live Luma validation, full zod-equivalent option validation, and broader response schema fixture coverage remain unported. |
| `packages/mistral` (`@ai-sdk/mistral`) | provider package | in-progress | `crates/ai-sdk-mistral`, `src/openai_compatible.rs` | `mistral_provider_creates_chat_model_with_headers_and_base_url`; `mistral_provider_creates_embedding_model_with_usage_and_headers`; `mistral_provider_uses_default_base_url_and_function_alias`; `mistral_provider_reports_unsupported_image_models`; `mistral_provider_settings_serde_accepts_upstream_base_url` | Initial provider-owned crate mirrors upstream `createMistral` settings for default/custom base URL, `MISTRAL_API_KEY`, custom headers, Mistral user-agent suffix, callable-style `mistral(...)`, `language_model`/`chat`, embedding/text-embedding aliases, provider-v4 trait integration, OpenAI-compatible `/chat/completions` and `/embeddings` request routing, usage/header mapping, and unsupported image lookups. Mistral-specific prompt conversion, safe prompt, document image/page limits, structured-output options, strict JSON schema handling, parallel tool-call options, reasoning effort, tool preparation, usage conversion edge cases, SSE fixture coverage, generated ids, and exact error schema mapping remain unported. |
| `packages/moonshotai` (`@ai-sdk/moonshotai`) | provider package | in-progress | `crates/ai-sdk-moonshotai` | `moonshotai_provider_creates_chat_model_with_headers_options_and_usage`; `moonshotai_provider_streams_chat_with_options_and_usage`; `moonshotai_provider_uses_default_base_url_and_function_alias`; `moonshotai_provider_reports_unsupported_model_families`; `moonshotai_provider_implements_provider_trait`; `moonshotai_provider_settings_serde_accepts_upstream_base_url`; `moonshotai_language_model_options_serde_match_upstream_shape`; `moonshotai_usage_conversion_handles_null_and_token_details` | Initial provider-owned crate mirrors upstream `createMoonshotAI` settings for default/custom base URL, `MOONSHOT_API_KEY`, custom headers, MoonshotAI user-agent suffix, callable-style `moonshotai()`, `language_model`/`chat_model`, provider-v4 trait integration, OpenAI-compatible `/chat/completions` request routing and SSE streaming, upstream `thinking.budgetTokens` to `thinking.budget_tokens` and `reasoningHistory` to `reasoning_history` body transformation, top-level `cached_tokens` and reasoning-token usage conversion for generate and stream finishes, and unsupported embedding/image lookups. MoonshotAI-specific message conversion, tool preparation, error schema mapping, workflow serialization hooks, and zod-equivalent provider-option validation remain unported. |
| `packages/perplexity` (`@ai-sdk/perplexity`) | provider package | in-progress | `crates/ai-sdk-perplexity` | `perplexity_provider_creates_language_model_with_headers_and_base_url`; `perplexity_provider_uses_default_base_url_and_function_alias`; `perplexity_provider_reports_unsupported_model_families`; `perplexity_provider_implements_provider_trait`; `perplexity_provider_settings_serde_accepts_upstream_base_url` | Initial provider-owned crate mirrors upstream `createPerplexity` settings for default/custom base URL, `PERPLEXITY_API_KEY`, custom headers, Perplexity user-agent suffix, `perplexity()` factory, `language_model`, provider-v4 trait integration, OpenAI-compatible `/chat/completions` request routing, provider id `perplexity`, and unsupported embedding/image lookups. Perplexity-specific message conversion warnings, citation/source extraction, images/search usage/cost provider metadata, JSON-schema response format, SSE stream parsing, and error schema mapping remain unported. |
| `packages/prodia` (`@ai-sdk/prodia`) | provider package | not-started | none | none | Needs image, language, video, and provider settings. |
| `packages/replicate` (`@ai-sdk/replicate`) | provider package | not-started | none | none | Needs image, video, and provider settings. |
| `packages/revai` (`@ai-sdk/revai`) | provider package | in-progress | `crates/ai-sdk-revai` | `revai_transcription_model_transcribes_audio_with_headers_options_and_response`; `revai_transcription_duration_falls_back_for_open_segment`; `revai_transcription_model_maps_api_and_status_errors_to_metadata`; `revai_provider_reports_unsupported_model_families_and_trait_transcription`; `revai_provider_settings_serde_accepts_upstream_shape` | Initial provider-owned crate mirrors upstream `createRevai` settings for `REVAI_API_KEY`, bearer auth, Rev.ai user-agent suffix, custom provider/request headers, `revai(...)`/`create_revai`, `transcription`/`transcription_model`, provider-v4 trait integration, unsupported language/embedding/image lookups with upstream messages, multipart `media` + `config` submit to `/speechtotext/v1/jobs`, polling `/speechtotext/v1/jobs/{id}`, transcript fetch `/speechtotext/v1/jobs/{id}/transcript`, transcript text/segment/duration/language/response metadata mapping, raw transcript body/headers, provider option passthrough, status/API error metadata, and dependency-free multipart default transport. Workflow serialization hooks, exact thrown-error classes, abort handling, live RevAI validation, full zod-equivalent option validation/defaults, and exact browser `File` filename/media extension parity remain unported. |
| `packages/togetherai` (`@ai-sdk/togetherai`) | provider package | in-progress | `src/togetherai.rs`, `src/openai_compatible.rs` | `togetherai_provider_creates_chat_model_with_headers_base_url_and_body`; `togetherai_provider_creates_completion_model`; `togetherai_provider_creates_embedding_model_aliases`; `togetherai_provider_creates_image_model_and_generates_images`; `togetherai_image_model_maps_api_error_to_metadata`; `togetherai_image_model_reports_unsupported_mask_without_request`; `togetherai_provider_creates_reranking_model`; `togetherai_reranking_model_maps_api_error_to_metadata`; `togetherai_provider_uses_default_base_url_and_function_alias`; `togetherai_api_key_prefers_explicit_then_new_env_then_deprecated_env`; `togetherai_provider_implements_provider_trait`; `togetherai_provider_settings_serde_accepts_upstream_base_url` | Provider foundation mirrors upstream `createTogetherAI` settings for default/custom base URL, `TOGETHER_API_KEY`, deprecated `TOGETHER_AI_API_KEY` fallback precedence, custom headers, TogetherAI user-agent suffix, callable-style `togetherai(...)`, provider-v4 trait integration, OpenAI-compatible `/chat/completions`, `/completions`, and `/embeddings` models with provider ids `togetherai.chat`, `togetherai.completion`, and `togetherai.embedding`. Custom image generation covers `/images/generations`, base64 and URL input image shaping, `size` width/height, single-image warning, aspect-ratio warning, provider options, max images per call, response headers, and API error metadata. Reranking covers `/rerank`, text/object documents, `top_n`, `rank_fields`, `return_documents: false`, ranking mapping, raw response body, response headers, and API error metadata. Remaining package parity is broader OpenAI-compatible option/error edge coverage rather than missing custom TogetherAI model families. |
| `packages/vercel` (`@ai-sdk/vercel`) | provider package | in-progress | `src/vercel.rs` | `vercel_provider_creates_openai_compatible_chat_model`; `vercel_provider_uses_default_base_url_and_function_alias`; `vercel_provider_reports_unsupported_model_families`; `vercel_provider_implements_provider_trait` | Upstream `@ai-sdk/vercel` v0 provider settings, default/custom base URL, `VERCEL_API_KEY` lookup, custom headers, `vercel.chat` OpenAI-compatible chat model creation, Vercel user-agent suffix, callable-style `vercel(...)` Rust function, provider-v4 trait integration, and unsupported embedding/image model errors are represented. Broader v0 API live validation and any future upstream package expansion remain unported. |
| `packages/voyage` (`@ai-sdk/voyage`) | provider package | in-progress | `src/voyage.rs` | `voyage_provider_creates_embedding_model_with_options_headers_and_sorted_results`; `voyage_embedding_model_chunks_at_128_and_maps_api_error_to_metadata`; `voyage_provider_creates_reranking_model_with_object_warning_and_options`; `voyage_reranking_model_maps_api_error_to_metadata`; `voyage_provider_reports_unsupported_language_and_image_models`; `voyage_provider_uses_default_base_url_and_factory_alias`; `voyage_provider_implements_provider_traits`; `voyage_provider_settings_serde_accepts_upstream_base_url`; `voyage_api_key_prefers_explicit_then_env`; `voyage_embedding_direct_call_reports_too_many_values` | Initial provider foundation mirrors upstream `createVoyage` settings for default/custom base URL, `VOYAGE_API_KEY`, custom headers, Voyage user-agent suffix, `voyage()`/`create_voyage`, embedding/text-embedding aliases, reranking aliases, provider-v4 trait integration, `/embeddings` request options (`input_type`, `truncation`, `output_dimension`, `output_dtype`), 128-value chunking, response index sorting, `/rerank` request options (`top_k`, `return_documents`, `truncation`), object-document string conversion compatibility warnings, response headers/raw bodies, API error metadata from `detail`, and unsupported language/image lookups. Full zod-equivalent provider-option validation and live Voyage validation remain unported. |
| `packages/mcp` (`@ai-sdk/mcp`) | protocol/client package | in-progress | `crates/ai-sdk-mcp` | `protocol_constants_match_upstream_mcp_package`; `json_rpc_message_shapes_match_mcp_transport_boundary`; `list_tools_result_is_serializable_cache_data`; `mcp_client_initializes_and_sends_initialized_notification`; `mcp_client_lists_calls_reads_resources_and_prompts`; `mcp_client_reports_capability_protocol_and_json_rpc_errors`; `mcp_client_handles_elicitation_request_messages`; `mcp_client_reports_elicitation_request_errors_to_server`; `mcp_client_invokes_uncaught_error_callback_for_transport_start_errors`; `mcp_client_invokes_uncaught_error_callback_for_elicitation_handler_errors`; `mcp_client_builds_executable_dynamic_tools_from_definitions`; `mcp_client_builds_schema_typed_tools_from_structured_content`; `mcp_client_schema_typed_tools_parse_text_content_fallback`; `mcp_client_schema_typed_tools_report_output_validation_errors`; `mcp_client_runs_authenticated_http_tools_with_output_schema_and_provider_metadata`; `mcp_http_transport_posts_json_and_cleans_up_session`; `mcp_http_transport_parses_sse_message_responses`; `mcp_http_transport_reopens_inbound_sse_after_accepted_post`; `mcp_http_transport_sends_last_event_id_when_resuming_inbound_sse`; `mcp_http_transport_retries_inbound_sse_open_failures`; `mcp_http_transport_retries_resumed_inbound_sse_after_accepted_post`; `mcp_http_transport_computes_inbound_sse_reconnect_backoff`; `mcp_http_transport_reports_max_inbound_sse_reconnect_attempts`; `mcp_http_transport_reports_invalid_inbound_sse_messages`; `mcp_http_transport_reports_http_errors_with_sse_hint`; `mcp_sse_transport_connects_to_endpoint_and_posts_messages`; `mcp_sse_transport_parses_post_sse_message_responses`; `mcp_sse_transport_rejects_endpoint_origin_mismatch`; `mcp_sse_transport_reports_http_errors_with_http_hint`; `mcp_sse_transport_reports_post_errors`; `stdio_environment_copies_custom_env_and_inherits_safe_defaults`; `stdio_message_framing_serializes_deserializes_and_buffers_lines`; `stdio_transport_errors_when_not_connected`; `stdio_transport_writes_message_and_reads_response_line`; `mcp_to_model_output_converts_text_images_and_unknown_content`; `mcp_to_model_output_falls_back_to_json_without_content_array`; `mcp_app_client_capabilities_match_upstream_extension_shape`; `mcp_app_tool_meta_reads_ui_and_legacy_resource_uris`; `mcp_app_tool_meta_rejects_invalid_resource_uri`; `split_mcp_app_tools_respects_model_and_app_visibility`; `mcp_app_resource_from_read_result_extracts_text_html_and_meta`; `mcp_app_resource_from_read_result_decodes_blob_html`; `read_mcp_app_resource_rejects_non_ui_uri`; `mcp_provider_metadata_includes_title_and_app_metadata`; `resource_url_from_server_url_removes_fragment_and_preserves_url_parts`; `resource_url_strip_slash_removes_only_pathless_trailing_slash`; `check_resource_allowed_matches_origin_and_path_boundaries`; `select_resource_url_uses_protected_metadata_when_allowed`; `select_resource_url_rejects_mismatched_protected_metadata`; `extract_resource_metadata_url_reads_bearer_www_authenticate_parameter`; `build_discovery_urls_matches_upstream_priority_order`; `oauth_metadata_safe_url_deserialization_rejects_dangerous_schemes`; `discover_oauth_protected_resource_metadata_uses_path_query_and_protocol_header`; `discover_oauth_protected_resource_metadata_falls_back_to_root_on_path_4xx`; `discover_oauth_protected_resource_metadata_does_not_fallback_for_explicit_metadata_url`; `discover_authorization_server_metadata_tries_urls_in_order`; `discover_authorization_server_metadata_validates_oidc_s256_support`; `discover_authorization_server_metadata_returns_none_when_all_endpoints_are_4xx`; `oauth_pkce_challenge_derives_s256_challenge_from_verifier`; `oauth_pkce_challenge_generates_random_url_safe_verifier`; `start_authorization_builds_pkce_resource_scope_state_and_prompt_params`; `start_authorization_can_generate_pkce_material`; `start_authorization_uses_metadata_endpoint_and_validates_capabilities`; `start_authorization_strips_pathless_resource_trailing_slash`; `parse_oauth_error_response_reads_standard_error_body`; `exchange_authorization_posts_code_verifier_client_secret_and_resource`; `exchange_authorization_uses_basic_auth_when_metadata_prefers_it`; `exchange_authorization_allows_custom_client_authentication_hook`; `exchange_authorization_validates_grant_type_and_token_response`; `refresh_authorization_posts_refresh_token_and_preserves_missing_replacement`; `refresh_authorization_reports_oauth_error_response`; `refresh_authorization_allows_custom_client_authentication_hook`; `register_client_posts_metadata_and_parses_full_information`; `register_client_uses_metadata_endpoint_and_requires_registration_support`; `auth_registers_client_and_redirects_when_tokens_are_missing`; `auth_exchanges_callback_code_and_saves_tokens_with_resource`; `auth_rejects_mismatched_callback_state_before_token_exchange`; `auth_invalidates_rejected_refresh_token_and_retries_to_redirect`; `auth_invalidates_rejected_client_credentials_and_reregisters`; `cargo run -p ai-sdk-mcp --example local_mcp_client`; `cargo run -p ai-sdk-mcp --example http_auth_typed_tools`; `cargo run -p ai-sdk-mcp --example stdio_typed_tools`; `cargo run -p ai-sdk-mcp --example sse_typed_tools`; `cargo run -p ai-sdk-mcp --example hosted_oauth_http`; `vercel_ai_gateway_openai_compatible_runs_generate_text_with_mcp_tools`; ignored `live_vercel_ai_gateway_openai_compatible_generate_text_mcp_tool_loop` | Package-owned crate mirrors the portable upstream `@ai-sdk/mcp` protocol constants, JSON-RPC message envelope shapes, MCP implementation/capability/tool/resource/prompt/elicitation result data structures, serializable `listTools` cache data, deterministic transport trait and mock transport, client initialize/request/notification/close lifecycle, protocol negotiation, capability gating, JSON-RPC error metadata, `tools/list`, `tools/call`, resource and prompt methods, client-side `elicitation/create` request handling with success, missing-handler, invalid-request, handler-error JSON-RPC replies, and upstream-shaped `onUncaughtError` callback behavior for transport startup and request-handler errors, dynamic AI SDK tool creation from MCP definitions with MCP/App metadata and execution, schema-filtered tool creation, authenticated loopback Streamable HTTP tool execution with bearer headers, session cleanup and provider metadata assertions, output-schema retention, `structuredContent` extraction, text JSON fallback parsing, validation errors, and `isError` bypass behavior, initial Streamable HTTP transport with real loopback POST JSON, protocol/session/custom headers, best-effort inbound SSE GET on start and after `202 Accepted`, upstream-shaped inbound SSE reconnect delay/backoff/max-retry behavior, `Last-Event-ID` resumption headers across retries, invalid inbound SSE message errors, JSON and SSE message response parsing, 404 SSE hint errors, and DELETE session cleanup, standalone SSE transport with endpoint event parsing, same-origin enforcement, bounded message parsing, POST dispatch, bounded POST message response parsing, custom headers, and HTTP/POST error reporting, stdio inherited environment filtering/overlay behavior, newline-delimited JSON-RPC stdio serialization/deserialization and read buffering, child-process stdio transport start/write/read/close behavior, MCP Apps capability metadata, `_meta.ui`/legacy resource URI parsing, model/app visibility splitting, `ui://` app resource extraction from text or base64 blob resources, MCP tool-result conversion into model-facing text/file/content or JSON fallback output, OAuth resource URL helpers and protected-resource selection, upstream-shaped OAuth metadata/client/token/error structs with safe URL validation, `WWW-Authenticate` protected-resource metadata extraction, protected-resource metadata discovery over loopback HTTP with path-aware root fallback and `MCP-Protocol-Version`, authorization-server OAuth/OIDC discovery ordering with S256 PKCE validation, generated or caller-supplied S256 PKCE material, authorization URL construction with scope, state, offline-access consent prompt, RFC 8707 resource parameter handling, form-encoded authorization-code exchange, refresh-token exchange with refresh-token preservation, client auth method selection for public, post-body secret, basic auth requests, and custom client-authentication hooks, standard OAuth error body parsing, dynamic client registration over JSON, high-level OAuth provider orchestration for protected-resource discovery, dynamic registration, callback code exchange, stored-state validation, refresh-token reuse, redirect PKCE persistence, custom resource validation override, credential invalidation retry behavior, and a deterministic local MCP client example that mirrors the upstream tool-definitions flow by listing tools, converting definitions into AI SDK tools, calling tools, reading resources/templates, listing/getting prompts, handling elicitation, printing server instructions, and closing the client, plus authenticated Streamable HTTP, stdio, and SSE typed-tool examples that start local servers, validate `structuredContent`, and print MCP provider metadata, plus a hosted OAuth HTTP example that starts a local protected-resource and authorization server, performs dynamic client registration, PKCE redirect/callback exchange, token exchange, configured auth-provider transport creation, and protected tool execution, plus a Vercel AI Gateway OpenAI-compatible `generate_text` integration that consumes MCP tool definitions, executes the selected MCP tool, and has an ignored live Gateway proof for the same tool-loop path. Remaining work: protected live MCP service auth validation if suitable credentials are available. |
| `packages/otel` (`@ai-sdk/otel`) | telemetry package | in-progress | `crates/ai-sdk-otel` | `select_attributes_matches_telemetry_recording_flags`; `assemble_operation_name_includes_function_id_when_present`; `maps_provider_and_operation_names_to_genai_semconv_values`; `formats_system_and_input_messages`; `formats_output_messages_and_finish_reasons`; `base_and_supplemental_attributes_match_upstream_prefixes`; `stringify_for_telemetry_converts_file_data_to_strings`; `record_span_executes_function_records_attributes_and_ends_by_default`; `record_span_can_leave_successful_span_open`; `record_span_records_exception_status_and_ends_on_error`; `record_error_on_span_sets_status_only_for_non_error_values`; `mock_and_noop_tracers_match_upstream_test_shapes`; `open_telemetry_records_generate_text_root_step_and_chat_spans`; `open_telemetry_records_object_operation_and_step_spans`; `open_telemetry_enrichment_keeps_official_attribute_precedence`; `open_telemetry_records_tool_span_and_wraps_execute_tool`; `open_telemetry_records_error_on_active_spans_and_cleans_state`; `open_telemetry_records_embedding_operation_and_inner_span`; `open_telemetry_records_rerank_operation_and_inner_span`; `legacy_open_telemetry_records_generate_text_step_tool_and_root_spans`; `legacy_open_telemetry_records_object_embedding_and_rerank_spans`; `otlp_http_json_payload_uses_collector_shape`; `local_otlp_http_receiver_captures_exported_span_payload`; `real_opentelemetry_http_exporter_sends_json_to_local_receiver`; `open_telemetry_integration_exports_dispatcher_spans_to_local_otlp_receiver`; `legacy_open_telemetry_integration_exports_dispatcher_spans_to_local_otlp_receiver`; ignored `live_vercel_ai_gateway_openai_compatible_generate_text_with_otel`; ignored `live_vercel_ai_gateway_openai_compatible_stream_text_with_otel`; ignored `live_vercel_ai_gateway_openai_compatible_generate_object_with_otel`; ignored `live_vercel_ai_gateway_openai_compatible_stream_object_with_otel`; ignored `live_vercel_ai_gateway_openai_responses_generate_text_with_otel`; ignored `live_vercel_ai_gateway_openai_responses_stream_text_with_otel`; `scripts/check-otel-loopback.sh`; `cargo run -p ai-sdk-otel --example local_otlp_receiver` | Initial package-owned crate covers portable helper behavior from upstream `@ai-sdk/otel`: telemetry/input/output attribute gating, operation/resource attribute naming, provider and operation GenAI semantic-convention mapping, system/input/output/object message formatting, file/base64/URL prompt telemetry stringification, base model-call attributes, supplemental attribute selection, runtime-context/header/detailed-usage attribute helpers, finish-reason mapping, and dependency-free Rust analogues for `recordSpan`, `recordErrorOnSpan`, `MockTracer`, noop tracer behavior, `OpenTelemetry` operation/step/language-model/tool/object/embedding/reranking lifecycle span recording, `LegacyOpenTelemetry` legacy `ai.*` text/tool/object/embedding/reranking span recording, enrichment precedence, active-span error cleanup, OTLP/HTTP JSON export payload construction, a loopback local OTLP receiver that captures actual HTTP wire payloads, a `real-opentelemetry` feature that configures the real Rust `opentelemetry` SDK OTLP/HTTP JSON exporter against that receiver, root dispatcher adapters that register `OpenTelemetry` and `LegacyOpenTelemetry` as normal telemetry integrations and export dispatcher-produced spans to the receiver, a required `scripts/check-otel-loopback.sh` proof command that also runs live Gateway OpenAI-compatible generate, stream, object, and stream-object telemetry checks plus Gateway OpenAI Responses generate and stream telemetry checks when live mode is enabled, and a runnable daemon-style local receiver example for manual collector-style validation. Remaining work: provider live tests that enable telemetry and assert emitted OTLP data through the local receiver or a collector. |
| `packages/workflow` (`@ai-sdk/workflow`) | agent/workflow package | in-progress | `crates/ai-sdk-workflow` | `serialize_tool_set_serializes_function_tools_with_description_and_input_schema`; `serialize_tool_set_preserves_provider_tool_identity_and_args`; `resolve_serializable_tools_reconstructs_function_tools`; `resolve_serializable_tools_reconstructs_provider_tools`; `resolve_serializable_tools_reports_missing_provider_tool_id`; `to_ui_message_chunk_maps_text_reasoning_and_tool_call_parts`; `to_ui_message_chunk_maps_files_sources_results_approval_and_errors`; `model_call_stream_to_ui_chunks_adds_lifecycle_chunks_and_drops_internal_parts`; `workflow_chat_transport_uses_default_options_and_builds_send_request`; `workflow_chat_transport_sends_messages_and_reports_chat_end`; `workflow_chat_transport_requires_workflow_run_id_for_interrupted_send`; `workflow_chat_transport_reconnects_after_interrupted_send_using_run_id_and_chunk_index`; `workflow_chat_transport_reconnect_uses_positive_initial_start_index_for_retries`; `workflow_chat_transport_reconnect_resolves_negative_start_index_from_tail_header`; `workflow_chat_transport_reconnect_falls_back_to_zero_for_invalid_negative_tail_header`; `workflow_chat_transport_reconnect_formats_consecutive_errors`; `workflow_chat_transport_reports_http_errors`; `stream_text_iterator_maps_provider_metadata_to_provider_options_for_continuation`; `stream_text_iterator_upstream_should_preserve_provider_metadata_for_multiple_parallel_tool_calls`; `stream_text_iterator_upstream_should_handle_mixed_tool_calls_with_and_without_provider_metadata`; `stream_text_iterator_upstream_should_not_add_provider_options_when_provider_metadata_is_undefined`; `stream_text_iterator_upstream_should_strip_openai_item_id_from_provider_metadata_to_avoid_reasoning_item_errors`; `stream_text_iterator_upstream_should_preserve_other_openai_metadata_while_stripping_item_id`; `stream_text_iterator_upstream_should_preserve_gemini_metadata_while_stripping_openai_item_id_in_mixed_provider_metadata`; `stream_text_iterator_omits_provider_options_without_metadata`; `stream_text_iterator_strips_openai_item_id_and_preserves_other_metadata`; `stream_text_iterator_passes_contexts_to_executor_and_yields_them`; `stream_text_iterator_upstream_should_allow_prepare_step_to_modify_messages`; `stream_text_iterator_upstream_should_apply_prepare_step_system_after_messages_override`; `stream_text_iterator_upstream_should_allow_prepare_step_to_change_model_dynamically`; `stream_text_iterator_upstream_should_allow_prepare_step_to_set_active_tools_and_tool_choice`; `stream_text_iterator_upstream_should_update_runtime_and_tools_context_from_prepare_step`; `do_stream_step_from_parts_collects_provider_executed_results_and_valid_step_content`; `workflow_agent_upstream_should_expose_id_when_provided_in_constructor`; `workflow_agent_upstream_should_have_undefined_id_when_not_provided`; `workflow_agent_upstream_should_convert_tool_execution_error_to_error_text_result`; `workflow_agent_upstream_should_successfully_execute_tools_that_return_normally`; `workflow_agent_upstream_should_skip_local_execution_for_provider_executed_tools`; `workflow_agent_upstream_should_handle_provider_executed_tool_errors_with_is_error_flag`; `workflow_agent_upstream_should_return_empty_result_when_provider_executed_tool_result_is_missing`; `workflow_agent_upstream_should_stop_the_loop_for_client_side_tools_without_execute`; `workflow_agent_upstream_should_call_on_finish_when_stopping_for_client_side_tools`; `workflow_agent_compat_should_call_on_finish_from_constructor`; `workflow_agent_compat_should_call_on_finish_from_stream_method`; `workflow_agent_compat_should_call_both_constructor_and_method_on_finish_in_correct_order`; `workflow_agent_compat_should_pass_finish_event_information`; `workflow_agent_compat_should_call_experimental_on_start_from_constructor`; `workflow_agent_compat_should_call_experimental_on_start_from_stream_method`; `workflow_agent_compat_should_call_both_constructor_and_method_experimental_on_start_in_correct_order`; `workflow_agent_compat_should_pass_experimental_on_start_event_information`; `workflow_agent_compat_should_call_experimental_on_step_start_from_constructor`; `workflow_agent_compat_should_call_experimental_on_step_start_from_stream_method`; `workflow_agent_compat_should_call_both_constructor_and_method_experimental_on_step_start_in_correct_order`; `workflow_agent_compat_should_pass_experimental_on_step_start_event_information`; `workflow_agent_compat_should_call_on_step_finish_from_constructor`; `workflow_agent_compat_should_call_on_step_finish_from_stream_method`; `workflow_agent_compat_should_call_both_constructor_and_method_on_step_finish_in_correct_order`; `workflow_agent_compat_should_pass_step_result_to_on_step_finish_callback`; `workflow_agent_compat_should_call_on_tool_execution_start_from_constructor`; `workflow_agent_compat_should_call_on_tool_execution_start_from_stream_method`; `workflow_agent_compat_should_call_both_constructor_and_method_on_tool_execution_start_in_correct_order`; `workflow_agent_compat_should_pass_tool_execution_start_event_information`; `workflow_agent_compat_should_call_on_tool_execution_end_from_constructor`; `workflow_agent_compat_should_call_on_tool_execution_end_from_stream_method`; `workflow_agent_compat_should_call_both_constructor_and_method_on_tool_execution_end_in_correct_order`; `workflow_agent_compat_should_pass_tool_execution_end_event_information_on_success`; `workflow_agent_upstream_should_have_empty_tool_calls_when_all_tools_complete_normally`; `workflow_agent_upstream_should_pass_generation_settings_from_constructor_to_stream_text_iterator`; `workflow_agent_upstream_should_allow_stream_options_to_override_constructor_generation_settings`; `workflow_agent_upstream_should_pass_tool_choice_from_constructor_to_stream_text_iterator`; `workflow_agent_upstream_should_allow_stream_options_to_override_constructor_tool_choice`; `workflow_agent_upstream_should_filter_tools_when_active_tools_is_specified`; `workflow_agent_upstream_should_pass_conversation_messages_to_tool_execute_function`; `workflow_agent_upstream_should_pass_per_tool_tools_context_entry_as_execute_context`; `workflow_agent_upstream_should_validate_per_tool_context_against_context_schema`; `workflow_agent_upstream_should_pass_prepare_step_callback_to_stream_text_iterator`; `workflow_agent_upstream_prepare_step_updates_runtime_context_for_agent_loop` | Initial package-owned crate covers portable `serializable-schema`, `to-ui-message-chunk`, `workflow-chat-transport`, deterministic `stream-text-iterator` behavior, and the first deterministic `WorkflowAgent` loop behavior: runtime tools serialize to plain descriptions/input JSON Schemas, provider tool identity/args/`isProviderExecuted` survive workflow step boundaries, serializable definitions reconstruct Rust tool descriptors, model-call stream parts convert into UI-message text/reasoning/file/source/tool/approval/error chunks with workflow lifecycle wrappers, chat transport builds send/reconnect requests and handles interrupted stream resume planning, the stream-text iterator collects model-call stream steps, preserves runtime/tool contexts, applies prepare-step message/system/model/generation/active-tool/tool-choice/runtime/tool-context overrides, maps single, parallel, mixed, and absent tool-call provider metadata into continuation prompt `providerOptions`, strips OpenAI `itemId` while preserving other OpenAI and non-OpenAI metadata, accepts tool-result continuation messages, captures provider-executed tool results, and `WorkflowAgent` now exposes optional ids, executes local tools, converts tool execution failures to `error-text`, skips local execution for provider-executed tool calls, surfaces provider-executed results/errors, returns an empty text result for missing provider-executed results, stops for client-side tools without executors, calls finish callbacks for client-side stops, constructor-then-stream finish callbacks with event payloads, start and step-start callbacks with constructor-then-stream ordering and event payloads, step-finish callbacks with constructor-then-stream ordering and step payloads, and tool-execution start/end callbacks with constructor-then-stream ordering and event payloads, clears final tool calls and results after completed tool rounds, passes accumulated messages and per-tool context to tool callbacks, forwards prepare-step callbacks through the agent facade, updates runtime context from prepare-step callbacks, and validates Rust-side context schema validators. Real model execution inside the iterator, real HTTP/SSE transport adapters, integration-style workflow execution, and Ajv-equivalent runtime validation for arbitrary JSON Schema remain unported. |
| `packages/test-server` (`@ai-sdk/test-server`) | testing support package | verified | `crates/ai-sdk-test-server` | `create_test_server_exposes_urls_and_empty_calls`; `create_test_server_supports_response_mutations_and_reset`; `create_test_server_supports_response_types`; `create_test_server_tracks_request_inspection`; `create_test_server_parses_multipart_request_body`; `create_test_server_selects_sequence_and_dynamic_responses_by_call_number`; `response_controller_records_writes_errors_and_close`; `loopback_test_server_serves_http_and_records_requests`; `loopback_test_server_serves_streams_missing_routes_and_reset`; `cargo test -p ai-sdk-test-server` | Package-owned crate covers the portable upstream `createTestServer` surface: mutable URL handlers, static/missing/sequence/dynamic responses, JSON/stream/binary/empty/error/controlled-stream response rendering, call capture, request header/body/multipart body/URL/method/credentials inspection, reset behavior, `convertArrayToReadableStream`-style chunk collection, and a dependency-light loopback HTTP server for real request/response validation of path-keyed routes. Upstream's MSW interception and `with-vitest` lifecycle hook wrapper are JavaScript-runtime bindings, so they are intentionally documented as non-portable rather than remaining Rust parity debt. |
| `packages/devtools` (`@ai-sdk/devtools`) | JavaScript devtools package | js-only-documented | none | This row | Upstream exposes JavaScript middleware/telemetry integration for AI SDK DevTools. Portable Rust tracing/telemetry behavior is tracked under telemetry rows; browser devtools integration is intentionally not ported. |
| `packages/codemod` (`@ai-sdk/codemod`) | JavaScript migration tooling | js-only-documented | none | This row | Upstream codemods transform JavaScript/TypeScript source between AI SDK versions. No Rust SDK runtime equivalent is required. |
| `packages/angular` (`@ai-sdk/angular`) | JavaScript framework adapter | js-only-documented | none | This row | Angular components/services are JavaScript framework bindings. Portable chat/object/transport semantics are tracked in API rows. |
| `packages/react` (`@ai-sdk/react`) | JavaScript framework adapter | js-only-documented | none | This row | React hooks/components are JavaScript framework bindings. Portable chat/object/transport semantics are tracked in API rows. |
| `packages/rsc` (`@ai-sdk/rsc`) | JavaScript framework adapter | js-only-documented | none | This row | React Server Components and streamable UI surfaces depend on React/Next.js runtime semantics. Portable streaming contracts are tracked separately. |
| `packages/svelte` (`@ai-sdk/svelte`) | JavaScript framework adapter | js-only-documented | none | This row | Svelte stores/components are JavaScript framework bindings. Portable chat/object/transport semantics are tracked in API rows. |
| `packages/vue` (`@ai-sdk/vue`) | JavaScript framework adapter | js-only-documented | none | This row | Vue composition functions/components are JavaScript framework bindings. Portable chat/object/transport semantics are tracked in API rows. |
| `packages/langchain` (`@ai-sdk/langchain`) | JavaScript library adapter | js-only-documented | none | This row | Upstream adapts LangChain JS callbacks, streams, and transports. A Rust-native LangChain adapter would target a different ecosystem and is intentionally outside the portable SDK runtime. |
| `packages/llamaindex` (`@ai-sdk/llamaindex`) | JavaScript library adapter | js-only-documented | none | This row | Upstream adapts LlamaIndex TS message/tool shapes. A Rust-native adapter would target different crates and is intentionally outside this port. |
| `packages/valibot` (`@ai-sdk/valibot`) | JavaScript schema adapter | js-only-documented | none | This row | Valibot is a JavaScript validation library. Rust schema validation is tracked through `Schema`, `JsonSchema`, and serde-style boundaries. |

## High-Level API Inventory

| Upstream API or feature | Status | Rust path | Evidence | Notes |
| --- | --- | --- | --- | --- |
| Provider-v4 language model contract | verified | `src/language_model.rs` | `language_model_trait_exposes_upstream_v4_identity_capabilities_and_generate_boundary`; stream part serialization tests | Exact JavaScript stream primitive is replaced with Rust associated `Stream`; high-level stream consumers remain unported. |
| Provider-v4 embedding model contract | verified | `src/embedding_model.rs` | `embedding_model_trait_exposes_upstream_v4_identity_capabilities_and_embed_boundary` | Includes max-per-call and parallel-call capability futures. |
| Provider-v4 image model contract | verified | `src/image_model.rs` | Image model call/result serialization tests | Includes image generation result and metadata contracts. |
| Provider-v4 speech model contract | verified | `src/speech_model.rs` | `speech_model_trait_exposes_upstream_v4_identity_and_generate_boundary` | Non-streaming speech generation boundary only. |
| Provider-v4 transcription model contract | verified | `src/transcription_model.rs` | `transcription_model_trait_exposes_upstream_v4_identity_and_generate_boundary` | Non-streaming transcription boundary only. |
| Provider-v4 reranking model contract | verified | `src/reranking_model.rs` | `reranking_model_trait_exposes_upstream_v4_identity_and_rerank_boundary` | Covers text/object document inputs. |
| Provider-v4 video model contract | verified | `src/video_model.rs` | `video_model_trait_exposes_upstream_v4_identity_capability_and_generate_boundary` | Includes max-videos-per-call capability. |
| Provider-v4 files contract | verified | `src/files.rs`, `src/upload_file.rs` | `files_trait_exposes_upstream_v4_identity_and_upload_boundary`; upload tests | Provider implementations remain unported. |
| Provider-v4 skills contract | verified | `src/skills.rs`, `src/upload_skill.rs` | `skills_trait_exposes_upstream_v4_identity_and_upload_boundary`; upload tests | Provider implementations remain unported. |
| Provider metadata, options, headers, warnings, provider references | verified | `src/provider.rs`, `src/headers.rs`, `src/warning.rs`, `src/file_data.rs` | Serialization and error tests in each module | Includes Rust `ProviderReference` wrapper for upstream provider references. |
| JSON values and schemas | verified | `crates/ai-sdk-provider/src/json.rs`, `crates/ai-sdk-provider-utils`, root facade shims in `src/json.rs` and `src/provider_utils.rs` | JSON/schema helper tests | Exact JavaScript validator library adapters remain js-only or unported. |
| Error types and messages | in-progress | `crates/ai-sdk-provider/src/provider.rs`, `crates/ai-sdk-provider-utils`, `crates/ai-sdk-gateway/src/gateway_error.rs`, `src/generate_text.rs`, model modules | Existing error serialization/message tests; `gateway_error::*` tests | Many upstream errors exist, including Gateway-specific error classifications. Other provider-specific error types remain unported. |
| Retry and exponential backoff utility | verified | `src/retry.rs` | `retry_with_exponential_backoff_*`; `retry_delay_*`; `retry_error_*`; `retry_executor_*` | Mirrors every portable upstream `retry-with-exponential-backoff.test.ts` case one-to-one for retry-after-ms, retry-after seconds and HTTP dates, unreasonable/invalid/negative header fallbacks, Anthropic/OpenAI 429 response headers, multiple retries, retry-after-ms precedence, Gateway internal-server and rate-limit retry, Gateway authentication no-retry, and retry headers carried by an API-call cause. Rust uses injected sleep recording instead of JavaScript fake timers, which proves the same delay selection without depending on a JS event loop. |
| Retry preparation utility | verified | `src/util.rs` | `prepare_retries_should_set_default_values_correctly_when_no_input_is_provided` | Mirrors the portable upstream `prepare-retries.test.ts` case one-to-one: absent `maxRetries` resolves to the upstream default of 2 and prepares retry executor options with the same value. Rust accepts a typed `usize` for explicit retry counts, so JavaScript negative and fractional validation is enforced at the type boundary. |
| Request timeout helpers | verified | `src/prompt.rs` | `get_tool_timeout_ms_should_return_undefined_when_timeout_is_undefined`; `get_tool_timeout_ms_should_return_undefined_when_timeout_is_a_number`; `get_tool_timeout_ms_should_return_undefined_when_tool_ms_is_not_set`; `get_tool_timeout_ms_should_return_tool_ms_when_set`; `get_tool_timeout_ms_should_return_tool_ms_alongside_other_timeout_values`; `get_total_timeout_ms_should_return_undefined_when_timeout_is_undefined`; `get_total_timeout_ms_should_return_the_number_directly_when_timeout_is_a_number`; `get_total_timeout_ms_should_return_total_ms_from_an_object`; `get_total_timeout_ms_should_return_undefined_when_total_ms_is_not_set`; `get_step_timeout_ms_should_return_undefined_when_timeout_is_undefined`; `get_step_timeout_ms_should_return_undefined_when_timeout_is_a_number`; `get_step_timeout_ms_should_return_step_ms_from_an_object`; `get_chunk_timeout_ms_should_return_undefined_when_timeout_is_undefined`; `get_chunk_timeout_ms_should_return_undefined_when_timeout_is_a_number`; `get_chunk_timeout_ms_should_return_chunk_ms_from_an_object`; additive `get_tool_timeout_ms_should_prefer_tool_specific_timeout` | Mirrors every portable upstream `prepare-language-model-call-options.test.ts` `request-options` timeout helper case one-to-one: undefined timeouts, numeric total timeout passthrough, missing detailed fields, and detailed `totalMs`/`stepMs`/`chunkMs`/`toolMs` extraction. Rust also keeps an additive per-tool override test for the typed `{toolName}Ms` map. |
| Language model call option preparation | verified | `src/prompt.rs` | `prepare_language_model_call_options_should_not_throw_an_error_for_valid_settings`; `prepare_language_model_call_options_should_allow_undefined_values_for_optional_settings`; `prepare_language_model_call_options_should_reject_non_integer_max_output_tokens_at_type_boundary`; `prepare_language_model_call_options_should_throw_invalid_argument_error_if_max_output_tokens_is_less_than_1`; `prepare_language_model_call_options_should_reject_temperature_if_temperature_is_not_a_number_at_type_boundary`; `prepare_language_model_call_options_should_reject_top_p_if_top_p_is_not_a_number_at_type_boundary`; `prepare_language_model_call_options_should_reject_top_k_if_top_k_is_not_a_number_at_type_boundary`; `prepare_language_model_call_options_should_reject_presence_penalty_if_presence_penalty_is_not_a_number_at_type_boundary`; `prepare_language_model_call_options_should_reject_frequency_penalty_if_frequency_penalty_is_not_a_number_at_type_boundary`; `prepare_language_model_call_options_should_reject_non_integer_seed_at_type_boundary`; `prepare_language_model_call_options_should_pass_through_valid_reasoning_values`; `prepare_language_model_call_options_should_pass_through_provider_default`; `prepare_language_model_call_options_should_pass_through_undefined`; `prepare_language_model_call_options_should_return_a_new_object_with_limited_values` | Mirrors every upstream `prepare-language-model-call-options.test.ts` `prepareLanguageModelCallOptions` case with typed Rust boundaries. `LanguageModelCallSettings` and `prepare_language_model_call_options` pass through valid/optional values, preserve reasoning values including `provider-default` and absence, return only the limited generation settings, and retain the portable runtime `maxOutputTokens >= 1` check. JavaScript dynamic non-number and non-integer inputs are represented by serde/type-boundary rejection tests because Rust callers cannot construct those invalid typed settings directly. |
| Simulated readable stream utility | verified | `src/util.rs` | `simulate_readable_stream_should_create_a_readable_stream_with_provided_values`; `simulate_readable_stream_should_respect_the_chunk_delay_in_ms_setting`; `simulate_readable_stream_should_handle_empty_values_array`; `simulate_readable_stream_should_handle_different_types_of_values`; `simulate_readable_stream_should_skip_all_delays_when_both_delay_settings_are_null`; `simulate_readable_stream_should_apply_chunk_delays_but_skip_initial_delay_when_initial_delay_in_ms_is_null`; `simulate_readable_stream_should_apply_initial_delay_but_skip_chunk_delays_when_chunk_delay_in_ms_is_null` | Mirrors every portable upstream `simulate-readable-stream.test.ts` case one-to-one: chunks are emitted in order, empty inputs close immediately, generic value shapes are preserved, and injected delay hooks observe the same initial/chunk delay sequence including the upstream distinction between `null` and zero-millisecond delay. Rust exposes the portable pull/collect contract instead of a Web `ReadableStream`, which remains a JavaScript runtime primitive. |
| Server response writer utility | verified | `src/util.rs` | `write_to_server_response_should_write_data_to_server_response`; `write_to_server_response_should_respect_backpressure_and_wait_for_drain_event`; `write_to_server_response_should_set_headers_correctly_when_status_text_is_undefined`; `write_to_server_response_should_set_headers_correctly_when_status_text_is_provided`; `write_to_server_response_should_set_headers_correctly_when_status_text_is_not_set_and_status_is_not_set` | Mirrors every portable upstream `write-to-server-response.test.ts` case one-to-one: status defaults to 200, optional status text selects the proper header shape, headers pass through unchanged, byte chunks are written in order, response finalization occurs after the stream, and backpressure waits for a drain boundary before continuing. Rust uses a `ServerResponseWriter` trait instead of Node's concrete `ServerResponse` and EventEmitter objects. |
| Async iterable stream utility | verified | `src/util.rs` | `create_async_iterable_stream_should_read_all_chunks_from_a_non_empty_stream_using_async_iteration`; `create_async_iterable_stream_should_handle_an_empty_stream_gracefully`; `create_async_iterable_stream_should_maintain_readable_stream_functionality`; `create_async_iterable_stream_should_cancel_stream_on_early_exit_from_for_await_loop`; `create_async_iterable_stream_should_cancel_stream_when_exception_thrown_inside_for_await_loop`; `create_async_iterable_stream_should_not_cancel_stream_when_exception_thrown_inside_for_await_loop`; `create_async_iterable_stream_should_not_allow_iterating_twice_after_breaking`; `create_async_iterable_stream_should_propagate_errors_from_source_stream_to_async_iterable`; `create_async_iterable_stream_should_stop_async_iterable_when_stream_is_cancelled`; `create_async_iterable_stream_should_not_collect_any_chunks_when_iterating_on_already_cancelled_stream`; `create_async_iterable_stream_should_not_throw_when_return_is_called_after_the_stream_completed` | Mirrors every portable upstream `async-iterable-stream.test.ts` case one-to-one: non-empty and empty chunk iteration, readable-stream style collection, early-exit and thrown-error cancellation, natural completion without cancellation, exhausted post-break iteration, source error propagation, cancellation during active iteration, already-cancelled empty iteration, and `return()` after completion. Rust exposes a source trait plus iterator facade instead of a Web `ReadableStream`/`TransformStream`, preserving the portable consumption and cleanup contract. |
| Stitchable stream utility | verified | `src/util.rs` | `create_stitchable_stream_should_return_no_stream_when_immediately_closed`; `create_stitchable_stream_should_return_all_values_from_a_single_inner_stream`; `create_stitchable_stream_should_return_all_values_from_2_inner_streams`; `create_stitchable_stream_should_return_all_values_from_3_inner_streams`; `create_stitchable_stream_should_handle_empty_inner_streams`; `create_stitchable_stream_should_handle_reading_a_single_value_before_it_is_added`; `create_stitchable_stream_should_return_all_values_from_2_inner_streams_when_reads_start_before_they_are_added`; `create_stitchable_stream_should_handle_errors_from_inner_streams`; `create_stitchable_stream_should_cancel_all_inner_streams_when_cancelled`; `create_stitchable_stream_should_throw_an_error_when_adding_a_stream_after_closing`; `create_stitchable_stream_should_immediately_close_the_stream_and_cancel_all_inner_streams`; `create_stitchable_stream_should_throw_an_error_when_adding_a_stream_after_terminating` | Mirrors every portable upstream `create-stitchable-stream.test.ts` case one-to-one: immediate close, one/two/three queued inner streams, empty inner streams, read-before-add pending behavior, queued reads resolving in order, inner stream error propagation, outer cancellation, add-after-close/add-after-terminate errors, and immediate termination cancellation. Rust exposes explicit `Pending`/`Chunk`/`Done` reads instead of JavaScript read promises and Web `ReadableStreamDefaultReader` objects. |
| AI download utility | verified | `src/util.rs` | `download_should_reject_private_ipv4_addresses`; `download_should_reject_localhost`; `download_should_reject_redirects_to_private_ip_addresses`; `download_should_reject_redirects_to_localhost`; `download_should_allow_redirects_to_safe_urls`; `download_should_download_data_successfully_and_match_expected_bytes`; `download_should_allow_inline_data_urls`; `download_should_throw_download_error_when_response_is_not_ok`; `download_should_throw_download_error_when_fetch_throws_an_error`; `download_should_abort_when_response_exceeds_default_size_limit`; `download_should_pass_abort_signal_to_fetch` | Mirrors every portable upstream `util/download/download.test.ts` case one-to-one: initial URL SSRF rejection, final redirect URL SSRF rejection, safe redirects, successful bytes/media-type download with prepared user-agent headers, inline data URLs, non-OK and fetch errors, default size-limit rejection, and abort-signal propagation to the injected transport boundary. Rust exposes `download_with_transport` and `DownloadTransportRequest` instead of JavaScript global `fetch`; exact DOM `AbortSignal` and `DOMException` identity remain JavaScript-runtime-specific. |
| Abort utilities | verified | `src/util.rs`, `crates/ai-sdk-provider/src/language_model.rs` | `merge_abort_signals_*`; `set_abort_timeout_*` | Mirrors every portable upstream `merge-abort-signals.test.ts` and `set-abort-timeout.test.ts` case one-to-one: empty and nullish source filtering, single-signal identity preservation, multi-signal first-abort propagation, already-aborted reason precedence, many-signal propagation, numeric timeout sources, timeout scheduling, timeout cancellation, missing controller/timeout no-op behavior, and timeout reason naming/message. Rust uses a JSON timeout reason with `name: "TimeoutError"` and the upstream-shaped message instead of JavaScript `DOMException` object identity, and a cancellable background timer handle instead of JavaScript fake timers. |
| Callback utilities | verified | `src/util.rs` | `merge_callbacks_should_*`; `notify_should_*` | Mirrors every portable upstream `merge-callbacks.test.ts` and `notify.test.ts` case one-to-one: merged callbacks are started together, the returned future waits for all callbacks to settle, missing callbacks are skipped, callback errors are ignored, `notify` accepts single and array callbacks, async callbacks are awaited together, event typing is preserved through nested event shapes, and repeated notifications reuse the same callback. Rust models JavaScript rejected promises as `CallbackResult::Err` and additionally settles callback panics so generation-style callers keep the same non-breaking behavior. |
| Serial job executor utility | verified | `src/util.rs` | `serial_job_executor_should_*` | Mirrors every portable upstream `serial-job-executor.test.ts` case one-to-one: single job success, multiple jobs in submission order, job error propagation, one-at-a-time execution, mixed success/failure continuation, and queued run calls resolving in submission order even when later jobs are released first. Rust uses a single worker thread and blocking `SerialJobHandle` waits instead of JavaScript promises and `DelayedPromise`, preserving the same serialized execution contract. |
| Partial JSON repair utility | verified | `src/util.rs` | `parse_partial_json_*`; `fix_json_upstream_*` | Mirrors every portable upstream `parse-partial-json.test.ts` case and every portable upstream `fix-json.test.ts` case one-to-one. Rust retains existing grouped regression tests as additive coverage, but the `fix_json_upstream_*` tests now map the original TypeScript cases individually for null/boolean/number/string repair, arrays, objects, nesting, and regression fixtures. |
| `generateText` non-streaming | verified | `src/generate_text.rs` | `generate_text_*` tests in `src/generate_text.rs`; `examples/kitchen_sink.rs` | Covers deterministic model calls, retryable pre-content provider failures up to `maxRetries`, `maxRetries` start-event configuration, tool loops, result shaping, usage, warnings, metadata, and response messages. |
| Tool calling, tool execution, tool approval, repair, refinement, active tools, pruning | verified | `src/generate_text.rs`, `crates/ai-sdk-provider-utils`, `crates/ai-sdk-gateway/src/gateway_tools.rs` | Tool loop, approval, repair, execution, pruning, provider-executed factory, Gateway tools, and mapping tests | Provider-executed deferred results and Gateway provider-executed tools are represented in Rust. Most concrete provider-specific tools remain unported. |
| Open Responses basic text generated result | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_generates_basic_text_response`; `open_responses_provider_extracts_basic_text_usage`; `open_responses_provider_extracts_basic_text_response_id_metadata` | Mirrors the upstream `OpenAIResponsesLanguageModel > doGenerate > basic text response` generated-result tests one-to-one for text content, text item id metadata, detailed usage conversion (`cacheRead`, `noCache`, reasoning/text output split, and raw provider usage), and top-level OpenAI `responseId` provider metadata. |
| Open Responses basic request body and reasoning-model settings | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_sends_model_id_settings_and_input`; `open_responses_provider_keeps_temperature_and_top_p_for_gpt_5_1_reasoning_none`; `open_responses_provider_removes_unsupported_settings_for_o1`; `open_responses_provider_removes_unsupported_settings_for_reasoning_model_*` generated cases | Mirrors upstream `OpenAIResponsesLanguageModel > doGenerate` request-body tests `should send model id, settings, and input`, `should keep temperature and topP for gpt-5.1 models when reasoning effort is none`, `should remove unsupported settings for o1`, and the full `openaiResponsesReasoningModelIds` table. Rust now serializes role-based Responses input messages without the extra `type: "message"` field, preserves GPT-5.1+ sampling parameters only for `reasoningEffort: none`, and emits one Rust test per upstream reasoning model id: `o1`, `o1-2024-12-17`, `o3`, `o3-2025-04-16`, `o3-mini`, `o3-mini-2025-01-31`, `o4-mini`, `o4-mini-2025-04-16`, `gpt-5`, `gpt-5-2025-08-07`, `gpt-5-codex`, `gpt-5-mini`, `gpt-5-mini-2025-08-07`, `gpt-5-nano`, `gpt-5-nano-2025-08-07`, `gpt-5-pro`, `gpt-5-pro-2025-10-06`, `gpt-5.1`, `gpt-5.1-chat-latest`, `gpt-5.1-codex-mini`, `gpt-5.1-codex`, `gpt-5.1-codex-max`, `gpt-5.2`, `gpt-5.2-chat-latest`, `gpt-5.2-pro`, `gpt-5.2-codex`, `gpt-5.3-chat-latest`, `gpt-5.3-codex`, `gpt-5.4`, `gpt-5.4-2026-03-05`, `gpt-5.4-mini`, `gpt-5.4-mini-2026-03-17`, `gpt-5.4-nano`, `gpt-5.4-nano-2026-03-17`, `gpt-5.4-pro`, `gpt-5.4-pro-2026-03-05`, `gpt-5.5`, and `gpt-5.5-2026-04-23`. |
| Open Responses provider-tool outputs prompt conversion | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_includes_provider_tool_outputs_with_multiple_tool_results` | Maps the upstream `convertToOpenAIResponsesInput > provider tool outputs` test one-to-one: mixed shell and apply-patch tool-role results serialize as `shell_call_output` and `apply_patch_call_output` instead of standard function outputs. |
| Open Responses function-tool prompt history | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_includes_client_side_tool_calls_in_prompt` | Maps the upstream `convertToOpenAIResponsesInput > function tools` test one-to-one: client-side assistant tool calls with `providerExecuted: false` remain in prompt history as `function_call` items with JSON-stringified arguments. |
| Open Responses MCP approval response continuation | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_converts_approved_mcp_approval_response_when_stored`; `open_responses_provider_converts_denied_mcp_approval_response_when_stored`; `open_responses_provider_converts_mcp_approval_response_when_unstored`; `open_responses_provider_skips_duplicate_mcp_approval_response`; `open_responses_provider_converts_multiple_mcp_approval_responses`; `open_responses_provider_skips_denied_mcp_tool_output_with_approval_id`; `open_responses_provider_mixes_mcp_approval_response_with_tool_result`; `open_responses_provider_converts_tool_approval_responses_to_mcp_input`; `open_responses_provider_aliases_mcp_calls_from_prompt_approval_metadata`; `open_responses_provider_streams_additional_tool_items` | Maps upstream `MCP tool approval responses` prompt-conversion tests one-to-one: approved and denied responses become `mcp_approval_response`, stored approval responses add item references, unstored responses omit item references, duplicate approval ids are deduplicated, multiple approval ids preserve order, approval-denied synthetic tool outputs are skipped, and mixed regular tool results remain as function outputs. Existing regressions also preserve approval request metadata and alias later MCP calls back to local tool ids. |
| Open Responses stored assistant history item references | verified | `src/open_responses.rs` | `open_responses_provider_uses_item_references_for_stored_assistant_history` | Stored Open Responses prompt conversion now preserves assistant text, reasoning, provider-executed tool calls, and tool results as `item_reference` entries when upstream item ids are present, keeping stored conversation continuation aligned with the Responses API history model. |
| Open Responses conversation history item filtering | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_skips_conversation_history_items`; `open_responses_provider_skips_assistant_text_item_ids_when_conversation_is_set`; `open_responses_provider_skips_assistant_tool_call_item_ids_when_conversation_is_set`; `open_responses_provider_includes_fresh_assistant_text_when_conversation_is_set`; `open_responses_provider_uses_item_references_when_conversation_is_not_set`; `open_responses_provider_skips_reasoning_item_ids_when_conversation_is_set` | Maps the upstream `convertToOpenAIResponsesInput > hasConversation` tests one-to-one: assistant text, assistant tool-call, and reasoning items with existing OpenAI item ids are skipped when `conversation` is set; fresh assistant text without an item id is still sent; stored item ids still become `item_reference` entries when no conversation is set; tool-role outputs remain in the prompt; and the broader regression still emits the upstream warning when `conversation` and `previousResponseId` are combined. |
| Open Responses stored provider-defined tool item references | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_converts_stored_provider_executed_tool_history_to_item_reference` | Mirrors the upstream `provider-defined tools` prompt test for a stored provider-executed `code_interpreter` call/result: the matching Rust crate keeps the single `item_reference` keyed by the original tool-call id when `store: true`. |
| Open Responses tool search prompt history reconstruction | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_reconstructs_hosted_tool_search_call_and_output_with_store_false`; `open_responses_provider_uses_distinct_hosted_tool_search_item_references_when_stored`; `open_responses_provider_serializes_client_tool_search_output_with_call_id_from_tool_role`; `open_responses_provider_uses_client_tool_search_call_id_not_item_id`; `open_responses_provider_reconstructs_hosted_tool_search_history_with_store_false`; `open_responses_provider_reconstructs_client_tool_search_output_with_store_false` | Maps the upstream `provider-defined tools` `tool_search` prompt-history tests one-to-one: hosted/server assistant tool-search calls and outputs reconstruct as `tool_search_call`/`tool_search_output` when `store: false`, stored hosted calls/results become distinct item references, client tool-search outputs from tool-role messages use the tool-call `call_id`, and client tool-search call serialization keeps the OpenAI item id separate from the call id. |
| Open Responses client tool-search generated fixture | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`, `crates/ai-sdk-open-responses/src/fixtures/openai-client-tool-search.1.json` | `open_responses_provider_generates_client_tool_search_fixture`; `open_responses_provider_omits_provider_executed_for_client_tool_search_fixture`; `open_responses_provider_uses_call_id_for_client_tool_search_fixture` | Mirrors upstream OpenAI Responses non-streaming client `tool_search` fixture tests one-to-one: generated client-executed tool-search calls omit `providerExecuted`, use the OpenAI `call_id` as the Rust `toolCallId`, keep the original item id in OpenAI provider metadata, preserve serialized `arguments` plus `call_id`, and forward `store: false` with client `tool_search` request shaping. |
| Open Responses client tool-search streaming fixtures | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_streams_client_tool_search_fixture_as_non_provider_executed_parts`; `open_responses_provider_streams_client_tool_search_omits_provider_executed_flag`; `open_responses_provider_streams_client_tool_search_uses_final_call_id`; `open_responses_provider_streams_function_call_after_client_tool_search_output` | Mirrors upstream OpenAI Responses client `tool_search` streaming tests one-to-one: client-executed streamed `tool_search_call` items emit tool-input start/end and tool-call parts, a dedicated Rust test proves the stream parts omit `providerExecuted`, final `call_id` comes from the done chunk instead of the provisional added-chunk id, `itemId` metadata and returned tool definitions are preserved, the provisional id does not leak into stream output, and the follow-up streamed function call after a client tool-search output is retained. |
| Open Responses file-search non-streaming fixtures | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_generates_file_search_without_results_include_fixture_request_body`; `open_responses_provider_generates_file_search_without_results_include_fixture_content`; `open_responses_provider_generates_file_search_with_results_include_fixture_request_body`; `open_responses_provider_generates_file_search_with_results_include_fixture_content` | Mirrors upstream OpenAI Responses `openai-file-search-tool.1` and `openai-file-search-tool.2` JSON fixtures: each upstream request-body and content test now has an explicit Rust counterpart; request tool shaping maps vector store ids, max results, filters, and ranking options; the include option is forwarded only for `file_search_call.results`; file-search calls/results are provider-executed with the fixture id; results preserve `null` when omitted and map included `file_id` to `fileId`; reasoning/text metadata, file-citation sources, cached/reasoning usage, response metadata, and stop finish reason are retained. |
| Open Responses file-search streaming fixtures | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_streams_file_search_without_results_include`; `open_responses_provider_streams_file_search_with_results_include` | Mirrors upstream OpenAI Responses `file_search` streaming fixture tests with and without `include: ["file_search_call.results"]`: streamed provider-executed file-search calls use the fixture id and alias, emit `{}` tool input, map final query arrays, preserve `results: null` when results are not included, forward the OpenAI include option only for the results fixture, and map included result fields from `file_id` to `fileId` while preserving filename, score, attributes, and text. |
| Open Responses code-interpreter and image-generation generated fixtures | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`, `crates/ai-sdk-open-responses/src/fixtures/openai-code-interpreter-tool.1.json`, `crates/ai-sdk-open-responses/src/fixtures/openai-image-generation-tool.1.json` | `open_responses_provider_generates_code_interpreter_fixture_results`; `open_responses_provider_generates_image_generation_fixture_results` | Mirrors upstream OpenAI Responses `openai-code-interpreter-tool.1` and `openai-image-generation-tool.1` JSON fixtures: request tool shaping forwards code-interpreter output includes plus image generation options, generated provider-executed hosted tool calls/results preserve code/container/image payloads, container-file citations emit document sources with OpenAI metadata, empty assistant text metadata is retained, and cached/reasoning usage is preserved. |
| Open Responses code-interpreter and image-generation request-body tests | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_sends_code_interpreter_request_body_with_include_and_tool`; `open_responses_provider_sends_image_generation_request_body_with_tool` | Mirrors the upstream non-streaming `should send request body with include and tool` tests for hosted `openai.code_interpreter` and `openai.image_generation`: code interpreter sends the automatic `code_interpreter_call.outputs` include plus auto container tool, and image generation sends output format, quality, size, and partial image options in the Responses tool shape. |
| Open Responses code-interpreter annotation streaming fixture | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_streams_code_interpreter_results_with_annotations` | Mirrors upstream OpenAI Responses `openai-code-interpreter-tool.1` streaming fixture coverage for hosted `code_interpreter`: streamed code input deltas are completed into the provider-executed tool call input with `containerId`, tool results preserve `outputs`, `container_file_citation` annotations emit a document source with filename/media type/OpenAI `fileId` and `containerId` metadata, and final text-end metadata keeps the raw OpenAI annotation payload. |
| Open Responses image-generation streaming fixture | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_streams_image_generation_fixture_results` | Mirrors upstream OpenAI Responses `openai-image-generation-tool.1` streaming fixture coverage for hosted `image_generation`: streamed image-generation calls emit provider-executed tool calls with `{}` input, partial image events become preliminary tool results, the completed output item becomes the final tool result, and the empty assistant message still emits text start/end metadata with the upstream item id. |
| Open Responses local-shell generated fixture | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`, `crates/ai-sdk-open-responses/src/fixtures/openai-local-shell-tool.1.json` | `open_responses_provider_generates_local_shell_fixture_call` | Mirrors upstream OpenAI Responses `openai-local-shell-tool.1` non-streaming fixture coverage for `openai.local_shell`: the request tool maps to `{ "type": "local_shell" }` without hosted includes, empty reasoning metadata is preserved, the completed `local_shell_call` maps to the configured `shell` tool call with action JSON and OpenAI item metadata, no synthetic tool result is emitted, and cached/reasoning usage remains aligned. |
| Open Responses local-shell and web-search request-body tests | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_sends_local_shell_request_body_with_tool`; `open_responses_provider_sends_web_search_request_body_with_include_and_tool` | Mirrors upstream non-streaming request-body tests for `openai.local_shell` and `openai.web_search`: local shell emits only the local-shell tool shape, while web search emits the hosted web-search tool plus automatic `web_search_call.action.sources` include. |
| Open Responses local-shell streaming fixture | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_streams_local_shell_fixture_call` | Mirrors upstream OpenAI Responses `openai-local-shell-tool.1` streaming fixture coverage for `openai.local_shell`: the request tool maps to `{ "type": "local_shell" }`, streamed reasoning start/end metadata is preserved, the completed `local_shell_call` maps to the configured `shell` tool call with action JSON and item metadata, no synthetic tool result is emitted, and final response metadata, finish reason, service tier, and usage are preserved. |
| Open Responses shell generated fixture | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`, `crates/ai-sdk-open-responses/src/fixtures/openai-shell-tool.1.json` | `open_responses_provider_generates_shell_fixture_request_body`; `open_responses_provider_generates_shell_fixture_call` | Mirrors upstream OpenAI Responses `openai-shell-tool.1` non-streaming fixture coverage for `openai.shell`: the request tool maps to `{ "type": "shell" }` without hosted includes, the completed local shell call maps to the configured `shell` tool call with command action JSON and OpenAI item metadata, no synthetic tool result is emitted for local execution, and cached/reasoning usage remains aligned. |
| Open Responses shell streaming fixture | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_streams_shell_fixture_multiresponse` | Mirrors upstream OpenAI Responses `openai-shell-tool.1` streaming fixture coverage for `openai.shell`: the request tool maps to `{ "type": "shell" }`, multiple streamed Responses objects each emit response metadata instead of suppressing the second id, the shell call maps final command action JSON and item metadata without a synthetic result for local execution, and the follow-up assistant text plus final response metadata, finish reason, service tier, and usage are preserved. |
| Open Responses shell container generated fixture | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`, `crates/ai-sdk-open-responses/src/fixtures/openai-shell-container.1.json` | `open_responses_provider_generates_shell_container_fixture_request_body`; `open_responses_provider_generates_shell_container_fixture_content` | Mirrors upstream OpenAI Responses `openai-shell-container.1` non-streaming fixture coverage for `openai.shell` with `environment.type = containerAuto`: the request maps to `container_auto`, generated shell calls are marked provider-executed, shell output items map stdout/stderr/outcome with `exit_code` converted to `exitCode`, assistant text follows the tool result, and cached/reasoning usage remains aligned. |
| Open Responses shell container streaming fixture | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_streams_shell_container_fixture` | Mirrors upstream OpenAI Responses `openai-shell-container.1` streaming fixture coverage for `openai.shell` with `environment.type = containerAuto`: the request maps to `container_auto`, streamed shell calls are marked provider-executed, shell output items map stdout/stderr/outcome with `exit_code` converted to `exitCode`, assistant text streams after the tool result, and final response metadata, finish reason, service tier, and usage are preserved. |
| Open Responses shell environment fixtures | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`, `crates/ai-sdk-open-responses/src/fixtures/openai-shell-skills.1.json`, `crates/ai-sdk-open-responses/src/fixtures/openai-shell-skills.1.chunks.txt` | `open_responses_provider_generates_shell_environment_fixture_request_body`; `open_responses_provider_generates_shell_environment_fixture_content`; `open_responses_provider_streams_shell_environment_fixture` | Mirrors upstream OpenAI Responses `openai-shell-skills.1` non-streaming and streaming fixture coverage for `openai.shell` with `environment.type = containerAuto`: the non-streaming upstream request-body and content tests now have explicit Rust counterparts; the request maps to `container_auto`, provider-executed shell calls preserve OpenAI item metadata, shell output items map stdout/stderr/outcome with `exit_code` converted to `exitCode`, final assistant STOP-instruction text is preserved, and response metadata plus cached/reasoning usage are retained. |
| Open Responses shell container multiturn generated fixture | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`, `crates/ai-sdk-open-responses/src/fixtures/openai-shell-container-multiturn.1.json` | `open_responses_provider_generates_shell_container_multiturn_fixture_request_body`; `open_responses_provider_generates_shell_container_multiturn_fixture_content` | Mirrors upstream OpenAI Responses `openai-shell-container-multiturn.1` non-streaming fixture coverage for `openai.shell` follow-up prompts: stored provider-executed shell calls and assistant text use item references, shell output history is replayed as `shell_call_output`, the container request maps to `container_auto`, follow-up assistant text is returned with OpenAI item metadata, and cached/reasoning usage remains aligned. |
| Open Responses shell container multiturn streaming fixture | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`, `crates/ai-sdk-open-responses/src/fixtures/openai-shell-container-multiturn.1.chunks.txt` | `open_responses_provider_streams_shell_container_multiturn_fixture` | Mirrors upstream OpenAI Responses `openai-shell-container-multiturn.1` streaming fixture coverage for `openai.shell` follow-up prompts: stored provider-executed shell calls and assistant text use item references, shell output history is replayed as `shell_call_output`, the container request maps to `container_auto`, follow-up assistant text streams with OpenAI item metadata, and final response metadata, finish reason, service tier, and usage are preserved. |
| Open Responses shell local multiturn generated fixture | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`, `crates/ai-sdk-open-responses/src/fixtures/openai-shell-local-multiturn.1.json` | `open_responses_provider_generates_shell_local_multiturn_fixture_request_body`; `open_responses_provider_generates_shell_local_multiturn_fixture_content` | Mirrors upstream OpenAI Responses `openai-shell-local-multiturn.1` non-streaming fixture coverage for `openai.shell` follow-up prompts without a container: stored shell calls and assistant text use item references, tool-role shell output history is replayed as `shell_call_output`, the request tool remains `{ "type": "shell" }`, follow-up assistant text is returned with OpenAI item metadata, and cached/reasoning usage remains aligned. |
| Open Responses shell local multiturn streaming fixture | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`, `crates/ai-sdk-open-responses/src/fixtures/openai-shell-local-multiturn.1.chunks.txt` | `open_responses_provider_streams_shell_local_multiturn_fixture` | Mirrors upstream OpenAI Responses `openai-shell-local-multiturn.1` streaming fixture coverage for `openai.shell` follow-up prompts without a container: stored shell calls and assistant text use item references, tool-role shell output history is replayed as `shell_call_output`, the request tool remains `{ "type": "shell" }`, follow-up assistant text streams with OpenAI item metadata, and final response metadata, finish reason, service tier, and usage are preserved. |
| Open Responses MCP generated fixture | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`, `crates/ai-sdk-open-responses/src/fixtures/openai-mcp-tool.1.json` | `open_responses_provider_generates_mcp_tool_fixture_request_body`; `open_responses_provider_generates_mcp_tool_fixture_content` | Mirrors upstream OpenAI Responses `openai-mcp-tool.1` non-streaming fixture coverage for `openai.mcp`: request tool shaping maps server label, URL, description, and default `require_approval`, `mcp_list_tools` items are not emitted as model tool calls, reasoning metadata is preserved around the provider-executed dynamic MCP call/result, the tool result preserves OpenAI item metadata, final assistant text is returned with OpenAI item metadata, and cached/reasoning usage remains aligned. |
| Open Responses MCP streaming fixture | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_streams_mcp_tool_fixture` | Mirrors upstream OpenAI Responses `openai-mcp-tool.1` streaming fixture coverage for `openai.mcp`: request tool shaping maps server label, URL, description, and default `require_approval`, `mcp_list_tools` items do not emit model tool calls, streamed MCP calls/results are provider-executed dynamic parts with item metadata, reasoning metadata is preserved around the MCP calls, final assistant text streams, and cached/reasoning usage is retained. |
| Open Responses MCP approval non-streaming fixtures | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`, `crates/ai-sdk-open-responses/src/fixtures/openai-mcp-tool-approval.{1,2,3,4}.json` | `open_responses_provider_generates_mcp_approval_request_fixture_turn_1`; `open_responses_provider_generates_mcp_approval_denial_fixture_turn_2`; `open_responses_provider_generates_mcp_approval_retry_fixture_turn_3`; `open_responses_provider_generates_mcp_approval_result_fixture_turn_4` | Mirrors upstream OpenAI Responses `openai-mcp-tool-approval.1` through `openai-mcp-tool-approval.4` JSON fixtures for `openai.mcp`: required-approval request tool shaping, generated dynamic MCP approval tool calls, approval request ids, denial continuation input, retry approval requests, approved MCP call/result mapping, final assistant text, stop finish reason, response metadata, service tier, and usage are retained from byte-matched fixture files. |
| Open Responses MCP approval streaming fixtures | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`, `crates/ai-sdk-open-responses/src/fixtures/openai-mcp-tool-approval.{1,2,3,4}.chunks.txt` | `open_responses_provider_streams_mcp_approval_request_fixture_turn_1`; `open_responses_provider_streams_mcp_approval_denial_fixture_turn_2`; `open_responses_provider_streams_mcp_approval_retry_fixture_turn_3`; `open_responses_provider_streams_mcp_approval_result_fixture_turn_4` | Mirrors upstream OpenAI Responses `openai-mcp-tool-approval.1` through `openai-mcp-tool-approval.4` streaming fixtures for `openai.mcp`: required-approval request tool shaping, model-generated dynamic MCP approval tool calls, approval request ids, denial continuation input, retry approval requests, approved MCP call/result mapping, final assistant text streaming, and usage/response metadata are retained from byte-matched fixture files. |
| Open Responses unstored hosted tool-result fallback | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_excludes_provider_executed_tool_history_with_store_false`; `open_responses_provider_warns_for_unstored_hosted_tool_results` | Mirrors the upstream `provider-defined tools` unstored hosted-tool test one-to-one: provider-executed assistant `web_search` calls and results are excluded from the prompt when `store: false`, surrounding assistant text remains, and the OpenAI hosted-tool warning is emitted instead of failing conversion. |
| Open Responses assistant execution-denied tool-result filtering | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_skips_execution_denied_tool_results_in_assistant_messages`; `open_responses_provider_skips_json_wrapped_execution_denied_tool_results`; `open_responses_provider_skips_assistant_execution_denied_tool_results` | Maps the upstream `provider-defined tools` execution-denied tests one-to-one: direct and JSON-wrapped `execution-denied` assistant tool results are skipped without hosted-tool fallback warnings, and surrounding assistant text remains as separate message items. |
| Open Responses local shell prompt history reconstruction | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_converts_stored_local_shell_history_to_item_reference`; `open_responses_provider_converts_unstored_local_shell_history_to_call_items`; `open_responses_provider_reconstructs_local_shell_history_with_store_false` | Maps upstream `convertToOpenAIResponsesInput > provider-defined tools > local shell` tests one-to-one: stored local-shell assistant calls become item references while tool-role results remain `local_shell_call_output`, and unstored local-shell history reconstructs `local_shell_call` plus `local_shell_call_output` with snake-case request action fields. |
| Open Responses shell prompt history reconstruction | verified | `src/open_responses.rs` | `open_responses_provider_reconstructs_shell_history_with_store_false`; `open_responses_provider_reconstructs_stored_assistant_shell_outputs` | Open Responses prompt conversion now recognizes the OpenAI shell provider tool during history serialization, maps shell prompt calls to `shell_call` with snake-case request action fields when not stored, maps shell outputs from tool-role messages to `shell_call_output`, and reconstructs assistant shell outputs even when `store` is enabled because upstream shell output item ids differ from shell call item ids. |
| Open Responses apply-patch prompt history reconstruction | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_converts_stored_apply_patch_history_to_item_reference`; `open_responses_provider_converts_unstored_apply_patch_create_file_to_call`; `open_responses_provider_converts_unstored_apply_patch_update_file_to_call`; `open_responses_provider_converts_unstored_apply_patch_delete_file_to_call`; `open_responses_provider_reconstructs_apply_patch_history_with_store_false`; `open_responses_provider_reconstructs_stored_apply_patch_outputs` | Maps upstream `convertToOpenAIResponsesInput > provider-defined tools > apply_patch` tests one-to-one: stored apply-patch calls become item references while tool-role outputs remain `apply_patch_call_output`, and unstored create, update, and delete operations reconstruct `apply_patch_call` items with call id, item id, status, and operation payloads. |
| Open Responses custom provider-tool prompt history reconstruction | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_reconstructs_custom_tool_calls`; `open_responses_provider_reconstructs_custom_tool_outputs`; `open_responses_provider_converts_custom_tool_call_to_custom_tool_call_input_item`; `open_responses_provider_json_stringifies_non_string_custom_tool_call_input`; `open_responses_provider_converts_stored_custom_tool_call_to_item_reference`; `open_responses_provider_converts_custom_tool_text_result_to_output`; `open_responses_provider_converts_custom_tool_json_result_to_output`; `open_responses_provider_converts_execution_denied_custom_tool_result_to_output`; `open_responses_provider_converts_custom_tool_content_result_to_output`; `open_responses_provider_converts_custom_tool_file_url_content_result_to_output`; `open_responses_provider_falls_back_to_function_call_without_custom_provider_tool_names` | Maps the upstream `convertToOpenAIResponsesInput > custom tool calls` tests one-to-one in the package-owned Open Responses crate: `openai.custom` provider tool names serialize assistant calls as `custom_tool_call` with string-preserving or JSON-stringified input, stored calls become `item_reference`, text/JSON/execution-denied/content/file-url tool-role outputs become `custom_tool_call_output`, and absent custom-provider tool registration falls back to `function_call` with JSON-stringified arguments. |
| Open Responses assistant text prompt metadata | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_converts_assistant_text_part_to_output_text`; `open_responses_provider_includes_commentary_phase_on_assistant_text`; `open_responses_provider_includes_final_answer_phase_on_assistant_text`; `open_responses_provider_omits_phase_when_not_set_on_assistant_text`; `open_responses_provider_reconstructs_text_item_id_and_phase_with_store_false`; `open_responses_provider_uses_item_references_for_stored_assistant_history` | Maps the upstream `convertToOpenAIResponsesInput` assistant text and phase cases one-to-one: assistant text becomes `output_text`, OpenAI item IDs preserve `phase: "commentary"` and `phase: "final_answer"` when present, phase is omitted when unset, and stored text items still collapse to `item_reference` entries when `store` is enabled. |
| Open Responses phase metadata fixtures | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`, `crates/ai-sdk-open-responses/src/fixtures/openai-phase.1.json`, `crates/ai-sdk-open-responses/src/fixtures/openai-phase.1.chunks.txt` | `open_responses_provider_generates_phase_fixture_metadata`; `open_responses_provider_streams_phase_fixture_metadata` | Mirrors upstream OpenAI Responses `openai-phase.1` JSON and streaming fixtures byte-for-byte: generated text content and streamed `text-start`/`text-end` parts preserve OpenAI `itemId` metadata plus `phase: "commentary"` and `phase: "final_answer"` for the two assistant message items, while response/service-tier metadata and cached/reasoning usage remain aligned. |
| Open Responses encrypted reasoning fixtures | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`, `crates/ai-sdk-open-responses/src/fixtures/openai-reasoning-encrypted-content.1.json`, `crates/ai-sdk-open-responses/src/fixtures/openai-reasoning-encrypted-content.1.chunks.txt` | `open_responses_provider_generates_reasoning_encrypted_content_fixture`; `open_responses_provider_streams_reasoning_encrypted_content_fixture` | Mirrors upstream OpenAI Responses `openai-reasoning-encrypted-content.1` JSON and streaming fixtures byte-for-byte: generated reasoning preserves `reasoningEncryptedContent`, streamed reasoning emits start/delta/end parts with encrypted-content metadata, tool calls stream through the calculator flow, final text and response metadata/usage stay aligned, and the chat-only `maxCompletionTokens` provider option is asserted not to leak into Responses request bodies. |
| Open Responses inline reasoning generated variants | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_generates_reasoning_with_summary_parts`; `open_responses_provider_generates_reasoning_with_empty_summary`; `open_responses_provider_generates_encrypted_reasoning_with_summary_parts`; `open_responses_provider_generates_encrypted_reasoning_with_empty_summary`; `open_responses_provider_generates_multiple_reasoning_blocks` | Maps upstream OpenAI Responses non-streaming inline reasoning tests one-to-one: generated summary arrays become separate reasoning content parts, empty summary arrays emit an empty reasoning part, encrypted content is preserved in OpenAI provider metadata, multiple reasoning/message blocks retain output ordering, request-body `reasoning` and `include` shaping matches upstream, and response metadata/usage are preserved. |
| Open Responses inline reasoning summary streaming variants | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_streams_reasoning_with_summary_parts`; `open_responses_provider_streams_reasoning_with_empty_summary`; `open_responses_provider_streams_encrypted_reasoning_with_summary_parts`; `open_responses_provider_streams_encrypted_reasoning_with_empty_summary`; `open_responses_provider_streams_multiple_reasoning_blocks` | Maps upstream OpenAI Responses inline reasoning stream tests one-to-one: summary-part start/delta/end events with multiple summary indices, empty summary arrays, encrypted-content start/final metadata, multiple interleaved reasoning/message blocks, request-body `reasoning` and `include` shaping, final text reconstruction, finish metadata, and usage are preserved. |
| Open Responses compaction fixtures | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`, `crates/ai-sdk-open-responses/src/fixtures/openai-compaction.1.json`, `crates/ai-sdk-open-responses/src/fixtures/openai-compaction.1.chunks.txt` | `open_responses_provider_generates_compaction_fixture`; `open_responses_provider_streams_compaction_fixture` | Mirrors upstream OpenAI Responses `openai-compaction.1` JSON and streaming fixtures byte-for-byte: request bodies forward `store: false` and `context_management` with `compact_threshold`, generated and streamed text content stay aligned with the fixture, compaction items map to `openai.compaction` custom content with `itemId`, `type`, and `encryptedContent` metadata, and response/service-tier/usage metadata is preserved. |
| Open Responses reasoning prompt history reconstruction | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_converts_single_reasoning_text_part_with_store_false`; `open_responses_provider_converts_single_reasoning_part_with_encrypted_content`; `open_responses_provider_matches_null_encrypted_content_reasoning_upstream_case`; `open_responses_provider_creates_empty_reasoning_summary_for_initial_empty_text`; `open_responses_provider_creates_empty_reasoning_summary_for_initial_empty_text_with_encryption`; `open_responses_provider_warns_when_appending_empty_reasoning_text`; `open_responses_provider_merges_consecutive_reasoning_parts_with_same_id`; `open_responses_provider_drops_unencrypted_reasoning_parts_with_store_false`; `open_responses_provider_creates_separate_reasoning_messages_for_different_ids`; `open_responses_provider_handles_reasoning_across_multiple_assistant_messages_when_stored`; `open_responses_provider_handles_reasoning_across_multiple_assistant_messages_when_unstored`; `open_responses_provider_handles_complex_reasoning_sequences_with_tool_interactions`; `open_responses_provider_warns_when_reasoning_part_has_no_provider_options`; `open_responses_provider_warns_when_reasoning_lacks_openai_item_id_options`; `open_responses_provider_includes_unstored_reasoning_without_item_id_when_encrypted`; `open_responses_provider_warns_when_reasoning_lacks_item_id_and_encrypted_content`; `open_responses_provider_reconstructs_reasoning_history_with_store_false`; `open_responses_provider_warns_for_unstored_reasoning_without_encrypted_content` | Maps upstream `convertToOpenAIResponsesInput > reasoning messages (store: false)` tests one-to-one: single reasoning parts, duplicate upstream encrypted/null-titled cases, empty initial summaries, empty append warnings, same-id merging, unencrypted drop behavior, separate IDs, stored and unstored multi-message histories, complex reasoning/tool interleaving, missing provider-option warnings, encrypted reasoning without an item id, and missing item/encryption warnings. The broader Rust regressions remain as extra coverage for mixed assistant text and combined warning behavior. |
| Open Responses compaction prompt history reconstruction | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_reconstructs_compaction_history_with_store_false`; `open_responses_provider_uses_item_references_for_stored_assistant_history`; `open_responses_provider_converts_compaction_to_item_reference_when_stored`; `open_responses_provider_converts_compaction_to_full_item_when_unstored`; `open_responses_provider_skips_compaction_item_ids_when_conversation_is_set`; `open_responses_provider_converts_compaction_alongside_fresh_text_when_unstored`; `open_responses_provider_converts_compaction_alongside_text_to_item_references_when_stored` | Maps the upstream `convertToOpenAIResponsesInput > compaction` tests one-to-one: assistant `openai.compaction` custom prompt history becomes stored `item_reference` entries when `store` is true, full `compaction` items with encrypted content when `store` is false, is skipped when `conversation` indicates the item already lives in the conversation, and preserves ordering beside assistant text for both stored and unstored prompts. |
| Open Responses standard tool-result outputs | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_converts_single_json_tool_result_to_function_call_output`; `open_responses_provider_converts_single_text_tool_result_to_function_call_output`; `open_responses_provider_converts_execution_denied_tool_result_to_function_call_output`; `open_responses_provider_converts_multiple_tool_results_in_single_message`; `open_responses_provider_converts_standard_tool_result_outputs` | Maps upstream `convertToOpenAIResponsesInput > tool messages` standard output tests one-to-one: JSON, text, execution-denied, and multiple tool-result parts become Responses `function_call_output` items with the expected `call_id` and stringified output. The broader Rust regression remains as extra coverage for error-text and error-json output variants. |
| Open Responses multipart tool-result file outputs | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_converts_tool_result_content_text_to_output_array`; `open_responses_provider_converts_tool_result_content_image_data_to_input_image`; `open_responses_provider_forwards_image_detail_on_tool_result_image_data`; `open_responses_provider_forwards_image_detail_on_tool_result_image_url`; `open_responses_provider_converts_tool_result_content_image_url_to_input_image`; `open_responses_provider_converts_tool_result_content_pdf_data_to_input_file`; `open_responses_provider_converts_tool_result_content_file_url_to_input_file`; `open_responses_provider_converts_tool_result_mixed_content_with_file_url`; `open_responses_provider_converts_tool_result_mixed_text_image_pdf_content`; `open_responses_provider_converts_tool_result_file_content_outputs` | Maps upstream multipart tool-result `content` tests one-to-one: text parts become `input_text`, image data and URLs become `input_image`, omitted TypeScript `detail: undefined` is absent in Rust JSON, OpenAI `imageDetail` provider options are forwarded for data and URL images, PDF data and URLs become `input_file`, missing data filenames default to `data`, and mixed content preserves ordering. The broader Rust regression remains as extra combined coverage. |
| Open Responses assistant function-call prompt arguments | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`, `crates/ai-sdk-provider/src/language_model.rs` | `open_responses_provider_converts_assistant_text_and_tool_call_to_function_call`; `open_responses_provider_defaults_missing_assistant_tool_call_input_to_empty_object`; `open_responses_provider_converts_stored_assistant_tool_call_ids_to_item_references`; `open_responses_provider_converts_multiple_assistant_tool_calls_in_single_message`; `open_responses_provider_stringifies_assistant_function_call_arguments`; `assistant_tool_call_part_deserializes_missing_input_as_null` | Maps upstream assistant tool-call prompt conversion tests one-to-one: assistant text plus tool-call parts become an assistant output-text item followed by a `function_call`, omitted TypeScript tool-call input deserializes into Rust's null input representation and defaults to `{}`, stored text/tool-call item IDs become `item_reference` entries when `store` is true, and multiple tool calls preserve order. The broader Rust regression remains as extra coverage for already-string inputs and explicit null stringification. |
| Open Responses function tool strict modes | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_passes_strict_true_function_tool`; `open_responses_provider_passes_strict_false_function_tool`; `open_responses_provider_omits_undefined_strict_function_tool`; `open_responses_provider_passes_mixed_strict_function_tools`; `open_responses_provider_prepares_function_tool_strict_modes` | Open Responses function-tool request preparation now maps upstream `prepareResponsesTools` strict-mode cases one-to-one: `strict: true` and `strict: false` are preserved on the tool definition, omitted strict mode leaves the request field absent, and mixed strict/non-strict/default function tools preserve per-tool settings. Rust request-body assertions omit TypeScript-only `undefined` fields but preserve the portable serialized OpenAI shape. |
| Open Responses language-model request parameters and tools | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_sends_lmstudio_request_parameters_body`; `open_responses_provider_sends_lmstudio_tools_request_body` | Maps upstream `open-responses-language-model.test.ts` `doGenerate > request parameters` and `doGenerate > tools` request-body cases one-to-one: the LMStudio request-parameter body preserves `max_output_tokens`, `temperature`, `top_p`, `presence_penalty`, `frequency_penalty`, and JSON schema text format, and the two-function-tool body preserves descriptions, input schemas, and explicit `strict: true` on the `search` tool. |
| Open Responses language-model function tool choice | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_sends_tool_choice_auto`; `open_responses_provider_sends_tool_choice_none`; `open_responses_provider_sends_tool_choice_required`; `open_responses_provider_sends_tool_choice_specific_tool` | Maps upstream `open-responses-language-model.test.ts` `doGenerate > tool choice` tests one-to-one: `auto`, `none`, `required`, and specific function-tool selection each have a dedicated Rust test that asserts the generated model id, user prompt, function tool schema, and Responses `tool_choice` request body shape. |
| Open Responses reasoning request options | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_maps_top_level_reasoning_high_to_effort`; `open_responses_provider_maps_top_level_reasoning_minimal_to_low`; `open_responses_provider_maps_top_level_reasoning_none_to_none`; `open_responses_provider_passes_top_level_reasoning_xhigh_directly`; `open_responses_provider_omits_reasoning_when_not_specified`; `open_responses_provider_sends_detailed_reasoning_summary_from_provider_options`; `open_responses_provider_combines_top_level_reasoning_with_summary`; `open_responses_provider_sends_concise_reasoning_summary_from_provider_options`; `open_responses_provider_omits_reasoning_for_empty_provider_options`; additive `open_responses_provider_filters_non_reasoning_generic_provider_options`; `open_responses_provider_omits_provider_default_top_level_reasoning_for_openai`; `open_responses_provider_maps_top_level_reasoning_none_for_openai`; `open_responses_provider_maps_top_level_reasoning_minimal_for_openai`; `open_responses_provider_maps_top_level_reasoning_low_for_openai`; `open_responses_provider_maps_top_level_reasoning_medium_for_openai`; `open_responses_provider_maps_top_level_reasoning_high_for_openai`; `open_responses_provider_maps_top_level_reasoning_xhigh_for_openai`; `open_responses_provider_prefers_provider_reasoning_effort_over_top_level_for_openai`; `open_responses_provider_strips_temperature_and_top_p_for_top_level_reasoning_model`; `open_responses_provider_keeps_sampling_parameters_for_top_level_reasoning_none` | Generic `@ai-sdk/open-responses` top-level reasoning still maps to Responses `reasoning.effort`, including upstream `high`, `xhigh`, `minimal` to `low` compatibility warnings, `none`, and omitted/default passthrough, while provider `reasoningSummary` maps `detailed`, `auto`, and `concise` to `reasoning.summary` without leaking provider-option fields into the request body. OpenAI/Azure/Gateway wrapper routes now mirror upstream `@ai-sdk/openai` by keeping top-level `minimal` as `reasoning.effort: "minimal"`, omitting provider-default reasoning, letting provider `reasoningEffort` override top-level reasoning, stripping sampling parameters for non-`none` reasoning models, and preserving sampling parameters when top-level reasoning is `none` on non-reasoning models. |
| Open Responses OpenAI reasoning model table rows | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_sends_reasoning_options_for_reasoning_model_o1`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_o1_2024_12_17`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_o3`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_o3_2025_04_16`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_o3_mini`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_o3_mini_2025_01_31`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_o4_mini`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_o4_mini_2025_04_16`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_2025_08_07`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_codex`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_mini`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_mini_2025_08_07`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_nano`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_nano_2025_08_07`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_pro`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_pro_2025_10_06`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_1`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_1_chat_latest`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_1_codex_mini`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_1_codex`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_1_codex_max`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_2`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_2_chat_latest`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_2_pro`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_2_codex`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_3_chat_latest`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_3_codex`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_4`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_4_2026_03_05`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_4_mini`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_4_mini_2026_03_17`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_4_nano`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_4_nano_2026_03_17`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_4_pro`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_4_pro_2026_03_05`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_5`; `open_responses_provider_sends_reasoning_options_for_reasoning_model_gpt_5_5_2026_04_23`; `open_responses_provider_warns_for_reasoning_options_on_non_reasoning_model_gpt_4_1`; `open_responses_provider_warns_for_reasoning_options_on_non_reasoning_model_gpt_4_1_2025_04_14`; `open_responses_provider_warns_for_reasoning_options_on_non_reasoning_model_gpt_4_1_mini`; `open_responses_provider_warns_for_reasoning_options_on_non_reasoning_model_gpt_4_1_mini_2025_04_14`; `open_responses_provider_warns_for_reasoning_options_on_non_reasoning_model_gpt_4_1_nano`; `open_responses_provider_warns_for_reasoning_options_on_non_reasoning_model_gpt_4_1_nano_2025_04_14`; `open_responses_provider_warns_for_reasoning_options_on_non_reasoning_model_gpt_4o`; `open_responses_provider_warns_for_reasoning_options_on_non_reasoning_model_gpt_4o_2024_05_13`; `open_responses_provider_warns_for_reasoning_options_on_non_reasoning_model_gpt_4o_2024_08_06`; `open_responses_provider_warns_for_reasoning_options_on_non_reasoning_model_gpt_4o_2024_11_20`; `open_responses_provider_warns_for_reasoning_options_on_non_reasoning_model_gpt_4o_audio_preview`; `open_responses_provider_warns_for_reasoning_options_on_non_reasoning_model_gpt_4o_audio_preview_2024_12_17`; `open_responses_provider_warns_for_reasoning_options_on_non_reasoning_model_gpt_4o_search_preview`; `open_responses_provider_warns_for_reasoning_options_on_non_reasoning_model_gpt_4o_search_preview_2025_03_11`; `open_responses_provider_warns_for_reasoning_options_on_non_reasoning_model_gpt_4o_mini_search_preview`; `open_responses_provider_warns_for_reasoning_options_on_non_reasoning_model_gpt_4o_mini_search_preview_2025_03_11`; `open_responses_provider_warns_for_reasoning_options_on_non_reasoning_model_gpt_4o_mini`; `open_responses_provider_warns_for_reasoning_options_on_non_reasoning_model_gpt_4o_mini_2024_07_18`; `open_responses_provider_warns_for_reasoning_options_on_non_reasoning_model_gpt_3_5_turbo_0125`; `open_responses_provider_warns_for_reasoning_options_on_non_reasoning_model_gpt_3_5_turbo`; `open_responses_provider_warns_for_reasoning_options_on_non_reasoning_model_gpt_3_5_turbo_1106`; `open_responses_provider_warns_for_reasoning_options_on_non_reasoning_model_gpt_5_chat_latest` | Splits upstream `packages/openai/src/responses/openai-responses-language-model.test.ts` `it.each(openaiResponsesReasoningModelIds)` and `it.each(nonReasoningModelIds)` provider-option cases into one Rust test function per upstream table row. Reasoning rows assert `reasoningEffort: "low"` and `reasoningSummary: "auto"` serialize into `reasoning.effort` and `reasoning.summary` with no warnings; non-reasoning rows assert the request omits `reasoning` and emits the upstream unsupported `reasoningEffort` warning. The older broad loop remains as additive coverage, not a replacement for table-row parity. |
| Open Responses no-schema JSON response format | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`, `src/vercel_ai_gateway.rs` | `open_responses_provider_maps_no_schema_json_format_by_route`; `vercel_ai_gateway_openai_responses_maps_no_schema_json_response_format`; ignored live `live_vercel_ai_gateway_openai_responses_no_schema_json_response_format` | OpenAI, Azure, and Vercel AI Gateway Responses wrapper routes now mirror upstream OpenAI Responses by mapping `responseFormat: { type: "json" }` without a schema to `text.format.type: "json_object"`, while the generic Open Responses package route keeps its upstream `json_schema` no-schema behavior. Gateway has credential-gated live coverage for the JSON response-format path, last run 2026-05-20. |
| Open Responses unsupported standard call options | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_warns_for_unsupported_standard_call_options` | Open Responses call preparation now has explicit coverage for the upstream warning snapshot: `topK`, `seed`, `presencePenalty`, `frequencyPenalty`, and `stopSequences` emit unsupported warnings in upstream order and do not leak into the Responses request body. |
| Open Responses system-message request shaping | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_sends_instructions_from_system_message`; `open_responses_provider_joins_multiple_system_messages_with_newlines`; `open_responses_provider_converts_single_system_message_to_instructions`; `open_responses_provider_joins_system_messages_with_newlines_for_input_conversion`; `open_responses_provider_returns_no_instructions_without_system_messages`; `open_responses_provider_handles_system_message_with_user_and_assistant_messages`; `open_responses_provider_converts_openai_message_chain_with_system_input_items`; `open_responses_provider_converts_system_message_to_system_role`; `open_responses_provider_converts_system_message_to_developer_role`; `open_responses_provider_removes_system_message`; `open_responses_provider_maps_openai_system_message_modes` | Generic Open Responses providers now map upstream `open-responses-language-model.test.ts` system-message request cases one-to-one: one system message becomes top-level `instructions`, and multiple system messages join with newlines. Generic `convertToOpenResponsesInput` system-message cases also have exact named Rust counterparts for one system message, multiple system messages, no system messages, and system plus user/assistant message chains. OpenAI, Azure, and Vercel AI Gateway Responses wrapper routes also mirror upstream `packages/openai` one-to-one for `systemMessageMode`: system messages can be emitted as `system` or `developer` input items, and `remove` drops them with the upstream warning. |
| Open Responses generic provider option filtering | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_filters_non_reasoning_generic_provider_options`; `vercel_ai_gateway_openai_responses_passes_gateway_provider_options` | Generic Open Responses providers now mirror upstream by accepting only `reasoningSummary` from provider options; broader request-body passthrough remains scoped to OpenAI, Azure, and Gateway wrapper routes that own those Responses API options. |
| Open Responses OpenAI wrapper request option mapping | verified | `src/open_responses.rs` | `open_responses_provider_maps_openai_responses_provider_options_to_request_body`; `open_responses_provider_streams_context_management_options`; `open_responses_provider_warns_for_conversation_with_previous_response_id`; `open_responses_provider_maps_openai_passthrough_option_edges`; `open_responses_provider_falls_back_to_openai_options_for_azure_requests`; `open_responses_provider_prefers_azure_options_over_openai_fallback`; `vercel_ai_gateway_openai_responses_passes_gateway_provider_options` | OpenAI/Azure/Gateway wrapper provider options now map upstream camelCase fields such as `previousResponseId`, `maxToolCalls`, `parallelToolCalls`, `promptCacheKey`, `promptCacheRetention`, `safetyIdentifier`, `serviceTier`, `textVerbosity`, `strictJsonSchema`, `reasoningEffort`, `reasoningSummary`, `contextManagement`, `instructions`, multi-value `include`, `user`, `conversation`, `metadata`, `store`, `truncation`, and `logprobs` to Responses request keys or nested fields on both generate and stream calls where upstream defines them, warn when `conversation` and `previousResponseId` are both set, fall back from Azure to `providerOptions.openai` only when `providerOptions.azure` is absent while retaining Azure provider metadata, preserve wrapper-owned passthrough fields such as Gateway `caching`, and prevent SDK-only flags from leaking into the request body. |
| Open Responses OpenAI wrapper isolated provider-option tests | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_sends_parallel_tool_calls_provider_option`; `open_responses_provider_sends_user_provider_option`; `open_responses_provider_sends_conversation_provider_option`; `open_responses_provider_sends_previous_response_id_provider_option`; `open_responses_provider_matches_metadata_named_user_provider_option_upstream_case`; `open_responses_provider_sends_metadata_provider_option`; `open_responses_provider_sends_instructions_provider_option`; `open_responses_provider_sends_single_include_provider_option`; `open_responses_provider_sends_multiple_include_provider_options`; `open_responses_provider_sends_text_verbosity_low_provider_option`; `open_responses_provider_sends_text_verbosity_medium_provider_option`; `open_responses_provider_sends_text_verbosity_high_provider_option`; `open_responses_provider_sends_prompt_cache_key_provider_option`; `open_responses_provider_sends_prompt_cache_retention_provider_option`; `open_responses_provider_sends_safety_identifier_provider_option`; `open_responses_provider_sends_truncation_auto_provider_option`; `open_responses_provider_sends_truncation_disabled_provider_option`; `open_responses_provider_omits_unspecified_truncation_provider_option`; `open_responses_provider_sends_logprobs_provider_option` | Adds one-to-one Rust coverage for the upstream isolated request-body tests for `parallelToolCalls`, `user`, `conversation`, `previousResponseId`, the upstream metadata-titled case that asserts `user`, actual `metadata` passthrough, `instructions`, single and multiple `include` values, `textVerbosity` low/medium/high, `promptCacheKey`, `promptCacheRetention`, `safetyIdentifier`, `truncation` auto/disabled/omitted, and numeric `logprobs`, so these cases are not only covered indirectly by the broader provider-options matrix. |
| Open Responses OpenAI wrapper isolated response-format tests | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_sends_json_response_format`; `open_responses_provider_sends_json_schema_response_format`; `open_responses_provider_sends_json_schema_response_format_with_strict_json_schema_false` | Adds one-to-one Rust coverage for the upstream isolated `responseFormat` request-body tests: no-schema JSON maps to `text.format.type: "json_object"`, JSON schema emits `text.format.type: "json_schema"` with `strict: true`, and `strictJsonSchema: false` overrides that strict flag without relying on the broader request-parameter test. |
| Open Responses Azure provider metadata keys | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_uses_azure_metadata_key_for_text_result`; `open_responses_provider_uses_azure_metadata_key_for_function_call_content`; `open_responses_provider_streams_azure_metadata_key_for_reasoning_and_finish` | Azure Responses now mirrors upstream provider-metadata key selection for non-streaming text results, non-streaming function-call content, and streaming reasoning/finish events: generated content and stream parts use the `azure` key and do not also emit `openai` metadata. |
| Open Responses OpenAI metadata keys and text deltas | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_uses_openai_metadata_key_for_text_result`; `open_responses_provider_streams_text_deltas_and_openai_finish_metadata` | Mirrors upstream OpenAI Responses provider-metadata key tests for non-Azure providers and the simple `should stream text deltas` SSE fixture: generated text results use only the `openai` provider metadata key, streamed text starts and deltas preserve the provisional item id, text-end metadata follows the final item id, stream response metadata is emitted only for `response.created`, finish metadata keeps the created response id, and cached/reasoning usage is preserved. |
| Open Responses allowed tools request option | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_maps_allowed_tools_to_tool_choice`; `open_responses_provider_maps_allowed_tools_required_mode`; `open_responses_provider_allowed_tools_overrides_request_tool_choice` | OpenAI/Gateway wrapper provider `allowedTools` now mirrors upstream by keeping the full `tools` array for prompt caching while overriding request-level `toolChoice` with Responses `tool_choice: { type: "allowed_tools", mode, tools }`, defaulting mode to `auto`, mapping tool names through provider-tool aliases, supporting `required` mode, and preventing the SDK-only provider option from leaking into the request body. |
| Open Responses web-search, web-search-preview, and local-shell request tools | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_prepares_local_shell_tool`; `open_responses_provider_prepares_web_search_without_options`; `open_responses_provider_prepares_web_search_external_web_access_true`; `open_responses_provider_prepares_web_search_external_web_access_false`; `open_responses_provider_prepares_web_search_with_all_options`; `open_responses_provider_prepares_web_search_filters_without_external_web_access`; `open_responses_provider_resolves_web_search_tool_choice`; `open_responses_provider_prepares_multiple_tools_including_web_search`; `open_responses_provider_prepares_web_search_preview_and_local_shell_tools` | Open Responses provider-tool request preparation now maps upstream `prepareResponsesTools` local-shell and web-search cases one-to-one: local shell emits `{ "type": "local_shell" }`, web search supports omitted options, `externalWebAccess` true/false, filters, `searchContextSize`, `userLocation`, hosted tool-choice resolution, and mixed function plus web-search tool lists. Rust keeps the older `openai.web_search_preview` combined request regression as extra coverage. |
| Open Responses generated computer-use tool calls | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_generates_computer_use_tool_calls` | Mirrors upstream non-streaming `should handle computer use tool calls` from `packages/openai/src/responses/openai-responses-language-model.test.ts`: `computer_call` output produces a provider-executed `computer_use` tool call, a matching `computer_use_tool_result`, assistant text remains ordered after the tool parts, OpenAI item metadata is preserved, and usage is retained. |
| Open Responses generated citation annotations | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_generates_mixed_url_and_file_citations`; `open_responses_provider_generates_file_citation_only`; `open_responses_provider_generates_file_citations_without_optional_fields`; `open_responses_provider_generates_container_file_citation`; `open_responses_provider_generates_file_path_citation` | Mirrors upstream non-streaming mixed citation tests from `packages/openai/src/responses/openai-responses-language-model.test.ts` for URL, file-only, missing-optional-field file citations, container-file citations, and file-path annotations. Rust emits matching text provider metadata plus URL/document source parts, maps OpenAI source metadata to camel-case provider metadata, and preserves file-path citations as `application/octet-stream` documents. |
| Open Responses streaming citation annotations | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_streams_mixed_url_and_file_citations`; `open_responses_provider_streams_file_citations_without_optional_fields`; `open_responses_provider_streams_container_file_citation`; `open_responses_provider_streams_file_path_citation` | Mirrors upstream `packages/openai/src/responses/openai-responses-language-model.test.ts` mixed citation streaming tests for URL, `file_citation`, `container_file_citation`, and `file_path` annotations. Rust emits the matching source variants, preserves raw OpenAI annotation metadata on `text-end`, keeps `responseId: null` when no `response.created` event was streamed, maps file-path citations as `application/octet-stream`, and preserves cached/reasoning usage. |
| Open Responses code-interpreter and image-generation request tools | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_prepares_code_interpreter_auto_container`; `open_responses_provider_prepares_code_interpreter_string_container`; `open_responses_provider_prepares_code_interpreter_file_ids_container`; `open_responses_provider_prepares_code_interpreter_empty_file_ids`; `open_responses_provider_prepares_code_interpreter_undefined_file_ids`; `open_responses_provider_resolves_code_interpreter_tool_choice`; `open_responses_provider_prepares_multiple_tools_including_code_interpreter`; `open_responses_provider_prepares_image_generation_with_all_options`; `open_responses_provider_resolves_image_generation_tool_choice`; `open_responses_provider_prepares_code_interpreter_and_image_generation_options` | Open Responses hosted-tool request preparation now maps upstream `prepareResponsesTools` code-interpreter and image-generation cases one-to-one: auto containers, string containers, container `fileIds`, empty `fileIds`, TypeScript-only undefined `fileIds` represented by an omitted Rust field, hosted tool-choice resolution, mixed function plus code-interpreter tool lists, and image-generation option casing. Rust keeps the existing combined request regression as extra coverage. |
| Open Responses custom provider-tool request formats | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_prepares_custom_tool_with_regex_format`; `open_responses_provider_prepares_custom_tool_with_lark_format`; `open_responses_provider_prepares_multiple_tools_including_custom_tool`; `open_responses_provider_resolves_custom_tool_choice_using_tool_name`; `open_responses_provider_prepares_custom_tool_formats_and_choice` | Open Responses custom provider-tool request preparation now maps upstream `prepareResponsesTools` custom-tool cases one-to-one: regex grammar tools, Lark grammar tools, mixed function plus custom tool lists, and `toolChoice` resolution to `{ type: "custom", name }`. Rust request-body assertions omit TypeScript-only `undefined` fields but preserve the portable serialized OpenAI shape. |
| Open Responses custom provider-tool fixtures | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_generates_custom_tool_fixture_request_body`; `open_responses_provider_generates_custom_tool_fixture_content`; `open_responses_provider_generates_custom_tool_fixture_tool_calls_finish_reason`; `open_responses_provider_streams_custom_tool_fixture` | Mirrors upstream OpenAI Responses `openai-custom-tool.1` JSON and streaming fixtures: the non-streaming upstream request-body, content, and finish-reason tests now have explicit Rust counterparts; request tool shaping emits `type: "custom"` with regex grammar format, generated and streamed custom tool calls preserve the OpenAI item id, streamed input deltas reconstruct the SQL text, response metadata is retained, usage maps to Rust usage fields, and finish reason remains `tool-calls`. |
| Open Responses web-search tool generated fixture | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`; `crates/ai-sdk-open-responses/src/fixtures/openai-web-search-tool.1.json` | `open_responses_provider_generates_web_search_fixture` | Mirrors upstream OpenAI Responses `openai-web-search-tool.1` non-streaming fixture for `should include web search tool call and result in content`: request shaping includes hosted web search sources, provider-executed `webSearch` call/result pairs are emitted for search/open-page/find-in-page actions with upstream camelCase action names, empty reasoning items are preserved, URL citations become source parts plus text annotation metadata, final text and cached/reasoning usage remain aligned. |
| Open Responses web-search tool streaming fixture | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`; `crates/ai-sdk-open-responses/src/fixtures/openai-web-search-tool.1.chunks.txt` | `open_responses_provider_streams_upstream_web_search_tool_fixture` | Mirrors upstream OpenAI Responses `openai-web-search-tool.1` streaming fixture for `should stream web search results (sources, tool calls, tool results)`: request shaping includes hosted web search sources, provider-executed `webSearch` call/result pairs are emitted for search/open-page/find-in-page actions with upstream camelCase action names, empty reasoning items produce start/end parts, streamed URL citations become source parts plus `text-end` annotation metadata, final text is reconstructed from deltas, and response metadata/service-tier/usage remain aligned. |
| Open Responses apply-patch and tool-search request tools | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_prepares_apply_patch_tool`; `open_responses_provider_resolves_apply_patch_tool_choice`; `open_responses_provider_prepares_multiple_tools_including_apply_patch`; `open_responses_provider_prepares_tool_search_tool`; `open_responses_provider_prepares_tool_search_with_deferred_function_tool`; `open_responses_provider_prepares_apply_patch_and_tool_search_tools` | Open Responses request preparation now maps upstream `prepareResponsesTools` apply-patch and tool-search cases one-to-one: apply-patch emits `{ "type": "apply_patch" }`, hosted tool choice resolves to that type, mixed function plus apply-patch tools preserve order, tool-search emits `{ "type": "tool_search" }`, and function tools alongside tool-search preserve OpenAI `deferLoading` as `defer_loading`. Rust request-body assertions omit TypeScript-only `undefined` fields but preserve the portable serialized OpenAI shape. |
| Open Responses hosted tool-search fixtures | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_generates_hosted_tool_search_fixture`; `open_responses_provider_streams_hosted_tool_search_fixture` | Mirrors upstream OpenAI Responses `openai-tool-search.1` JSON and streaming fixtures: hosted `tool_search_call` maps to provider-executed `toolSearch` calls with fixture ids, `call_id: null` is preserved in input, `tool_search_output` aliases back to the hosted call id, returned tool definitions preserve `defer_loading`, strict mode, descriptions, and schemas, OpenAI item metadata is retained for calls/results, and usage/finish metadata stays aligned. |
| Open Responses web-search schema resilience | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_maps_web_search_api_sources`; `open_responses_provider_maps_web_search_missing_action`; `open_responses_provider_streams_web_search_action_query`; `open_responses_provider_streams_web_search_missing_action` | Mirrors upstream OpenAI Responses web-search schema resilience tests: non-streaming API-typed sources are preserved in provider-executed `webSearch` results with assistant text metadata, missing optional `action` payloads map to empty result objects, streaming `action.query` payloads emit provider-executed web-search tool parts and finish usage, and streaming missing-action items emit an empty tool result without failing. |
| Open Responses API schema alignment | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_schema_alignment_matches_annotation_shape_between_stream_and_response`; `open_responses_schema_alignment_aligns_web_search_call_actions`; `open_responses_schema_alignment_aligns_code_interpreter_outputs`; `open_responses_schema_alignment_aligns_file_search_call_results`; `open_responses_schema_alignment_aligns_message_phase_between_added_done_and_response`; `open_responses_schema_alignment_aligns_output_text_logprobs` | Mirrors upstream `packages/openai/src/responses/openai-responses-api.test.ts` schema-alignment tests one-to-one. Rust cannot run TypeScript `expectTypeOf`, so the package-owned crate now proves the equivalent portable contract by parsing matching non-streaming response bodies and streamed SSE chunk events for annotation payloads, web-search actions, code-interpreter outputs, file-search results, message phase, and output-text logprobs, then asserting the preserved Rust metadata/tool-result/provider-metadata shapes match. |
| Open Responses apply-patch fixtures | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_generates_apply_patch_create_file_fixture_request_body`; `open_responses_provider_generates_apply_patch_create_file_fixture_content`; `open_responses_provider_streams_apply_patch_create_file_fixture`; `open_responses_provider_streams_apply_patch_delete_file_fixture` | Mirrors upstream OpenAI Responses `openai-apply-patch-tool.1` JSON fixture and the `openai-apply-patch-tool.1` / `openai-apply-patch-tool-delete.1` streaming fixtures: the non-streaming upstream request-body and content tests now have explicit Rust counterparts; request tool shaping emits `apply_patch`, generated create-file tool calls preserve OpenAI item metadata, streamed create-file diffs and delete-file operations reconstruct snapshot-equivalent tool input, response metadata and service tier are retained, and cached/reasoning usage maps to Rust usage fields. |
| Open Responses shell request environment tools | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_prepares_shell_tool_without_environment_args`; `open_responses_provider_prepares_shell_container_auto_without_skills`; `open_responses_provider_prepares_shell_container_auto_provider_reference_skill`; `open_responses_provider_defaults_shell_skill_reference_version_to_latest`; `open_responses_provider_rejects_unresolved_shell_skill_reference`; `open_responses_provider_prepares_shell_container_auto_inline_skill`; `open_responses_provider_prepares_shell_container_auto_network_policy_disabled`; `open_responses_provider_prepares_shell_container_auto_network_policy_allowlist`; `open_responses_provider_prepares_shell_container_auto_file_ids_and_memory_limit`; `open_responses_provider_prepares_shell_container_reference`; `open_responses_provider_prepares_shell_local_environment_with_skills`; `open_responses_provider_prepares_shell_local_environment_without_explicit_type`; `open_responses_provider_prepares_shell_local_environment_without_skills`; `open_responses_provider_prepares_shell_tool_environment_skills` | Open Responses shell provider-tool request preparation now maps upstream `prepareResponsesTools` shell cases one-to-one: no environment args, `containerAuto` with and without skills, OpenAI provider-reference skills, omitted skill versions defaulting to `latest`, inline skills, disabled and allowlist network policies, `fileIds`, `memoryLimit`, `containerReference`, local environments with and without explicit type/skills, and unresolved provider references. Rust request-body assertions omit TypeScript-only `undefined` fields but preserve the portable serialized OpenAI shape. |
| Open Responses unsupported assistant prompt parts | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_ignores_unsupported_assistant_file_parts` | Assistant prompt conversion now mirrors upstream by ignoring assistant file and reasoning-file parts while still serializing surrounding assistant text and tool calls. |
| Open Responses user prompt text and image conversion | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`; `crates/ai-sdk-provider-utils` | `open_responses_provider_converts_user_text_part_to_input_text`; `open_responses_provider_converts_user_image_url_part`; `open_responses_provider_converts_user_image_base64_data`; `open_responses_provider_converts_user_image_bytes_data`; `open_responses_provider_converts_user_image_file_id_with_prefix`; `open_responses_provider_converts_user_image_provider_reference`; `open_responses_provider_passes_full_image_png_through_unchanged_for_inline_data`; `open_responses_provider_detects_image_subtype_from_inline_bytes_for_top_level_image`; `open_responses_provider_passes_through_url_source_for_top_level_only_image`; `open_responses_provider_normalizes_image_wildcard_via_detection`; `open_responses_provider_detects_wildcard_image_type_from_bytes`; `open_responses_provider_rejects_undetected_wildcard_image_type`; `open_responses_provider_passes_through_top_level_image_url`; `open_responses_provider_adds_openai_image_detail_on_prompt_image`; `open_responses_provider_adds_azure_image_detail_on_prompt_image`; `open_responses_provider_resolves_top_level_image_media_types`; `resolve_full_media_type_detects_inline_byte_subtype`; `resolve_full_media_type_treats_wildcard_as_top_level` | User prompt conversion now maps upstream `convertToOpenAIResponsesInput` text and image cases one-to-one in the package-owned crate: text parts become `input_text`, URL/data/byte images become `input_image`, configured file-id prefixes and provider references become `file_id`, wildcard `image/*` data is detected or rejected, top-level-only image URLs pass through, and provider-specific `imageDetail` is read from both OpenAI and Azure provider option namespaces. Generic `convertToOpenResponsesInput` top-level-only media type cases now have exact named Rust counterparts for full `image/png`, top-level `image` inline detection, top-level image URLs, and `image/*` wildcard detection. |
| Open Responses user file provider references | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`; `src/file_data.rs` | `open_responses_provider_converts_user_image_provider_reference`; `open_responses_provider_converts_user_pdf_provider_reference`; `open_responses_provider_resolves_reference_for_different_provider_options_name`; `open_responses_provider_resolves_reference_for_pdf_parts`; `open_responses_provider_resolves_multiple_references_in_one_message`; `open_responses_provider_rejects_missing_provider_reference_file_part`; `open_responses_provider_rejects_file_parts_with_provider_references`; `provider_reference_resolves_provider_specific_id` | User file prompt conversion now resolves provider references for OpenAI/Azure/Gateway wrapper routes into Responses `input_image.file_id` and `input_file.file_id` parts, including active-provider selection for Azure, PDF references, mixed-provider references in one message, and the upstream missing-provider error path. The generic `packages/open-responses` route mirrors upstream by rejecting provider-reference file parts with the upstream unsupported-functionality error. |
| Open Responses user prompt PDF conversion | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_converts_user_pdf_data_part`; `open_responses_provider_converts_user_pdf_file_id_with_prefix`; `open_responses_provider_converts_user_pdf_provider_reference`; `open_responses_provider_uses_default_filename_for_pdf_file_parts_when_not_provided`; `open_responses_provider_converts_user_pdf_url_part` | User prompt file conversion now maps upstream PDF prompt cases one-to-one in the package-owned crate: PDF data parts become `input_file.file_data` with explicit or default filenames, configured file-id prefixes and provider references become `input_file.file_id`, and URL-backed PDF parts become `input_file.file_url`. |
| Open Responses prompt file defaults and unsupported files | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_uses_default_filename_for_pdf_file_parts_when_not_provided`; `open_responses_provider_rejects_unsupported_file_types_by_default`; `open_responses_provider_passes_through_unsupported_file_types_when_enabled` | User prompt file conversion now defaults unnamed PDF data parts to upstream `part-{index}.pdf`, rejects unsupported non-PDF file data by default, and honors provider `passThroughUnsupportedFiles` for explicit unsupported-file passthrough without leaking that SDK-only flag into the request body. |
| Open Responses deprecated file id prefixes | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`; `src/openai.rs` | `open_responses_provider_converts_image_part_with_custom_file_id_prefix`; `open_responses_provider_converts_pdf_part_with_custom_file_id_prefix`; `open_responses_provider_supports_multiple_file_id_prefixes`; `open_responses_provider_treats_plain_strings_as_base64_data`; `open_responses_provider_treats_file_data_as_base64_without_prefixes`; `open_responses_provider_handles_empty_file_id_prefixes_array`; `open_responses_provider_maps_deprecated_file_id_prefixes`; `openai_provider_responses_uses_default_file_id_prefix` | Prompt file conversion now mirrors upstream soft-deprecated `fileIdPrefixes` one-to-one: configured string prefixes in file data produce Responses `file_id` for image and PDF inputs, multiple prefixes are honored, missing or empty prefixes leave strings as base64 `image_url`/`file_data`, and the OpenAI wrapper supplies the upstream default `file-` prefix. |
| Open Responses PDF input-file fixtures | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_sends_pdf_input_file_request_body`; `open_responses_provider_produces_pdf_input_file_content`; `open_responses_provider_extracts_pdf_input_file_usage`; `open_responses_provider_streams_pdf_input_file_fixture` | Maps upstream `open-responses-language-model.test.ts` `doGenerate > pdf input file` cases one-to-one for `openai-pdf-input-file.1`: URL-backed PDF prompts emit `input_file.file_url`, non-streaming fixture responses produce `Dummy PDF file` text, and usage/cache/reasoning-token totals map to Rust usage fields. The SSE fixture remains covered for text deltas, text end, stop finish reason, and matching usage. |
| Open Responses LMStudio basic response fixture | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_sends_lmstudio_basic_request_body`; `open_responses_provider_produces_lmstudio_basic_content`; `open_responses_provider_extracts_lmstudio_basic_usage` | Maps upstream `open-responses-language-model.test.ts` `doGenerate > basic generation` cases one-to-one for `lmstudio-basic.1`: request body uses model `gemma-7b-it` and the `Hello` prompt, content preserves reasoning and message text parts in order with reasoning item metadata under the provider namespace, and usage/cache/reasoning-token totals map to Rust usage fields plus raw token metadata. |
| Open Responses failed stream response finish handling | verified | `src/open_responses.rs` | `open_responses_provider_stream_failed_response_sets_raw_reason_and_usage`; `open_responses_provider_streams_failed_response_incomplete_details_finish_reason`; `open_responses_provider_preserves_stream_error_event_data` | `response.failed` SSE events now mirror upstream by finishing with unified `error` from `response.error.code` or `response.status` when no incomplete reason is present, using `incomplete_details.reason` when upstream supplies it, mapping `max_output_tokens` to unified `length`, carrying usage from the failed response, and leaving standalone `error` SSE events as error stream parts. |
| Open Responses incomplete stream finish handling | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_stream_incomplete_response_sets_length_finish_reason` | `response.incomplete` SSE events now mirror upstream by finishing from `response.incomplete_details.reason`, mapping `max_output_tokens` to unified `length`, preserving the raw reason, carrying usage, and keeping the initial response id in finish provider metadata. |
| Open Responses finish reason mapping | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_finish_reason_undefined_with_tool_calls_maps_tool_calls`; `open_responses_finish_reason_null_with_tool_calls_maps_tool_calls`; `open_responses_finish_reason_undefined_without_tool_calls_maps_stop`; `open_responses_finish_reason_null_without_tool_calls_maps_stop`; `open_responses_finish_reason_max_output_tokens_maps_length`; `open_responses_finish_reason_content_filter_maps_content_filter`; `open_responses_finish_reason_unknown_with_tool_calls_maps_tool_calls`; `open_responses_finish_reason_unknown_without_tool_calls_maps_other`; additive `open_responses_finish_reason_maps_legacy_max_tokens_to_length` | Maps every upstream `packages/open-responses/src/responses/map-open-responses-finish-reason.test.ts` case one-to-one: `undefined`/`null` with tool calls both produce `tool-calls`, `undefined`/`null` without tool calls both produce `stop`, `max_output_tokens` produces `length`, `content_filter` produces `content-filter`, and unknown `completed` produces `tool-calls` when tool calls are present or `other` otherwise. Rust represents both upstream `undefined` and `null` as `None`, but keeps distinct named tests for each original upstream case. The legacy `max_tokens` alias is retained only as additional Rust coverage. |
| Open Responses response metadata provider shape | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`, `src/open_responses.rs`, `src/vercel_ai_gateway.rs` | `open_responses_provider_maps_openai_responses_provider_options_to_request_body`; `open_responses_provider_streams_text_sources_reasoning_and_compaction_metadata`; `open_responses_provider_generates_phase_fixture_metadata`; `open_responses_provider_streams_phase_fixture_metadata`; `open_responses_provider_generates_text_with_request_and_response_metadata`; `open_responses_provider_streams_text_with_request_and_response_metadata`; `open_responses_provider_preserves_stream_error_event_data`; `vercel_ai_gateway_openai_responses_generates_text`; `vercel_ai_gateway_openai_responses_streams_text` | Non-streaming result provider metadata and streaming finish provider metadata now mirror upstream Responses metadata, including provider-keyed `responseId` plus `serviceTier` and `logprobs` when returned; standard response metadata still carries response id, timestamp, model id, headers, and raw body. |
| Open Responses logprobs provider metadata | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_generates_logprobs_provider_metadata`; `open_responses_provider_streams_logprobs_provider_metadata` | Mirrors upstream OpenAI Responses `should extract logprobs in providerMetadata` and streaming `should handle logprobs` tests: non-streaming response output logprobs are grouped into provider metadata when `providerOptions.openai.logprobs` is set, and streaming output-text delta logprobs are collected onto the finish provider metadata with response id, service tier, stop finish reason, and usage. |
| Open Responses function-call item metadata | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`, `src/open_responses.rs`, `src/vercel_ai_gateway.rs` | `open_responses_provider_maps_function_call_response_and_usage`; `open_responses_provider_streams_lmstudio_tool_call_fixture`; `open_responses_streams_function_call_argument_deltas`; `open_responses_provider_runs_generate_text_tool_loop_end_to_end`; `vercel_ai_gateway_openai_responses_runs_generate_text_tool_loop_end_to_end` | Non-streaming and streaming `function_call` tool-call content now preserves provider-keyed `itemId` and optional `namespace`, matching upstream Responses tool-call content metadata. High-level Open Responses and Gateway tool-loop continuations use stored `item_reference` entries when those item ids are available, while still sending the matching `function_call_output`. |
| `streamText` and language model streaming orchestration | in-progress | `src/stream_text.rs`, `src/lib.rs`, `crates/ai-sdk-provider/src/language_model.rs`, `crates/ai-sdk-provider-utils/src/provider_utils.rs`, `crates/ai-sdk-gateway/src/gateway.rs`, `crates/ai-sdk-openai-compatible/src/openai_compatible.rs`, `crates/ai-sdk-open-responses/src/open_responses.rs` | `stream_text_calls_language_model_do_stream_with_standardized_prompt`; `stream_text_collects_text_deltas_and_finish_metadata`; `stream_text_result_full_stream_sends_text_deltas`; `stream_text_result_full_stream_sends_reasoning_deltas`; `stream_text_result_full_stream_sends_sources`; `stream_text_result_full_stream_sends_custom_parts`; `stream_text_result_full_stream_sends_files`; `stream_text_result_full_stream_sends_files_with_provider_metadata`; `stream_text_result_full_stream_sends_reasoning_files`; `stream_text_result_full_stream_uses_fallback_response_metadata_when_response_metadata_missing`; `stream_text_result_full_stream_sends_tool_calls`; `stream_text_result_full_stream_refines_tool_input_before_execution_parts_and_callbacks`; `stream_text_maps_tool_input_deltas_and_high_level_tool_outputs`; `stream_text_executes_local_tool_and_continues_to_final_text`; `stream_text_invokes_tool_execution_callbacks_for_local_tools`; `stream_text_invokes_lifecycle_callbacks_with_streamed_steps`; `stream_text_invokes_finish_callback_with_completed_records`; `stream_text_invokes_chunk_callback_for_portable_chunks`; `stream_text_invokes_error_callback_for_error_parts`; `stream_text_retries_retryable_pre_stream_errors`; `stream_text_stops_after_max_steps_even_when_tool_calls_continue`; `stream_text_honors_stop_condition_after_streamed_tool_call`; `stream_text_automatic_tool_approval_response_streams_before_tool_result`; `stream_text_applies_denied_tool_approval_to_continuation_messages`; `stream_text_repairs_and_refines_streamed_tool_call_before_execution`; `stream_text_continues_for_deferred_provider_executed_tool_results`; `stream_text_resolves_deferred_provider_tool_errors`; `stream_text_result_converts_to_ui_message_stream`; `stream_text_result_maps_portable_non_text_parts_to_ui_message_stream`; `stream_text_result_ui_message_stream_options_mask_errors_with_on_error`; `stream_text_result_ui_message_stream_options_use_persistence_message_ids`; `stream_text_result_ui_message_stream_options_on_finish_receives_persisted_messages`; `stream_text_result_creates_ui_message_stream_response`; `stream_text_result_maps_abort_part_to_ui_message_stream`; `stream_text_aborts_before_model_call_and_invokes_on_abort`; `stream_text_aborts_after_chunk_callback_and_suppresses_finish`; `stream_text_smooth_stream_transforms_chunks_before_callbacks`; `stream_text_smooth_stream_waits_after_detected_chunks`; `smooth_stream_combines_partial_words`; `smooth_stream_marks_detected_chunks_for_default_delay`; `smooth_stream_supports_custom_and_null_delay_options`; `smooth_stream_supports_line_and_pattern_chunking`; `smooth_stream_supports_detector_chunking_and_validation`; `smooth_stream_preserves_provider_metadata_on_flushed_reasoning_delta`; `stream_text_transform_updates_text_response_and_callbacks`; `stream_text_transform_applies_multiple_transforms_in_order`; `stream_text_transform_updates_tool_calls_and_tool_results`; `stream_text_transform_updates_local_tool_results_after_execution`; `stream_text_transform_updates_finish_metadata_and_usage`; `stream_text_transform_can_stop_stream_with_finish_parts`; `post_json_to_api_aborts_before_transport_call`; `post_json_to_api_aborts_pending_transport_when_signal_fires`; `gateway_model_passes_typed_gateway_provider_options_for_stream`; raw chunk, error chunk, abort chunk, and reasoning/source/custom mapping tests | Dependency-light collector over provider-v4 streams. It standardizes prompts, calls `do_stream`, maps text/reasoning/source/custom/tool/file/reasoning-file/raw/error parts, collects final text, result.fullStream text, reasoning, source, custom, file, reasoning-file, fallback response metadata, and tool-call title/provider-metadata and refined tool-input part ordering with provider metadata, usage, warnings, request/response metadata, raw chunks when requested, retries retryable pre-stream provider failures up to `maxRetries`, exposes max-retry configuration in start/telemetry events, tool-input deltas, high-level parsed tool calls/results, text response helpers, text/reasoning/tool/source/file/custom/abort `toUIMessageStream` chunks with source gating, provider metadata, automatic approval request/response chunks, message-metadata callback sequencing, persistence-mode response message id selection from original UI messages plus `generateMessageId` fallback for non-persistence streams, upstream-style UI-message `onFinish` events with persisted message reconstruction, stream-option-aware UI response helpers, and custom UI error masking for stream errors, invalid tool inputs, and local tool errors while preserving provider-executed tool error payloads, UI response helpers, local streamed tool execution, lifecycle/finish/local tool/chunk/error callbacks, Rust-native abort signal/controller handling with `onAbort`, abort chunk emission before provider calls and after chunk callbacks, provider call-option abort signal propagation, Rust-native `smoothStream` chunking parity for word/line/pattern/detector chunking across text and reasoning deltas before `onChunk`, default/custom/null `delayInMs` scheduling after detected chunks, provider-metadata-preserving flushes, Rust-native arbitrary transforms over collected `TextStreamPart`s before replay with result, step, callback, tool state, local post-execution tool result, finish metadata, finish usage, and stop-style truncation recomputation, and root facade re-exports for the abort controller/signal/event, continuation prompts, total usage across steps, max-step bounds, stop-condition checks, static tool approval, repair, input refinement, and deferred provider-executed tool result continuation/resolution. Provider HTTP request cancellation now propagates through provider-utils and the first-phase Gateway/OpenAI-compatible/Open Responses adapters; remaining retry/backoff details remain unported. |
| `streamText` result.fullStream tool-input/result cases | verified | `src/stream_text.rs` | `stream_text_result_full_stream_sends_tool_call_deltas`; `stream_text_result_full_stream_passes_provider_metadata_on_tool_input_start`; `stream_text_result_full_stream_sends_tool_results`; `stream_text_result_full_stream_sends_delayed_asynchronous_tool_results` | Mirrors the adjacent upstream `packages/ai/src/generate-text/stream-text.test.ts` `result.fullStream` tool-input and tool-result cases one-to-one: streamed tool-input start/delta/end parts preserve order, known static tool-input-start parts include the upstream `dynamic: false` marker, provider metadata passes through on tool-input-start, local tool execution sees the original user message in execution options, local tool results are emitted before finish-step, and a pending asynchronous tool future still resolves before the finish-step part. |
| `streamText` portable error callback cases | verified | `src/stream_text.rs` | `stream_text_invokes_finish_callback_when_error_chunk_occurs_mid_stream`; `stream_text_invokes_error_callback_when_error_occurs_in_second_step`; `stream_text_invokes_error_callback_for_error_parts` | Mirrors the portable upstream `packages/ai/src/generate-text/stream-text.test.ts` error-stream behavior: mid-stream provider error chunks invoke `onError`, still emit finish-step and finish, and still invoke `onFinish` with error finish reason, accumulated text, and usage; second-step provider errors after a tool-call continuation invoke `onError`. Upstream cases where `doStream` rejects as a JavaScript promise are represented through Rust provider error stream parts because the Rust `LanguageModel::do_stream` trait returns a stream result rather than a thrown/rejected value. |
| Text stream response helpers | verified | `src/text_stream_response.rs`; `src/stream_text.rs`; `src/stream_object.rs` | `create_text_stream_response_sets_headers_status_and_encoded_chunks`; `pipe_text_stream_to_response_writes_headers_chunks_and_end`; stream text/object response method assertions | Mirrors upstream `text-stream/create-text-stream-response.ts` and `pipe-text-stream-to-response.ts` with Rust-native collected responses, result convenience methods, and a writer trait instead of JS Web/Node response types. |
| `streamText` UI-message response helper cases | verified | `src/stream_text.rs` | `stream_text_result_to_ui_message_stream_masks_error_messages_by_default`; `stream_text_result_to_ui_message_stream_supports_custom_error_messages`; `stream_text_result_pipe_ui_message_stream_to_response_writes_data_stream_parts`; `stream_text_result_pipe_ui_message_stream_to_response_applies_custom_headers`; `stream_text_result_pipe_ui_message_stream_to_response_masks_error_messages_by_default`; `stream_text_result_pipe_ui_message_stream_to_response_supports_custom_error_messages`; `stream_text_result_pipe_ui_message_stream_to_response_omits_finish_when_send_finish_false`; `stream_text_result_pipe_ui_message_stream_to_response_writes_reasoning_content`; `stream_text_result_pipe_ui_message_stream_to_response_writes_source_content`; `stream_text_result_pipe_ui_message_stream_to_response_writes_file_content` | Mirrors the adjacent upstream `packages/ai/src/generate-text/stream-text.test.ts` `result.toUIMessageStream` default/custom error masking cases and `result.pipeUIMessageStreamToResponse` data stream, custom header/status, default/custom error masking, `sendFinish: false`, reasoning, source, and file content cases. Rust uses the crate's collected response/writer trait boundary instead of Node.js response objects and Web `ReadableStream`s, while preserving upstream SSE chunk shapes and the default `An error occurred.` masking behavior unless a custom `onError` mapper is supplied. |
| `streamText` multiple result consumption | verified | `src/stream_text.rs` | `stream_text_result_supports_text_ui_message_and_full_stream_from_single_result` | Mirrors upstream `packages/ai/src/generate-text/stream-text.test.ts` `multiple stream consumption` by proving one collected Rust `StreamTextResult` can be read as text deltas, serialized full-stream parts, and UI-message chunks without one view consuming or mutating the others. Rust materializes provider streams into owned result fields instead of one-shot JavaScript async iterables/ReadableStreams, so this is the equivalent multi-view contract. |
| UI message streams | in-progress | `src/ui_message_stream.rs`; `src/stream_text.rs` | `create_ui_message_stream_response_sets_sse_headers_and_encoded_chunks`; `create_ui_message_stream_response_preserves_existing_headers_and_encodes_errors`; `ui_message_chunk_serializes_portable_tool_source_and_file_chunks`; `handle_ui_message_stream_finish_injects_id_and_calls_on_finish`; `create_ui_message_stream_invokes_step_and_finish_callbacks`; `create_ui_message_stream_adds_error_chunk_when_execute_returns_error`; `create_ui_message_stream_adds_error_chunk_when_merged_stream_errors`; `stream_text_result_maps_portable_non_text_parts_to_ui_message_stream`; `stream_text_result_ui_message_stream_options_mask_errors_with_on_error`; `stream_text_result_ui_message_stream_options_use_persistence_message_ids`; `stream_text_result_ui_message_stream_options_on_finish_receives_persisted_messages`; `stream_text_result_applies_ui_message_metadata_callback_in_sequence`; `process_ui_message_stream_preserves_portable_non_text_chunks_as_parts`; `process_ui_message_stream_accepts_abort_chunks`; `read_ui_message_stream_invokes_finish_callback_with_final_state`; `pipe_ui_message_stream_to_response_writes_headers_chunks_and_end`; `get_response_ui_message_id_*`; `transform_text_to_ui_message_stream_*`; `read_ui_message_stream_returns_message_states_for_basic_text_stream`; `process_ui_message_stream_accumulates_reasoning_parts`; `process_ui_message_stream_reports_missing_text_delta`; `tool_ui_part_predicates_match_upstream_runtime_shape`; `get_static_tool_name_should_return_the_tool_name_after_the_tool_prefix`; `get_static_tool_name_should_return_the_tool_name_for_tools_that_contains_a_dash`; `is_custom_content_ui_part_should_return_true_for_a_custom_part`; `is_custom_content_ui_part_should_return_true_for_a_custom_part_without_provider_metadata`; `is_custom_content_ui_part_should_return_false_for_a_text_part`; `is_data_ui_part_should_return_true_if_the_part_is_a_data_part`; `is_data_ui_part_should_return_false_if_the_part_is_not_a_data_part`; `validate_ui_messages_should_*`; `safe_validate_ui_messages_should_*`; `last_assistant_message_is_complete_with_tool_calls_matches_upstream_cases`; `last_assistant_message_is_complete_with_approval_responses_matches_upstream_cases` | Text/reasoning/tool/source/file/reasoning-file/custom/approval/abort UI-message chunk contracts, `streamText.toUIMessageStream` conversion for portable non-text parts with source gating, provider metadata, message-metadata callback sequencing, persistence-mode response id reuse/generation from original messages, upstream-style finish and step-finish callback event reconstruction, and custom `onError` masking for stream/tool error chunks, SSE response helpers, portable UI message shape, text-to-UI-message transform, read/process state snapshots including abort-state tracking and finish callback final-state events, Rust-native `createUIMessageStream` writer/write/merge callback flow with fallible execute and merged-stream error chunks, non-text chunk preservation as message parts, metadata merging, error termination, malformed text/reasoning errors, response-message id continuation selection, tool-part detection, static-tool name extraction, custom-content detection, data-part detection, `validateUIMessages`/`safeValidateUIMessages` full 52-case portable corpus for parameter, metadata, data-part, static-tool, dynamic-tool, safe-failure, result-provider-metadata, raw-input, approval, and part-shape validation, and last-assistant tool/approval completion predicates mirror upstream `createUIMessageStream`, `createUIMessageStreamResponse`, `pipeUIMessageStreamToResponse`, `transformTextToUiMessageStream`, `readUIMessageStream`, `processUIMessageStream`, `validate-ui-messages`, `ui-messages`, protocol headers, `getResponseUIMessageId`, `handleUIMessageStreamFinish`, `isToolUIPart`, `lastAssistantMessageIsCompleteWithToolCalls`, and `lastAssistantMessageIsCompleteWithApprovalResponses`. Post-return delayed merge behavior remains unported because the Rust facade currently materializes a collected `Vec` rather than a live `ReadableStream`; JavaScript-specific `validate-ui-messages` type assertions remain compile-time-only while the portable runtime corpus is mapped one-to-one. |
| Chat/completion/object UI transport contracts | in-progress | `src/chat_transport.rs`, `src/completion_transport.rs`, `src/object_transport.rs`, `src/ui_message_stream.rs` | `chat_request_options_serialize_upstream_shape`; `http_chat_transport_builds_default_send_messages_request`; `http_chat_transport_prepare_send_options_match_upstream_callback_input`; `http_chat_transport_prepared_send_request_overrides_defaults`; `http_chat_transport_builds_default_reconnect_request`; `http_chat_transport_prepared_reconnect_request_overrides_defaults`; `default_chat_transport_parses_ui_message_event_stream`; `default_chat_transport_reports_invalid_ui_message_event`; `text_stream_chat_transport_maps_text_to_ui_message_stream`; `process_text_stream_should_process_stream_chunks_correctly`; `process_text_stream_should_handle_empty_streams`; `direct_chat_transport_streams_text_response_from_agent`; `direct_chat_transport_passes_prepared_agent_options`; `direct_chat_transport_applies_ui_message_stream_options`; `direct_chat_transport_converts_ui_messages_to_model_messages_in_order`; `direct_chat_transport_rejects_invalid_ui_message_part_shape`; `direct_chat_transport_reconnect_returns_none`; `convert_ui_messages_maps_static_tool_output_available_to_assistant_and_tool_messages`; `convert_ui_messages_maps_tool_output_error_raw_input_to_error_text`; `convert_ui_messages_maps_dynamic_tool_output_available_tool_name`; `convert_ui_messages_preserves_step_start_blocks_as_assistant_tool_pairs`; `convert_ui_messages_places_provider_executed_tool_result_in_assistant`; `convert_ui_messages_maps_denied_approval_response_to_execution_denied_result`; `convert_ui_messages_skips_unconverted_data_parts`; `convert_ui_messages_maps_file_provider_reference_and_metadata_parts`; `completion_transport_builds_default_request`; `completion_transport_builds_prepared_request_with_overrides`; `completion_transport_processes_text_stream`; `completion_transport_processes_data_event_stream`; `completion_transport_reports_data_event_error_chunks`; `completion_transport_reports_invalid_data_event_chunks`; `object_transport_builds_post_request_with_input_body`; `object_transport_processes_distinct_partial_json_updates`; `object_transport_skips_duplicate_partial_objects`; `object_transport_ignores_empty_chunks_until_json_can_be_repaired`; `object_transport_parses_final_json_for_validation_boundary` | Portable chat transport contract mirrors upstream `ChatTransport`, `ChatRequestOptions`, and deterministic `HttpChatTransport` request construction for send and reconnect flows, including API defaults, body/header merging, credentials, `prepareSendMessagesRequest` callback input shape, prepared request overrides, JSON `Content-Type`, and reconnect stream URL construction. Rust `DefaultChatTransport`, `TextStreamChatTransport`, and `process_text_stream` wrappers now preserve deterministic HTTP request builders while covering upstream response-stream transforms: JSON UI-message SSE parsing with validation errors, text stream conversion into UI-message chunks, byte text-stream decoding into callback text parts, and empty-stream no-callback behavior. Initial Rust-native `DirectChatTransport` mirrors upstream's in-process agent bridge by validating portable UI messages, converting system/user/assistant text, file/provider-reference, unconverted data-part skips, custom/reasoning metadata, and assistant tool-history parts into model messages, splitting assistant blocks on `step-start`, emitting following tool-result messages for local tools, keeping provider-executed tool results inside assistant content, forwarding Rust agent model settings, applying `to_ui_message_stream` options, and returning `None` for reconnect. Completion transport now mirrors upstream `callCompletionApi` portable behavior for POST body/header shaping, default `/api/completion`, `data` and `text` stream protocols, text-delta accumulation, error chunks, and invalid JSON event chunks. Object transport now mirrors upstream `experimental_useObject` portable behavior for POST input body/header shaping, partial JSON repair over accumulated text chunks, deep-equality duplicate suppression, and the final JSON parse boundary before schema validation. Browser fetch, AbortSignal, Web `ReadableStream`, React/Vue hook state, broader approval-state edge cases, and full chat state management remain unported or JS-runtime-specific. |
| Agent and tool-loop agent APIs | in-progress | `src/agent.rs` | `tool_loop_agent_exposes_version_id_and_tools`; `tool_loop_agent_generate_forwards_settings_and_instructions`; `tool_loop_agent_generate_forwards_temperature_to_generate_text`; `tool_loop_agent_generate_forwards_max_output_tokens_to_generate_text`; `tool_loop_agent_generate_forwards_top_p_to_generate_text`; `tool_loop_agent_generate_forwards_top_k_to_generate_text`; `tool_loop_agent_generate_forwards_presence_penalty_to_generate_text`; `tool_loop_agent_generate_forwards_frequency_penalty_to_generate_text`; `tool_loop_agent_generate_forwards_stop_sequences_to_generate_text`; `tool_loop_agent_generate_forwards_seed_to_generate_text`; `tool_loop_agent_generate_forwards_headers_to_generate_text`; `tool_loop_agent_prepare_call_can_shape_provider_options`; `tool_loop_agent_generate_rejects_invalid_call_options_schema_before_model_call`; `tool_loop_agent_generate_passes_valid_call_options_schema`; `tool_loop_agent_generate_passes_abort_signal_to_generate_text`; `tool_loop_agent_generate_passes_timeout_to_tool_execution`; `tool_loop_agent_merges_generate_start_callbacks_in_order`; `tool_loop_agent_generate_calls_on_step_start_from_constructor`; `tool_loop_agent_generate_calls_on_step_start_from_method`; `tool_loop_agent_generate_merges_on_step_start_callbacks_in_order`; `tool_loop_agent_generate_on_step_start_passes_event_information`; `tool_loop_agent_generate_calls_on_step_finish_from_constructor`; `tool_loop_agent_generate_calls_on_step_finish_from_method`; `tool_loop_agent_generate_merges_on_step_finish_callbacks_in_order`; `tool_loop_agent_generate_on_step_finish_passes_step_result_to_callback`; `tool_loop_agent_generate_calls_on_finish_from_constructor`; `tool_loop_agent_generate_calls_on_finish_from_method`; `tool_loop_agent_generate_merges_on_finish_callbacks_in_order`; `tool_loop_agent_generate_on_finish_passes_event_information`; `tool_loop_agent_uses_upstream_twenty_step_default_for_tool_loop`; `tool_loop_agent_generate_calls_on_tool_execution_start_from_constructor`; `tool_loop_agent_generate_calls_on_tool_execution_start_from_method`; `tool_loop_agent_generate_merges_on_tool_execution_start_callbacks_in_order`; `tool_loop_agent_generate_on_tool_execution_start_passes_event_information`; `tool_loop_agent_generate_calls_on_tool_execution_end_from_constructor`; `tool_loop_agent_generate_calls_on_tool_execution_end_from_method`; `tool_loop_agent_generate_merges_on_tool_execution_end_callbacks_in_order`; `tool_loop_agent_generate_on_tool_execution_end_passes_event_information_on_success`; `tool_loop_agent_merges_tool_execution_callbacks_in_order`; `tool_loop_agent_stream_delegates_to_stream_text`; `tool_loop_agent_stream_forwards_include_raw_chunks_to_stream_text`; `tool_loop_agent_stream_passes_abort_signal_to_stream_text`; `tool_loop_agent_stream_passes_timeout_to_tool_execution`; `tool_loop_agent_stream_calls_on_tool_execution_start_from_constructor`; `tool_loop_agent_stream_calls_on_tool_execution_start_from_method`; `tool_loop_agent_stream_merges_on_tool_execution_start_callbacks_in_order`; `tool_loop_agent_stream_on_tool_execution_start_passes_event_information`; `tool_loop_agent_stream_calls_on_tool_execution_end_from_constructor`; `tool_loop_agent_stream_calls_on_tool_execution_end_from_method`; `tool_loop_agent_stream_merges_on_tool_execution_end_callbacks_in_order`; `tool_loop_agent_stream_on_tool_execution_end_passes_event_information_on_success`; `tool_loop_agent_generate_calls_per_call_integration_listeners_for_all_lifecycle_events`; `tool_loop_agent_stream_calls_per_call_integration_listeners_for_all_lifecycle_events`; `tool_loop_agent_generate_calls_globally_registered_integration_listeners`; `tool_loop_agent_stream_calls_globally_registered_integration_listeners`; `tool_loop_agent_generate_includes_configured_runtime_context_properties_in_telemetry`; `tool_loop_agent_stream_includes_configured_runtime_context_properties_in_telemetry`; `tool_loop_agent_generate_calls_integration_listeners_alongside_agent_callbacks`; `tool_loop_agent_stream_calls_integration_listeners_alongside_agent_callbacks`; `tool_loop_agent_generate_does_not_break_when_an_integration_listener_panics`; `tool_loop_agent_stream_does_not_break_when_an_integration_listener_panics`; `tool_loop_agent_merges_stream_finish_callbacks_in_order`; `tool_loop_agent_stream_merges_on_step_start_callbacks_in_order`; `tool_loop_agent_stream_on_step_start_passes_event_information`; `tool_loop_agent_stream_merges_on_step_finish_callbacks_in_order`; `tool_loop_agent_stream_on_step_finish_passes_step_result_to_callback`; `tool_loop_agent_stream_calls_on_finish_from_constructor`; `tool_loop_agent_stream_calls_on_finish_from_method`; `tool_loop_agent_stream_on_finish_passes_event_information`; `direct_chat_transport_streams_text_response_from_agent`; `create_agent_ui_stream_response_uses_tool_model_output_for_ui_tool_results`; `create_agent_ui_stream_response_calls_on_finish_with_auto_original_messages` | Initial `ToolLoopAgent` wrapper mirrors upstream agent version/id/tools, shared settings, default twenty-step loops, generate/stream delegation, model/request option forwarding, prepare-call shaping, callback merging, step-start, step-finish, finish, and tool-execution start/end callback event forwarding, telemetry integration listener forwarding including global listeners, runtime-context filtering, callback interleaving, panic isolation, and per-call Rust abort/timeout request controls for provider calls and local tool execution, and is now exercised through `DirectChatTransport` and `create_agent_ui_stream_response`. The agent UI response helper converts UI message history through tool `toModelOutput`, reuses original UI messages for `onFinish`, and emits the standard UI-message stream response. Remaining generic agent call-options type-level parity, runtime-context typing, and broader agent surfaces remain unported. |
| `generateObject` non-streaming structured output | verified | `src/generate_object.rs` | `generate_object_*` tests; `generate_object_accepts_experimental_telemetry_alias`; `generate_object_retries_retryable_pre_content_errors`; `generate_object_callback_panics_do_not_break_generation` | Non-streaming object output is covered, including retryable pre-content provider failures up to `maxRetries`, callback panic isolation, and deprecated `experimental_telemetry` aliases telemetry without adding telemetry config fields to start callback events; `streamObject` is tracked separately. |
| `streamObject` | in-progress | `src/stream_object.rs`, `src/lib.rs`, `crates/ai-sdk-provider/src/language_model.rs`, `crates/ai-sdk-provider-utils/src/provider_utils.rs`, `crates/ai-sdk-gateway/src/gateway.rs`, `crates/ai-sdk-openai-compatible/src/openai_compatible.rs`, `crates/ai-sdk-open-responses/src/open_responses.rs` | `stream_object_calls_model_with_json_response_format_and_standardized_prompt`; `stream_object_collects_partial_objects_text_and_finish_metadata`; `stream_object_result_full_stream_matches_upstream_object_chunks`; `stream_object_result_full_stream_sends_finish_provider_metadata_and_timestamp`; `stream_object_array_output_formats_single_chunk_text_delta`; `stream_object_enum_output_streams_value_and_sends_response_format`; `stream_object_enum_output_completes_unambiguous_prefixes`; `stream_object_enum_output_handles_non_ambiguous_values`; `stream_object_no_schema_output_streams_partial_objects_without_response_schema`; `stream_object_repair_text_repairs_json_parse_error`; `stream_object_repair_text_repairs_type_validation_error`; `stream_object_repair_text_handles_repair_returning_none`; `stream_object_repair_text_repairs_json_wrapped_with_markdown_code_blocks`; `stream_object_repair_text_reports_no_object_when_parsing_still_fails`; `stream_object_invokes_lifecycle_callbacks_with_streamed_step`; `stream_object_step_finish_reports_ms_to_first_chunk`; `stream_object_callback_panics_do_not_break_stream`; `stream_object_accepts_experimental_telemetry_alias`; `stream_object_type_counterpart_*`; `stream_object_object_stream_invokes_on_error_callback_with_error`; `stream_object_invokes_error_callback_for_error_parts`; `stream_object_retries_retryable_pre_stream_errors`; `stream_object_aborts_before_model_call_and_suppresses_finish`; `stream_object_aborts_after_model_call_and_suppresses_finish`; `post_json_to_api_aborts_before_transport_call`; `post_json_to_api_aborts_pending_transport_when_signal_fires`; `gateway_model_passes_typed_gateway_provider_options_for_generate`; array/enum/error/warning/callback tests | Initial dependency-light collector over provider-v4 streams. It sends JSON response formats, accumulates text deltas, emits partial objects/text/full-stream parts with upstream object/text/finish ordering plus finish provider metadata and timestamps, unwraps array/enum/no-schema outputs, maps array output text deltas to upstream array syntax, streams complete array elements, applies enum prefix partial-output rules and enum response-format schema construction, exposes typed Rust accessors for final object, partial-object stream entries, and array elements, exposes text response helpers, parses final objects, applies upstream-style repair callbacks to final parse and validation failures including markdown code-fence cleanup and failed repaired text, invokes start/step-start/step-finish/finish lifecycle callbacks with a shared call id and `msToFirstChunk`, ignores synchronous callback panics so callback failures do not break the stream, runs `onError` callbacks for provider error stream parts including the Rust counterpart for upstream `doStream` rejection, carries provider warnings on the result and lifecycle callbacks, aliases deprecated `experimental_telemetry` without exposing telemetry option fields on start callback events, retries retryable pre-stream provider failures up to `maxRetries`, exposes max-retry configuration in start events, and now supports a Rust-native abort controller/signal that propagates to provider call options and first-phase provider HTTP requests, emits an abort-shaped error before provider calls or between returned stream parts, and suppresses step-finish/finish callbacks and end telemetry. Remaining stream-result edge cases remain unported. |
| Embeddings: `embed`, `embedMany` | verified | `src/embed.rs` | `embed_calls_model_with_single_value_and_maps_result`; `embed_accepts_experimental_telemetry_alias`; `embed_many_accepts_experimental_telemetry_alias`; `embed_many_*` tests | Deprecated `experimental_telemetry` alias behavior now has one-to-one Rust counterparts for both embed APIs. Provider implementations remain unported. |
| Image generation: `generateImage` and `experimental_generateImage` | verified | `src/generate_image.rs` | `generate_image_*` tests | Provider implementations remain unported. |
| Speech generation: `generateSpeech` and experimental alias | verified | `src/generate_speech.rs` | Speech generation tests | Provider implementations remain unported. |
| Video generation: `generateVideo` and experimental alias | verified | `src/generate_video.rs` | Video generation tests, including `generate_video_forwards_abort_signal_to_model_call` and `generate_video_forwards_abort_signal_to_download_callback` | Provider implementations remain unported. |
| Transcription: `transcribe` and experimental alias | verified | `src/transcribe.rs` | `transcribe_*` tests, including `transcribe_forwards_abort_signal_to_model_call` and `transcribe_forwards_abort_signal_to_download_callback` | Provider implementations remain unported. |
| Reranking: `rerank` | verified | `src/rerank.rs` | `rerank_*` tests; `rerank_accepts_experimental_telemetry_alias` | Deprecated `experimental_telemetry` alias behavior now has a named Rust counterpart. Provider implementations remain unported. |
| File upload: `uploadFile` | verified | `src/upload_file.rs` | `upload_file_*` tests | Provider implementations remain unported. |
| Skill upload: `uploadSkill` | verified | `src/upload_skill.rs` | `upload_skill_*` tests | Provider implementations remain unported. |
| Provider registry | in-progress | `src/registry.rs` | `create_provider_registry_*` tests; `create_provider_registry_should_wrap_all_language_models_accessed_through_the_provider_registry` | Core provider/model lookup, custom separators, missing-provider/model errors, files/skills interfaces, and language-model middleware wrapping through registry lookup are covered. Remaining upstream registry option gap: image-model middleware wrapping through registry lookup. Gateway-specific registry helpers remain unported. |
| Language model middleware: wrap/default settings | verified | `src/language_model_middleware.rs` | `wrap_language_model_model_property_should_pass_through_by_default`; `wrap_language_model_model_property_should_use_middleware_override_model_id_if_provided`; `wrap_language_model_model_property_should_use_model_id_parameter_if_provided`; `wrap_language_model_provider_property_should_pass_through_by_default`; `wrap_language_model_provider_property_should_use_middleware_override_provider_if_provided`; `wrap_language_model_provider_property_should_use_provider_id_parameter_if_provided`; `wrap_language_model_supported_urls_property_should_pass_through_by_default`; `wrap_language_model_supported_urls_property_should_use_middleware_override_if_provided`; `wrap_language_model_should_call_transform_params_middleware_for_do_generate`; `wrap_language_model_should_call_wrap_generate_middleware`; `wrap_language_model_should_call_transform_params_middleware_for_do_stream`; `wrap_language_model_should_call_wrap_stream_middleware`; `wrap_language_model_should_support_models_that_use_context_in_supported_urls`; `wrap_language_model_should_call_multiple_transform_params_middlewares_in_sequence_for_do_generate`; `wrap_language_model_should_call_multiple_transform_params_middlewares_in_sequence_for_do_stream`; `wrap_language_model_should_chain_multiple_wrap_generate_middlewares_in_the_correct_order`; `wrap_language_model_should_chain_multiple_wrap_stream_middlewares_in_the_correct_order`; `default_settings_middleware_*`; `add_tool_input_examples_middleware_*`; `extract_json_middleware_*`; `extract_reasoning_middleware_*`; `simulate_streaming_middleware_*` tests | Mirrors upstream v4 model-level hooks plus complete portable `wrapLanguageModel` identity/supported-URL/transform/wrap sequencing cases, default settings, tool input example description transforms, extract-JSON transforms, extract-reasoning transforms, and simulated streaming over Rust `Vec<LanguageModelStreamPart>` streams. Rust composes multiple language middlewares through nested wrappers instead of a JavaScript array argument; the upstream array-mutation identity check is JavaScript-runtime-specific and documented as non-portable. The default-settings tests now split the portable upstream cases for default application, user precedence, provider-options merging, zero-valued temperature, max output tokens, stop sequences, topP, headers, and empty/absent provider options; the JavaScript-only explicit `temperature: null as any` case is represented by Rust's typed `Option<f64>` boundary rather than a runtime null value. |
| Embedding model middleware: wrap/default settings | verified | `src/embedding_model_middleware.rs` | `wrap_embedding_model_model_property_should_pass_through_by_default`; `wrap_embedding_model_model_property_should_use_middleware_override_model_id_if_provided`; `wrap_embedding_model_model_property_should_use_model_id_parameter_if_provided`; `wrap_embedding_model_provider_property_should_pass_through_by_default`; `wrap_embedding_model_provider_property_should_use_middleware_override_provider_if_provided`; `wrap_embedding_model_provider_property_should_use_provider_id_parameter_if_provided`; `wrap_embedding_model_max_embeddings_per_call_property_should_pass_through_by_default`; `wrap_embedding_model_max_embeddings_per_call_property_should_use_middleware_override_if_provided`; `wrap_embedding_model_supports_parallel_calls_property_should_pass_through_by_default`; `wrap_embedding_model_supports_parallel_calls_property_should_use_middleware_override_if_provided`; `wrap_embedding_model_should_call_transform_params_middleware_for_do_embed`; `wrap_embedding_model_should_call_wrap_embed_middleware`; `wrap_embedding_model_should_call_multiple_transform_params_middlewares_in_sequence_for_do_embed`; `wrap_embedding_model_should_chain_multiple_wrap_embed_middlewares_in_the_correct_order`; `default_embedding_settings_middleware_applies_headers_without_overriding_params`; `default_embedding_settings_middleware_deep_merges_provider_options`; `default_embedding_settings_middleware_preserves_none_when_no_defaults_or_params`; `default_embedding_settings_middleware_should_merge_headers`; `default_embedding_settings_middleware_should_handle_empty_default_headers`; `default_embedding_settings_middleware_should_handle_empty_param_headers`; `default_embedding_settings_middleware_should_handle_both_headers_being_undefined`; `default_embedding_settings_middleware_should_handle_empty_default_provider_options`; `default_embedding_settings_middleware_should_handle_empty_param_provider_options`; `default_embedding_settings_middleware_should_handle_both_provider_options_being_undefined` | Mirrors upstream v4 hooks plus the complete portable `wrapEmbeddingModel` identity/capability/transform/wrap sequencing cases and the complete portable `defaultEmbeddingSettingsMiddleware` case set. Rust composes multiple embedding middlewares through nested wrappers instead of a JavaScript array argument; the upstream array-mutation identity check is JavaScript-runtime-specific and documented as non-portable. |
| Image model middleware: wrap | verified | `src/image_model_middleware.rs` | `wrap_image_model_model_property_should_pass_through_by_default`; `wrap_image_model_model_property_should_use_middleware_override_model_id_if_provided`; `wrap_image_model_model_property_should_use_model_id_parameter_if_provided`; `wrap_image_model_provider_property_should_pass_through_by_default`; `wrap_image_model_provider_property_should_use_middleware_override_provider_if_provided`; `wrap_image_model_provider_property_should_use_provider_id_parameter_if_provided`; `wrap_image_model_max_images_per_call_property_should_pass_through_by_default`; `wrap_image_model_max_images_per_call_property_should_use_middleware_override_if_provided`; `wrap_image_model_should_call_transform_params_middleware_for_do_generate`; `wrap_image_model_should_call_wrap_generate_middleware`; `wrap_image_model_should_support_models_that_use_context_in_max_images_per_call`; `wrap_image_model_should_call_multiple_transform_params_middlewares_in_sequence_for_do_generate`; `wrap_image_model_should_chain_multiple_wrap_generate_middlewares_in_the_correct_order` | Mirrors upstream v4 hooks plus the complete portable `wrapImageModel` identity/capability/transform/wrap sequencing cases. Rust composes multiple image middlewares through nested wrappers instead of a JavaScript array argument; the upstream array-mutation identity check is JavaScript-runtime-specific and documented as non-portable. |
| Provider wrapping middleware | verified | `src/provider_middleware.rs` | `wrap_provider_wraps_all_language_model_lookups`; `wrap_provider_with_image_middleware_wraps_all_image_model_lookups`; passthrough and optional-interface tests | Mirrors upstream `middleware/wrap-provider.ts` for provider-v4 Rust providers: wraps every language lookup, optionally wraps image lookups, forwards embedding and optional provider extensions. Provider-v2/v3 conversion remains tracked separately. |
| Provider `getErrorMessage` helper | verified | `crates/ai-sdk-provider/src/provider.rs` | `get_error_message_returns_unknown_error_for_null`; `get_error_message_returns_unknown_error_for_undefined`; `get_error_message_returns_string_as_is`; `get_error_message_returns_empty_string_as_is`; `get_error_message_includes_error_type_prefix_for_basic_error`; `get_error_message_includes_type_error_prefix`; `get_error_message_includes_range_error_prefix`; `get_error_message_returns_error_name_for_empty_message`; `get_error_message_returns_type_error_name_for_empty_message`; `get_error_message_handles_custom_error_subclasses`; `get_error_message_respects_custom_to_string_overrides`; `get_error_message_handles_custom_error_subclass_with_empty_message`; `get_error_message_json_stringifies_plain_objects`; `get_error_message_json_stringifies_numbers`; `get_error_message_json_stringifies_booleans`; `get_error_message_json_stringifies_arrays` | Mirrors upstream `packages/provider/src/errors/get-error-message.test.ts` one-to-one: nullable values become `unknown error`, strings including empty strings pass through, JavaScript error names and custom `toString` output are represented through Rust `Display`, and JSON-like objects, numbers, booleans, and arrays serialize to stable compact JSON strings. |
| Provider-utils nullish filtering helpers | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `filter_nullable_removes_null_and_undefined_values_from_value_list`; `filter_nullable_preserves_other_falsy_values`; `remove_undefined_entries_should_remove_undefined_entries_from_record`; `remove_undefined_entries_should_handle_empty_object`; `remove_undefined_entries_should_handle_object_with_all_undefined_values`; `remove_undefined_entries_should_remove_null_values`; `remove_undefined_entries_should_preserve_falsy_values_except_null_and_undefined`; `remove_undefined_entries_preserves_manual_null_json_values_for_rust_callers` | Mirrors upstream `filter-nullable.test.ts` and `remove-undefined-entries.test.ts` one-to-one: nullish values are omitted from lists and records, empty and all-missing records return empty maps, and falsy-but-present values remain. The extra Rust-only manual `JsonValue::Null` case documents the generic `Option<T>` boundary without replacing the upstream nullish tests. |
| Provider-utils type validation | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `validate_types_upstream_should_return_validated_object_for_valid_input`; `validate_types_upstream_should_throw_type_validation_error_for_invalid_input`; `safe_validate_types_upstream_should_return_validated_object_for_valid_input`; `safe_validate_types_upstream_should_return_error_object_for_invalid_input`; existing Rust context/transformation regressions | Mirrors every portable upstream `validate-types.test.ts` case one-to-one for successful `validateTypes`, failed `validateTypes` with `TypeValidationError` value/message/cause, successful `safeValidateTypes` preserving raw input, and failed `safeValidateTypes` returning an error object plus raw input. Existing Rust tests remain additive coverage for validation context, transformation, and schema-without-validator boundaries. |
| Provider-utils secure JSON parsing | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `secure_json_parse_upstream_parses_object_string`; `secure_json_parse_upstream_parses_null_string`; `secure_json_parse_upstream_parses_zero_string`; `secure_json_parse_upstream_parses_string_string`; `secure_json_parse_upstream_allows_constructor_property_with_non_object_value`; `secure_json_parse_upstream_allows_constructor_property_with_null_value`; `secure_json_parse_upstream_errors_on_constructor_property`; `secure_json_parse_upstream_errors_on_proto_property`; `secure_json_parse_upstream_errors_on_unicode_escaped_proto_property`; `secure_json_parse_upstream_errors_on_fully_unicode_escaped_proto_property`; `secure_json_parse_upstream_errors_on_unicode_escaped_constructor_property`; `secure_json_parse_upstream_errors_on_fully_unicode_escaped_constructor_property`; existing parse/safe-parse wrapper regressions | Mirrors every portable upstream `secure-json-parse.test.ts` case one-to-one for primitive and object parsing, constructor string/null allowance, object-valued constructor rejection, `__proto__` rejection, and unicode-escaped dangerous-key rejection. Exact JavaScript `SyntaxError` identity is JS-runtime-specific; Rust asserts the equivalent secure parse rejection and wraps it through `JsonParseError` for public parse APIs. |
| Provider-utils parse JSON wrappers | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `parse_json_upstream_should_parse_basic_json_without_schema`; `parse_json_upstream_should_parse_json_with_schema_validation`; `parse_json_upstream_should_throw_json_parse_error_for_invalid_json`; `parse_json_upstream_should_throw_type_validation_error_for_schema_validation_failures`; `safe_parse_json_upstream_should_safely_parse_basic_json_without_schema_and_include_raw_value`; `safe_parse_json_upstream_should_preserve_raw_value_even_after_schema_transformation`; `safe_parse_json_upstream_should_handle_failed_parsing_with_error_details`; `safe_parse_json_upstream_should_handle_schema_validation_failures`; `safe_parse_json_upstream_should_handle_nested_objects_and_preserve_raw_values`; `safe_parse_json_upstream_should_handle_arrays_and_preserve_raw_values`; `safe_parse_json_upstream_should_handle_discriminated_unions_in_schema`; `safe_parse_json_upstream_should_handle_nullable_fields_in_schema`; `safe_parse_json_upstream_should_handle_union_types_in_schema`; `is_parsable_json_upstream_should_return_true_for_valid_json`; `is_parsable_json_upstream_should_return_false_for_invalid_json` | Mirrors every portable upstream `parse-json.test.ts` case one-to-one for plain `parseJSON`, schema-validated parsing, invalid JSON errors, schema validation errors, `safeParseJSON` success/error/raw-value contracts, transformation raw-value preservation, nested object and array transformations, discriminated union, nullable, and union validation cases, plus valid and invalid `isParsableJson` checks. Rust uses the package-owned `Schema::with_validator` boundary instead of JavaScript Zod runtime objects. |
| Provider-utils schema wrappers | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `as_schema_upstream_should_create_an_object_schema_when_no_schema_is_provided`; `standard_schema_upstream_should_return_the_json_schema_from_input`; `standard_schema_upstream_should_pass_target_draft_07_to_json_schema_input`; `standard_schema_upstream_should_support_nested_objects`; `standard_schema_upstream_should_support_arrays`; `standard_schema_upstream_should_validate_and_return_value_for_valid_input`; `standard_schema_upstream_should_return_error_for_invalid_input`; `standard_schema_upstream_should_support_transform_in_validation`; `standard_schema_upstream_should_detect_non_zod_standard_schema_by_vendor`; `infer_schema_type_upstream_should_work_with_standard_schema`; existing grouped Rust schema regressions | Mirrors the portable upstream `schema.test.ts` `asSchema` default object case and every portable Standard Schema v1 case for input JSON Schema conversion, draft-07 target propagation, nested object/array additional-properties closure, valid/invalid validation, transform output validation, and non-Zod vendor detection. The Rust generic `StandardSchema<T>` test maps upstream `schema.test-d.ts` `InferSchema<StandardSchema<T>>` type inference. Zod v4 JSON-schema snapshot conversion and Zod transform validation are JavaScript adapter/runtime cases and remain documented rather than counted as Rust parity. |
| Provider-utils Zod JSON-schema adapters | js-only-documented | none; portable schema coverage is in `crates/ai-sdk-provider-utils/src/provider_utils.rs` | Upstream Zod v3 adapter inventory: `to-json-schema/zod3-to-json-schema/parse-def.test.ts` (9 cases), `parsers/array.test.ts` (6), `bigint.test.ts` (4), `branded.test.ts` (1), `catch.test.ts` (1), `date.test.ts` (5), `default.test.ts` (3), `effects.test.ts` (3), `intersection.test.ts` (3), `map.test.ts` (2), `native-enum.test.ts` (6), `nullable.test.ts` (2), `number.test.ts` (6), `object.test.ts` (6), `optional.test.ts` (7), `pipe.test.ts` (3), `promise.test.ts` (1), `readonly.test.ts` (1), `record.test.ts` (6), `set.test.ts` (1), `string.test.ts` (34), `tuple.test.ts` (2), `union.test.ts` (9), `refs.test.ts` (23), and `zod3-to-json-schema.test.ts` (18), for 162 `it` cases; upstream Zod v4 schema snapshots live in `schema.test.ts` plus `__snapshots__/schema.test.ts.snap` | These tests exercise JavaScript Zod v3/v4 runtime objects, private `_def` parser internals, Vitest snapshot text, override/post-process callbacks, fake timers, and the `zod3-to-json-schema` vendored adapter. Rust has no Zod runtime object graph or TypeScript conditional schema adapter to port. Rust-facing portable behavior is explicit JSON Schema and `StandardSchema<T>` conversion/validation, covered by the schema wrapper and JSON Schema rows above. |
| Provider-utils TypeScript type-level inventory | in-progress | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `infer_schema_type_upstream_should_work_with_standard_schema`; all 25 named `content_part_*` Rust counterparts for upstream `types/content-part.test-d.ts`; executable/execute runtime counterparts listed in `Provider-utils executable tool helpers`; broader Rust tool shape tests such as `tool_prepares_upstream_function_tool_shape`, `dynamic_tool_prepares_upstream_function_tool_shape`, `tool_prepares_upstream_provider_defined_tool_shape`, `tool_prepares_upstream_provider_executed_tool_shape`, `tool_needs_approval_options_use_upstream_shape`, `tool_defined_needs_approval_function_resolves_with_input_and_options`, `tool_to_model_output_accepts_untyped_output_without_execute`, `tool_to_model_output_accepts_execute_function_output`, `tool_to_model_output_accepts_output_schema_result_output`, `tool_needs_approval_function_accepts_input_schema_options`, `tool_needs_approval_function_accepts_execute_tool_options`, `tool_needs_approval_function_accepts_context_schema_context`, `dynamic_tool_upstream_should_include_dynamic_tools_in_the_tool_union`, `dynamic_tool_upstream_should_allow_function_style_properties`, `dynamic_tool_upstream_should_reject_provider_only_properties`, `dynamic_tool_upstream_should_create_dynamic_tools_with_the_dynamic_discriminator`, `provider_defined_tool_upstream_should_include_provider_defined_tools_in_the_tool_union`, `provider_defined_tool_upstream_should_require_provider_specific_properties`, `provider_defined_tool_upstream_should_allow_user_execution_or_an_output_schema`, `provider_defined_tool_upstream_rejects_function_only_properties`, `provider_executed_tool_upstream_should_include_provider_executed_tools_in_the_tool_union`, `provider_executed_tool_upstream_should_require_provider_specific_properties`, `provider_executed_tool_upstream_should_allow_deferred_result_support`, `provider_executed_tool_upstream_rejects_function_only_properties`, `function_tool_upstream_should_expose_the_function_tool_discriminator`, `function_tool_upstream_should_include_function_tools_in_the_tool_union`, `function_tool_upstream_should_allow_omitted_and_explicit_function_discriminators`, `function_tool_upstream_should_reject_dynamic_and_provider_only_properties`, `tool_union_upstream_should_expose_all_tool_variants_and_type_discriminators`, `tool_union_upstream_should_narrow_tools_by_type`, `tool_constructor_input_type_upstream_should_infer_input_type_from_zod_input_schema`, `tool_constructor_input_type_upstream_should_preserve_input_type_from_flexible_schema`, `tool_constructor_input_type_upstream_should_infer_input_type_with_optional_default_examples`, `tool_constructor_input_type_upstream_should_infer_input_type_with_refined_schema_examples`, `tool_constructor_context_type_upstream_should_infer_context_type_from_context_schema_in_execute`, `tool_constructor_context_type_upstream_should_infer_context_type_in_input_lifecycle_callbacks`, `tool_constructor_output_type_upstream_should_infer_output_type_from_execute_function`, `tool_constructor_output_type_upstream_should_infer_output_type_from_async_generator_execute_function`, `tool_execution_options_include_execution_metadata_context_abort_signal_and_sandbox`, `sandbox_command_options_include_abort_signal_without_serializing_it`, `tool_execute_function_accepts_input_output_and_execution_options`, and `tool_needs_approval_function_accepts_input_options_and_returns_boolean`; TypeScript-only `HasRequiredKey`/`NeverOptional` and generic tool inference cases documented below | Exact upstream `*.test-d.ts` inventory now tracked: `has-required-key.test-d.ts` (4 cases), `schema.test-d.ts` (1 case), `types/content-part.test-d.ts` (25 cases), `types/executable-tool.test-d.ts` (3 cases), `types/execute-tool.test-d.ts` (2 cases), `types/infer-tool-context.test-d.ts` (5 cases), `types/infer-tool-input.test-d.ts` (1 case), `types/infer-tool-output.test-d.ts` (2 cases), `types/infer-tool-set-context.test-d.ts` (7 cases), `types/never-optional.test-d.ts` (3 cases), `types/tool-execute-function.test-d.ts` (3 cases), `types/tool-needs-approval-function.test-d.ts` (1 case), and `types/tool.test-d.ts` (35 cases), for 92 `it` cases. The `types/content-part.test-d.ts` cases now have named Rust counterparts for tagged and shorthand file/reasoning-file data, tagged-only tool-result file content, legacy file/image result variants, and provider-reference rejection of the reserved `type` key. The `types/tool-execute-function.test-d.ts`, `types/tool-needs-approval-function.test-d.ts`, and portable runtime/API surfaces behind `types/tool.test-d.ts` now have named Rust counterparts for execution metadata, abort signals, sandbox command abort options, execute callback input/output/options, approval callback input/options/result, tool model-output callback options, tool-defined approval callback variants, dynamic/function/provider tool variant-property contracts, and tool constructor input/context/output contracts. `has-required-key.test-d.ts`, `types/never-optional.test-d.ts`, `types/executable-tool.test-d.ts`, `types/execute-tool.test-d.ts`, `types/infer-tool-context.test-d.ts`, `types/infer-tool-input.test-d.ts`, `types/infer-tool-output.test-d.ts`, and `types/infer-tool-set-context.test-d.ts` are explicitly accounted for as TypeScript-only compiler/generic inference rows below. Remaining `types/tool.test-d.ts` compile-only assertions are limited to TypeScript/Zod literal inference, `undefined`/`any` exactness, template-literal provider ids, excess-property rejection, and concrete-output-without-execute/outputSchema assignability, all of which need either compile-test infrastructure or explicit TypeScript-only documentation before this row can be verified. |
| Provider-utils TypeScript conditional helper types | js-only-documented | none | `has-required-key.test-d.ts`: empty object returns false; all-optional keys return false; at least one required key returns true; all required keys return true. `types/never-optional.test-d.ts`: known condition types preserve original properties; `any` condition makes properties optional; `never` condition allows only optional `undefined` properties and rejects original value types. | These 7 upstream cases exercise TypeScript conditional-type mechanics (`any`, `never`, optional property detection, readonly preservation, and compile-time structural assignability). Rust has no package-owned runtime function or public API equivalent for computing TypeScript optional-key predicates, `any`, or `never` assignability. Rust required/optional fields and uninhabited types are enforced directly by the Rust compiler and are not portable provider-utils behavior to test at runtime. |
| Provider-utils TypeScript generic tool inference helpers | js-only-documented | `crates/ai-sdk-provider-utils/src/provider_utils.rs` for runtime tool behavior | `types/executable-tool.test-d.ts`: narrows tools with execute to `ExecutableTool`, narrows executable tool unions that include `undefined`, preserves `undefined` execute for non-executable tools. `types/execute-tool.test-d.ts`: infers `AsyncGenerator` output type for non-streaming tools, infers streamed tool outputs from async generator execute functions. `types/infer-tool-context.test-d.ts`: contextSchema inference, no-context `never`, optional-only context object properties, optional context objects, empty context objects. `types/infer-tool-input.test-d.ts`: inputSchema inference. `types/infer-tool-output.test-d.ts`: output inference from execute function and outputSchema. `types/infer-tool-set-context.test-d.ts`: tool-name context maps, single tool context maps, empty maps for tools without required context, omission of tools without context, optional-only context properties, optional context entries, and empty-context omission. | These 20 upstream cases test TypeScript-only generic inference, conditional mapped types, control-flow narrowing, Zod `z.infer`, literal type preservation, and JavaScript `AsyncGenerator` type signatures. Rust has no TypeScript compiler, `undefined`, `any`, `never`, conditional mapped types, or Zod generic inference layer to port. The portable runtime behavior behind these helpers remains covered by the provider-utils tool rows: `execute_tool_upstream_yields_a_single_final_output_for_non_streaming_tools`, `execute_tool_upstream_yields_streamed_values_as_preliminary_output_and_repeats_the_last_one_as_final`, `is_executable_tool_upstream_returns_true_for_tools_with_an_execute_function`, `is_executable_tool_upstream_returns_false_for_tools_without_an_execute_function`, `is_executable_tool_upstream_returns_false_for_undefined`, `is_executable_tool_upstream_allows_executable_tools_to_be_passed_to_execute_tool_after_narrowing`, `tool_context_schema_is_retained_but_not_sent_to_provider`, `tool_defined_needs_approval_function_resolves_with_input_and_options`, `tool_execute_function_accepts_input_output_and_execution_options`, and `tool_needs_approval_function_accepts_input_options_and_returns_boolean`. |
| Provider-utils tool variant runtime contracts | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `dynamic_tool_upstream_should_include_dynamic_tools_in_the_tool_union`; `dynamic_tool_upstream_should_allow_function_style_properties`; `dynamic_tool_upstream_should_reject_provider_only_properties`; `dynamic_tool_upstream_should_create_dynamic_tools_with_the_dynamic_discriminator`; `provider_defined_tool_upstream_should_include_provider_defined_tools_in_the_tool_union`; `provider_defined_tool_upstream_should_require_provider_specific_properties`; `provider_defined_tool_upstream_should_allow_user_execution_or_an_output_schema`; `provider_defined_tool_upstream_rejects_function_only_properties`; `provider_executed_tool_upstream_should_include_provider_executed_tools_in_the_tool_union`; `provider_executed_tool_upstream_should_require_provider_specific_properties`; `provider_executed_tool_upstream_should_allow_deferred_result_support`; `provider_executed_tool_upstream_rejects_function_only_properties`; `function_tool_upstream_should_expose_the_function_tool_discriminator`; `function_tool_upstream_should_include_function_tools_in_the_tool_union`; `function_tool_upstream_should_allow_omitted_and_explicit_function_discriminators`; `function_tool_upstream_should_reject_dynamic_and_provider_only_properties`; `tool_union_upstream_should_expose_all_tool_variants_and_type_discriminators`; `tool_union_upstream_should_narrow_tools_by_type` | Mirrors the portable runtime/API behavior behind upstream `packages/provider-utils/src/types/tool.test-d.ts` DynamicTool, ProviderDefinedTool, ProviderExecutedTool, FunctionTool, and Tool discriminated-union cases. Rust proves function/dynamic/provider variant identity, provider id/args/provider-executed/deferred-result contracts, dynamic function-style properties, function-tool provider-facing discriminators, and narrowing through Rust variant predicates. Provider tools now ignore function-only builder properties and only provider-executed tools retain deferred-result support, matching the upstream static property exclusions at runtime. TypeScript-only template-literal id validation, excess-property assignability, and generic concrete-output requirements remain tracked in the broader type-level inventory row. |
| Provider-utils tool constructor runtime contracts | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `tool_constructor_input_type_upstream_should_infer_input_type_from_zod_input_schema`; `tool_constructor_input_type_upstream_should_preserve_input_type_from_flexible_schema`; `tool_constructor_input_type_upstream_should_infer_input_type_with_optional_default_examples`; `tool_constructor_input_type_upstream_should_infer_input_type_with_refined_schema_examples`; `tool_constructor_context_type_upstream_should_infer_context_type_from_context_schema_in_execute`; `tool_constructor_context_type_upstream_should_infer_context_type_in_input_lifecycle_callbacks`; `tool_constructor_output_type_upstream_should_infer_output_type_from_execute_function`; `tool_constructor_output_type_upstream_should_infer_output_type_from_async_generator_execute_function` | Mirrors the portable runtime/API behavior behind upstream `packages/provider-utils/src/types/tool.test-d.ts` `tool constructor` input type, context type, and output type cases. Rust proves tool constructor schemas are retained, `FlexibleSchema` normalization preserves validation output, input examples coexist with optional/default and refined-schema shapes, context schemas are available to execute and streamed input lifecycle callbacks, and execute plus streamed-execute callbacks expose final/preliminary outputs through the Rust tool API. TypeScript-only Zod literal inference, `undefined` exactness, and `AsyncGenerator` generic signatures remain compiler-only and tracked in the type-level inventory row. |
| Provider-utils and AI tool input lifecycle callbacks | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs`, `src/generate_text.rs`, `src/stream_text.rs` | `tool_input_lifecycle_callbacks_receive_upstream_execution_options`; `generate_text_invokes_tool_input_available_callback_for_tool_calls`; `stream_text_invokes_tool_input_lifecycle_callbacks_from_stream` | Mirrors the portable runtime behavior behind upstream `packages/provider-utils/src/types/tool.test-d.ts` input lifecycle callback typing plus `packages/ai/src/generate-text/invoke-tool-callbacks-from-stream.test.ts`, `generate-text.test.ts`, and `stream-text.test.ts` callback cases: Rust tools can register `onInputStart`, `onInputDelta`, and `onInputAvailable` callbacks, callback options carry tool call id, prompt messages, runtime context, abort signal, and sandbox where applicable, non-streaming `generate_text` invokes `onInputAvailable`, and `stream_text` invokes start/delta/available callbacks while passing through tool input stream parts. |
| Provider-utils tool model-output and approval callback options | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `tool_to_model_output_accepts_untyped_output_without_execute`; `tool_to_model_output_accepts_execute_function_output`; `tool_to_model_output_accepts_output_schema_result_output`; `tool_needs_approval_function_accepts_input_schema_options`; `tool_needs_approval_function_accepts_execute_tool_options`; `tool_needs_approval_function_accepts_context_schema_context` | Mirrors the portable runtime behavior behind upstream `packages/provider-utils/src/types/tool.test-d.ts` `toModelOutput` and function-form `needsApproval` inference cases. Rust cannot express TypeScript's exact generic output inference, but these package-owned tests prove the equivalent callback contracts: `toModelOutput` receives tool call id, parsed input, and untyped/execute-derived/schema-shaped output values, while function-form `needsApproval` receives parsed input, tool call id, messages, and context including tools with a context schema. |
| Provider-utils blob download | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `download_blob_upstream_should_download_a_blob_successfully`; `download_blob_upstream_should_throw_download_error_on_non_ok_response`; `download_blob_upstream_should_throw_download_error_on_network_error`; `download_blob_upstream_should_rethrow_download_error_without_wrapping`; `download_blob_upstream_should_abort_when_response_exceeds_default_size_limit`; `download_blob_ssrf_upstream_should_reject_private_ipv4_addresses`; `download_blob_ssrf_upstream_should_reject_localhost`; `download_blob_ssrf_upstream_should_reject_non_http_protocols`; `download_blob_ssrf_upstream_should_reject_redirects_to_private_ip_addresses`; `download_blob_ssrf_upstream_should_reject_redirects_to_localhost`; `download_blob_ssrf_upstream_should_allow_redirects_to_safe_urls`; `download_error_upstream_should_create_error_with_status_code_and_text`; `download_error_upstream_should_create_error_with_cause`; `download_error_upstream_should_create_error_with_custom_message`; `download_error_upstream_should_identify_download_error_instances_correctly`; existing grouped Rust download regressions | Mirrors every portable upstream `download-blob.test.ts` case one-to-one for successful blob media type/bytes, non-OK response errors, lower-level network error messages, propagated `DownloadError`s, default size-limit rejection, SSRF rejection for private IPv4, localhost, and non-HTTP URLs, redirected private/localhost URL rejection, safe redirect allowance, and `DownloadError` status, cause-message, custom-message, upstream error-name, and Rust type-identity checks. The upstream `should pass abortSignal to fetch` case is JavaScript Web Fetch runtime behavior; Rust documents this as non-portable because `download_blob` uses an injected transport and omits JavaScript `AbortSignal`. |
| Provider-utils GET API helper | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `get_from_api_upstream_should_successfully_fetch_and_parse_data`; `get_from_api_upstream_should_handle_api_errors`; `get_from_api_upstream_should_handle_network_errors`; `get_from_api_upstream_should_handle_abort_signals`; `get_from_api_upstream_should_remove_undefined_header_entries`; `get_from_api_upstream_should_handle_errors_in_response_handlers`; existing `get_from_api_*` request/transport regressions | Mirrors every portable upstream `get-from-api.test.ts` case one-to-one for successful GET request preparation, provider-utils user-agent suffixes, JSON response parsing and validation, failed status responses becoming `APICallError`, normalized fetch connection failures, abort-signal propagation before transport execution, removal of undefined header entries, and response-handler errors becoming `APICallError`. The upstream `should use default fetch when not provided` case is JavaScript-global-fetch behavior; Rust documents this as non-portable because `get_from_api` requires an injected transport. |
| Provider-utils delay | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `delay_upstream_should_resolve_after_the_specified_delay`; `delay_upstream_should_resolve_immediately_when_delay_in_ms_is_null`; `delay_upstream_should_resolve_immediately_when_delay_in_ms_is_undefined`; `delay_upstream_should_resolve_immediately_when_delay_in_ms_is_0`; `delay_upstream_should_reject_immediately_if_signal_is_already_aborted`; `delay_upstream_should_reject_when_signal_is_aborted_during_delay`; `delay_upstream_should_clean_up_timeout_when_aborted`; `delay_upstream_should_clean_up_event_listener_when_delay_completes_normally`; `delay_upstream_should_work_without_signal_option`; `delay_upstream_should_create_proper_dom_exception_for_abort`; `delay_upstream_should_handle_very_large_delays`; `delay_upstream_should_handle_negative_delays_treated_as_0`; `delay_upstream_should_handle_multiple_delays_simultaneously`; existing grouped Rust delay regressions | Mirrors every portable upstream `delay.test.ts` case one-to-one for deferred completion, immediate `null`/`undefined` resolution, zero and negative delay timer behavior, abort-before-start and abort-during-delay rejection, no-signal completion, upstream-compatible abort error name/message, very large pending delays, and multiple simultaneous delays. Rust uses `DelayOptions` with the package-wide abort signal instead of JavaScript DOM `AbortSignal`; listener/timer cleanup cases are represented by immediate abort completion and post-completion abort no-op semantics because Rust does not expose browser listener/timer counts. |
| Provider-utils executable tool helpers | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `execute_tool_upstream_yields_a_single_final_output_for_non_streaming_tools`; `execute_tool_upstream_yields_streamed_values_as_preliminary_output_and_repeats_the_last_one_as_final`; `is_executable_tool_upstream_returns_true_for_tools_with_an_execute_function`; `is_executable_tool_upstream_returns_false_for_tools_without_an_execute_function`; `is_executable_tool_upstream_returns_false_for_undefined`; `is_executable_tool_upstream_allows_executable_tools_to_be_passed_to_execute_tool_after_narrowing`; existing grouped Rust execute-tool regressions | Mirrors every portable upstream `types/execute-tool.test.ts` and `types/executable-tool.test.ts` runtime case one-to-one for single final output, streamed preliminary outputs with the final output repeated from the last preliminary value, executable-tool detection for present/missing executors and missing tools, and narrowed executable tools flowing into `execute_tool`. Rust exposes streaming tool executors through `Tool::with_execute_outputs`, preserving upstream-shaped preliminary/final records without depending on JavaScript async generators. |
| Provider-utils streaming tool-call tracker | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `streaming_tool_call_tracker_upstream_should_handle_single_tool_call_accumulated_across_multiple_deltas`; `streaming_tool_call_tracker_upstream_should_handle_full_tool_call_in_single_chunk`; `streaming_tool_call_tracker_upstream_should_handle_multiple_concurrent_tool_calls`; `streaming_tool_call_tracker_upstream_should_skip_deltas_for_already_finished_tool_calls`; `streaming_tool_call_tracker_upstream_should_skip_delta_emission_when_arguments_are_null`; `streaming_tool_call_tracker_upstream_should_use_index_fallback_when_index_is_not_provided`; `streaming_tool_call_tracker_upstream_should_throw_when_id_is_missing`; `streaming_tool_call_tracker_upstream_should_throw_when_function_name_is_missing`; `streaming_tool_call_tracker_upstream_should_not_validate_type_with_type_validation_none`; `streaming_tool_call_tracker_upstream_should_validate_type_when_present_with_type_validation_if_present`; `streaming_tool_call_tracker_upstream_should_require_function_type_with_type_validation_required`; `streaming_tool_call_tracker_upstream_should_finalize_unfinished_tool_calls_on_flush`; `streaming_tool_call_tracker_upstream_should_not_refinalize_already_finished_tool_calls`; `streaming_tool_call_tracker_upstream_should_extract_and_include_provider_metadata_in_tool_call_events`; `streaming_tool_call_tracker_upstream_should_include_provider_metadata_for_unfinished_tool_calls_finalized_in_flush`; `streaming_tool_call_tracker_upstream_should_not_include_provider_metadata_when_builder_returns_none`; `streaming_tool_call_tracker_upstream_should_use_custom_generate_id_for_tool_call_ids_when_id_is_missing_in_fallback`; existing grouped Rust tracker regressions | Mirrors every portable upstream `streaming-tool-call-tracker.test.ts` case one-to-one for incremental and single-chunk tool-call assembly, concurrent tool calls, finished-call skipping, null/absent argument delta skipping, fallback index allocation, missing id/name validation, `none`/`if-present`/`required` type-validation modes, flush finalization and idempotence, provider metadata extraction/building/omission, and custom generate-id fallback behavior. Rust returns emitted stream parts instead of enqueuing into a JavaScript `ReadableStreamDefaultController`, which is the package-owned Rust equivalent of the portable controller contract. |
| Provider-utils response size limit reader | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `read_response_with_size_limit_upstream_should_read_response_within_limit_successfully`; `read_response_with_size_limit_upstream_rejects_oversized_content_length_early`; `read_response_with_size_limit_upstream_should_abort_when_streamed_bytes_exceed_limit`; `read_response_with_size_limit_upstream_should_handle_lying_content_length`; `read_response_with_size_limit_upstream_should_handle_empty_body_null`; `read_response_with_size_limit_upstream_should_handle_empty_body_zero_length`; `read_response_with_size_limit_upstream_should_respect_custom_max_bytes`; `read_response_with_size_limit_upstream_should_reject_at_exact_boundary_max_bytes_plus_one`; existing grouped/default-limit regressions | Mirrors every portable upstream `read-response-with-size-limit.test.ts` case one-to-one for successful bounded reads, early `Content-Length` rejection, streamed-byte rejection, lying content-length rejection, null and zero-length body handling, exact custom limit acceptance, and `maxBytes + 1` rejection. The older grouped Rust tests remain additive coverage for default-limit and invalid-header behavior. |
| Provider-utils response handlers | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `response_handler_upstream_json_handler_returns_parsed_value_and_raw_value`; `response_handler_upstream_binary_handler_handles_binary_response_successfully`; `response_handler_upstream_binary_handler_throws_api_call_error_for_null_body`; `response_handler_upstream_status_code_handler_creates_error_with_status_text_and_body`; existing response-handler option/error regressions | Mirrors every portable upstream `response-handler.test.ts` case one-to-one for JSON response value/raw-value extraction, binary response byte handling, empty binary body `ApiCallError`, and status-code error construction with status text, body, URL, status code, and request body values. Existing Rust response-handler tests remain additive coverage for headers, serialization, invalid JSON, validation failure, event-source handlers, and JSON error handlers. |
| Provider-utils fetch error handling | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `handle_fetch_error_upstream_returns_abort_error_as_is`; `handle_fetch_error_upstream_handles_type_error_with_fetch_failed_message`; `handle_fetch_error_upstream_handles_type_error_with_failed_to_fetch_message`; `handle_fetch_error_upstream_handles_connection_refused_error`; `handle_fetch_error_upstream_handles_connection_closed_error`; `handle_fetch_error_upstream_handles_failed_to_open_socket_error`; `handle_fetch_error_upstream_handles_econnreset_error`; `handle_fetch_error_upstream_returns_unknown_errors_as_is`; existing grouped Rust regressions | Mirrors every portable upstream `handle-fetch-error.test.ts` case one-to-one for abort errors passing through, Node and browser fetch `TypeError` connection failures becoming retryable `ApiCallError`s, Bun-style `ConnectionRefused`, `ConnectionClosed`, `FailedToOpenSocket`, and `ECONNRESET` errors becoming retryable `ApiCallError`s, and unknown errors passing through unchanged. Existing Rust tests remain additive coverage for no-cause TypeError behavior and extra network error codes. |
| Provider-utils async iterator stream adapter | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `convert_async_iterator_stream_upstream_calls_return_on_cancel_and_triggers_finally`; `convert_async_iterator_stream_upstream_stops_reads_after_cancel`; `convert_async_iterator_stream_upstream_cancels_without_return_method`; `convert_async_iterator_stream_upstream_ignores_return_errors` | Mirrors every portable upstream `convert-async-iterator-to-readable-stream.test.ts` case one-to-one for cancellation invoking the iterator return hook, no reads after cancellation, clean cancellation when no return hook exists, and ignored return-hook errors. The exact browser `ReadableStream.getReader()` API surface is JavaScript-runtime-specific; Rust exposes the same portable read/cancel contract through a dependency-free async reader. |
| Provider-utils JSON serializability check | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `is_json_serializable_upstream_returns_true_for_null_and_undefined`; `is_json_serializable_upstream_returns_true_for_json_primitives`; `is_json_serializable_upstream_returns_false_for_unsupported_primitives`; `is_json_serializable_upstream_returns_true_for_serializable_arrays`; `is_json_serializable_upstream_returns_false_for_arrays_with_non_serializable_values`; `is_json_serializable_upstream_returns_true_for_serializable_plain_objects`; `is_json_serializable_upstream_returns_false_for_plain_objects_with_non_serializable_values`; `is_json_serializable_upstream_returns_false_for_non_plain_objects` | Mirrors every portable upstream `is-json-serializable.test.ts` case one-to-one for null/undefined, JSON primitives, unsupported JavaScript primitives, arrays, arrays containing non-serializable values, plain objects, nested non-serializable object values, and non-plain object instances. Rust models JavaScript-only unsupported runtime values through `JsonSerializableValue` so the upstream boundary remains explicit. |
| Provider-utils serialized model options | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `serialize_model_options_upstream_returns_model_id_and_serializable_config`; `serialize_model_options_upstream_resolves_headers_functions_but_filters_out_other_functions`; `serialize_model_options_upstream_filters_out_objects_containing_functions`; `serialize_model_options_upstream_keeps_arrays_of_primitives`; `serialize_model_options_upstream_filters_out_class_instances`; existing Rust boundary regressions | Mirrors every portable upstream `serialize-model-options.test.ts` case one-to-one for retaining `modelId` and serializable config, resolving the header-function boundary through already-resolved header values, omitting other function-valued entries through `None`, filtering nested objects/class-like values represented as non-serializable entries, and preserving primitive arrays. The two upstream Promise-returning header cases are JavaScript-only because Rust callers cannot pass JavaScript functions or promises into the typed `serde_json::Value`/`Option` serialization boundary. |
| Provider-utils JSON Schema additional properties | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `add_additional_properties_to_json_schema_upstream_adds_to_objects_recursively`; `add_additional_properties_to_json_schema_upstream_adds_to_objects_inside_arrays`; `add_additional_properties_to_json_schema_upstream_adds_when_union_includes_object`; `add_additional_properties_to_json_schema_upstream_adds_to_objects_inside_any_of`; `add_additional_properties_to_json_schema_upstream_adds_to_objects_inside_all_of`; `add_additional_properties_to_json_schema_upstream_adds_to_objects_inside_one_of`; `add_additional_properties_to_json_schema_upstream_adds_to_objects_inside_definitions`; `add_additional_properties_to_json_schema_upstream_overwrites_existing_flags`; `add_additional_properties_to_json_schema_upstream_leaves_non_object_schemas_unchanged`; `add_additional_properties_to_json_schema_visits_tuple_items` | Mirrors every upstream `add-additional-properties-to-json-schema.test.ts` case one-to-one for recursive object closure, array item object closure, union-object closure, `anyOf`/`allOf`/`oneOf` traversal, definitions traversal, existing flag overwrite, and non-object passthrough. The tuple-items case is extra Rust coverage for the local schema walker. |
| Provider-utils ID generation | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `create_id_generator_upstream_generates_id_with_correct_length`; `create_id_generator_upstream_generates_id_with_correct_default_length`; `create_id_generator_upstream_throws_error_when_separator_is_part_of_alphabet`; `generate_id_upstream_generates_unique_ids`; existing grouped regressions | Mirrors upstream `generate-id.test.ts` one-to-one for configured ID length, default ID length, invalid separator validation, and unique `generateId` output. The older grouped Rust tests remain as additive coverage for prefix/separator formatting, alphabet constraints, serde shape, and default alphabet membership. |
| Provider-utils delayed promise | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `delayed_promise_upstream_resolves_when_accessed_after_resolution`; `delayed_promise_upstream_rejects_when_accessed_after_rejection`; `delayed_promise_upstream_resolves_when_accessed_before_resolution`; `delayed_promise_upstream_rejects_when_accessed_before_rejection`; `delayed_promise_upstream_maintains_resolved_state_after_multiple_accesses`; `delayed_promise_upstream_maintains_rejected_state_after_multiple_accesses`; `delayed_promise_upstream_blocks_until_resolved_when_accessed_before_resolution`; `delayed_promise_upstream_blocks_until_rejected_when_accessed_before_rejection`; `delayed_promise_upstream_resolves_all_pending_promises_when_resolved_after_access`; existing grouped regressions | Mirrors upstream `delayed-promise.test.ts` one-to-one for resolve/reject before and after promise access, stable resolved/rejected state across repeated access, pending futures remaining blocked until settlement, and resolving all pending promise futures. The older grouped Rust tests remain additive coverage for initial pending state and settlement-order behavior. |
| Provider-utils JSON instruction injection | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `inject_json_instruction_upstream_basic_case_with_prompt_and_schema`; `inject_json_instruction_upstream_only_prompt_no_schema`; `inject_json_instruction_upstream_only_schema_no_prompt`; `inject_json_instruction_upstream_no_prompt_no_schema`; `inject_json_instruction_upstream_custom_schema_prefix_and_suffix`; `inject_json_instruction_upstream_empty_string_prompt`; `inject_json_instruction_upstream_empty_object_schema`; `inject_json_instruction_upstream_complex_nested_schema`; `inject_json_instruction_upstream_schema_with_special_characters`; `inject_json_instruction_upstream_very_long_prompt_and_schema`; `inject_json_instruction_upstream_undefined_values_for_optional_parameters`; `inject_json_instruction_into_messages_upstream_basic_case_with_prompt_and_schema`; `inject_json_instruction_into_messages_upstream_does_not_mutate_input_messages`; `inject_json_instruction_into_messages_upstream_empty_messages_array`; `inject_json_instruction_into_messages_upstream_messages_without_initial_system_message`; `inject_json_instruction_into_messages_upstream_system_message_with_empty_content`; `inject_json_instruction_into_messages_upstream_preserves_all_non_system_messages`; `inject_json_instruction_into_messages_upstream_case_with_no_schema`; `inject_json_instruction_into_messages_upstream_custom_schema_prefix_and_suffix`; additional Rust-shape regression tests for provider options and existing schema helpers | Mirrors every portable upstream `inject-json-instruction.test.ts` case one-to-one, including prompt/schema/default suffix variants, custom prefix/suffix, empty schema, complex nested schema, special-character schema, long schema, message insertion/update behavior, no-mutation proof, empty message arrays, missing/empty system messages, no-schema message injection, and non-system message preservation. The upstream explicit `null as any` optional-parameter case is documented as JavaScript-only because the Rust API uses typed `Option` values and does not expose a separate explicit-null state. |
| Provider-utils media type extension | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `media_type_to_extension_maps_audio_mpeg_to_mp3`; `media_type_to_extension_maps_audio_mp3_to_mp3`; `media_type_to_extension_maps_audio_wav_to_wav`; `media_type_to_extension_maps_audio_x_wav_to_wav`; `media_type_to_extension_maps_audio_webm_to_webm`; `media_type_to_extension_maps_audio_ogg_to_ogg`; `media_type_to_extension_maps_audio_opus_to_ogg`; `media_type_to_extension_maps_audio_mp4_to_m4a`; `media_type_to_extension_maps_audio_x_m4a_to_m4a`; `media_type_to_extension_maps_audio_flac_to_flac`; `media_type_to_extension_maps_audio_aac_to_aac`; `media_type_to_extension_maps_uppercase_audio_mpeg_to_mp3`; `media_type_to_extension_maps_uppercase_audio_mp3_to_mp3`; `media_type_to_extension_maps_invalid_media_type_to_empty_string` | Mirrors upstream `media-type-to-extension.test.ts` one-to-one across every `it.each` row for common audio types, uppercase handling, and invalid media types. The older grouped Rust regression remains as additive coverage only. |
| Provider-utils array and filename helpers | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `as_array_upstream_returns_empty_array_for_undefined`; `as_array_upstream_wraps_single_value_in_array`; `as_array_upstream_returns_array_value_unchanged`; `strip_file_extension_upstream_strips_extension_from_filename`; `strip_file_extension_upstream_returns_input_when_there_is_no_extension`; `strip_file_extension_upstream_strips_all_extension_segments_for_multi_dot_filenames`; `strip_file_extension_upstream_strips_a_trailing_dot` | Mirrors upstream `as-array.test.ts` and `strip-file-extension.test.ts` one-to-one for undefined input, single-value wrapping, array passthrough, single extension stripping, no-extension passthrough, multi-dot filename stripping, and trailing-dot stripping. |
| Provider-utils media detection and full media resolution | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `detect_media_type_upstream_*`; `get_top_level_media_type_upstream_*`; `is_full_media_type_upstream_*`; `resolve_full_media_type_returns_full_media_type_as_is`; `resolve_full_media_type_detects_inline_byte_subtype`; `resolve_full_media_type_treats_wildcard_as_top_level`; `resolve_full_media_type_detects_application_pdf`; `resolve_full_media_type_rejects_non_inline_byte_data`; `resolve_full_media_type_rejects_unrecognized_inline_bytes`; `resolve_full_media_type_rejects_unsupported_top_level_segment`; `resolve_full_media_type_accepts_base64_string_data`; existing grouped regressions | Mirrors upstream `detect-media-type.test.ts` and `resolve-full-media-type.test.ts` across every portable media signature, base64 signature, negative RIFF/WebP cross-detection, unknown/empty/short/invalid data case, top-level segment helper case, full-media-type helper case, top-level-specific detection case, automatic detection without top-level type, full media pass-through, inline-byte detection, wildcard detection, application PDF detection, non-inline rejection, unrecognized-byte rejection, unsupported segment rejection, and base64 data resolution. The older grouped Rust tests remain additive coverage only. |
| Provider-utils form-data and image file URI conversion | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `convert_to_form_data_upstream_*`; `convert_image_model_file_to_data_uri_upstream_*`; existing grouped regressions | Mirrors upstream `convert-to-form-data.test.ts` and `convert-image-model-file-to-data-uri.test.ts` one-to-one for string, number-as-string, binary/Blob, nullish skipping, single/multi/empty arrays, array-bracket disabling, typed input object shape, complex mixed inputs, URL and query URL passthrough, base64 file data, media-type variation, raw byte encoding, empty raw bytes, and raw byte media-type variation. JavaScript `Blob` instances are represented as dependency-free Rust byte form values. |
| Provider-utils header normalization | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `normalize_headers_upstream_returns_empty_object_for_undefined`; `normalize_headers_upstream_converts_headers_instance_to_record`; `normalize_headers_upstream_converts_tuple_array`; `normalize_headers_upstream_converts_plain_record_and_filters_nullish_values`; `normalize_headers_upstream_handles_empty_headers_instance`; `normalize_headers_upstream_converts_uppercase_keys_to_lowercase`; existing grouped regressions | Mirrors upstream `normalize-headers.test.ts` one-to-one for missing input, Headers instance conversion, tuple arrays, plain records with nullish filtering, empty Headers, and uppercase key normalization. The browser `Headers` object is represented by Rust iterable header pairs because this crate has no browser runtime type. |
| Provider-utils reasoning provider mapping | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `map_reasoning_to_provider_effort_upstream_returns_mapped_value_with_no_warning_for_direct_match`; `map_reasoning_to_provider_effort_upstream_returns_mapped_value_with_compatibility_warning_for_renamed_match`; `map_reasoning_to_provider_effort_upstream_returns_mapped_value_with_compatibility_warning_for_xhigh`; `map_reasoning_to_provider_effort_upstream_returns_undefined_with_unsupported_warning_for_key_missing_from_effort_map`; `is_custom_reasoning_upstream_returns_false_for_undefined`; `is_custom_reasoning_upstream_returns_false_for_provider_default`; `is_custom_reasoning_upstream_returns_true_for_none`; `is_custom_reasoning_upstream_returns_true_for_all_reasoning_levels`; `map_reasoning_to_provider_budget_upstream_returns_correct_budget_for_known_key`; `map_reasoning_to_provider_budget_upstream_caps_result_at_max_reasoning_budget`; `map_reasoning_to_provider_budget_upstream_floors_result_at_default_min_reasoning_budget`; `map_reasoning_to_provider_budget_upstream_respects_custom_min_reasoning_budget`; `map_reasoning_to_provider_budget_upstream_respects_custom_budget_percentages`; `map_reasoning_to_provider_budget_upstream_returns_undefined_with_unsupported_warning_for_key_missing_from_custom_budget_percentages`; existing grouped regressions | Mirrors upstream `map-reasoning-to-provider.test.ts` one-to-one for effort mapping, compatibility and unsupported warnings, custom-reasoning detection, default and custom budget percentages, min/max budget clamping, and unsupported custom budget rows. The older grouped Rust tests remain as additive coverage only. |
| Provider-utils resolvable values | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `resolve_upstream_should_resolve_raw_values`; `resolve_upstream_should_resolve_raw_objects`; `resolve_upstream_should_resolve_promises`; `resolve_upstream_should_resolve_rejected_promises`; `resolve_upstream_should_resolve_synchronous_functions`; `resolve_upstream_should_resolve_synchronous_functions_returning_objects`; `resolve_upstream_should_resolve_async_functions`; `resolve_upstream_should_resolve_async_functions_returning_promises`; `resolve_upstream_should_handle_async_function_rejections`; `resolve_upstream_should_handle_null`; `resolve_upstream_should_handle_undefined`; `resolve_upstream_should_resolve_nested_objects`; `resolve_headers_upstream_should_resolve_header_objects`; `resolve_headers_upstream_should_resolve_header_functions`; `resolve_headers_upstream_should_resolve_async_header_functions`; `resolve_headers_upstream_should_resolve_header_promises`; `resolve_headers_upstream_reinvokes_async_header_function_each_time`; `resolve_upstream_should_maintain_type_information`; existing grouped regressions | Mirrors upstream `resolve.test.ts` one-to-one for raw values, raw objects, promises/futures, rejected promises as `Result` values, synchronous and async function producers, null and undefined equivalents, nested objects, header objects/functions/futures, repeat async header producer calls, and type preservation. The older grouped Rust tests remain as additive coverage only. |
| Provider-utils tool-name mapping | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `create_tool_name_mapping_upstream_should_create_mappings_for_provider_defined_tools`; `create_tool_name_mapping_upstream_should_ignore_function_tools`; `create_tool_name_mapping_upstream_should_return_input_when_tool_not_in_provider_tool_names`; `create_tool_name_mapping_upstream_should_return_input_when_mapping_does_not_exist`; `create_tool_name_mapping_upstream_should_handle_empty_tools_array`; `create_tool_name_mapping_upstream_should_handle_mixed_function_and_provider_defined_tools`; existing grouped regressions | Mirrors upstream `create-tool-name-mapping.test.ts` one-to-one for provider-defined tool mapping, function-tool passthrough, missing provider-name entries, missing lookup entries, empty tool lists, and mixed function/provider tool lists. The older grouped Rust tests remain as additive coverage only. |
| Provider-utils user-agent suffixing | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `with_user_agent_suffix_upstream_creates_new_user_agent_header`; `with_user_agent_suffix_upstream_appends_suffix_parts_to_existing_user_agent_header`; `with_user_agent_suffix_upstream_removes_missing_header_entries`; `with_user_agent_suffix_upstream_preserves_headers_instance_entries`; `with_user_agent_suffix_upstream_handles_array_header_entries`; existing grouped regressions | Mirrors upstream `with-user-agent-suffix.test.ts` one-to-one for creating and appending user-agent values, filtering missing header entries, preserving browser `Headers`-style entries at the iterable header boundary, and handling array header entries. The older grouped Rust tests remain as additive coverage only. |
| Provider-utils provider-reference detection | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `is_provider_reference_upstream_returns_true_for_plain_record_of_provider_ids`; `is_provider_reference_upstream_returns_true_for_record_with_single_file_id_like_key`; `is_provider_reference_upstream_returns_false_for_object_carrying_type_property`; `is_provider_reference_upstream_returns_false_for_tagged_data_object`; `is_provider_reference_upstream_returns_false_for_uint8_array_json_boundary`; `is_provider_reference_upstream_returns_false_for_null`; `is_provider_reference_upstream_returns_false_for_string_primitive`; `is_provider_reference_upstream_returns_false_for_number_primitive`; existing grouped regressions | Mirrors every portable upstream `is-provider-reference.test.ts` case one-to-one for provider-id records, fileId-like records, tagged reference/data objects, Uint8Array-like JSON arrays, null, string, and number inputs. The upstream `URL` instance case is JavaScript-object-prototype-specific and non-portable at the Rust `serde_json::Value` boundary. |
| Provider-utils provider-reference resolution | verified | `crates/ai-sdk-provider-utils/src/provider_utils.rs` | `resolve_provider_reference_upstream_returns_identifier_when_provider_key_exists`; `resolve_provider_reference_upstream_returns_correct_identifier_for_different_provider`; `resolve_provider_reference_upstream_throws_when_no_entry_exists_for_provider`; `resolve_provider_reference_upstream_throws_when_reference_is_empty`; `resolve_provider_reference_upstream_works_with_single_provider_reference`; existing grouped regressions | Mirrors upstream `resolve-provider-reference.test.ts` one-to-one for provider-specific lookup, alternate provider lookup, missing-provider errors including retained provider/reference context, empty references, and single-provider references. The older grouped Rust tests remain as additive coverage only. |
| Add tool input examples middleware | verified | `src/language_model_middleware.rs` | `add_tool_input_examples_middleware_appends_examples_and_removes_them_by_default`; `add_tool_input_examples_middleware_supports_custom_options`; `add_tool_input_examples_middleware_handles_tool_without_existing_description`; `add_tool_input_examples_middleware_uses_default_json_stringify_format`; `add_tool_input_examples_middleware_passes_through_tools_without_input_examples`; `add_tool_input_examples_middleware_passes_through_tools_with_empty_input_examples`; `add_tool_input_examples_middleware_passes_through_provider_tools_unchanged`; `add_tool_input_examples_middleware_handles_multiple_tools_with_mixed_examples`; `add_tool_input_examples_middleware_handles_empty_tools_array`; `add_tool_input_examples_middleware_handles_undefined_tools` | Mirrors upstream `middleware/add-tool-input-examples-middleware.ts`: formats function tool `inputExamples` into descriptions, handles missing descriptions, defaults to the `Input Examples:` prefix and JSON stringify formatting, supports custom formatters, removes structured examples by default, can retain examples with `remove: false`, leaves function/provider tools without examples unchanged, and preserves empty or absent tool lists. |
| Extract JSON middleware | verified | `src/language_model_middleware.rs` | `extract_json_middleware_wrap_generate_should_strip_markdown_json_fence_from_text_content`; `extract_json_middleware_wrap_generate_should_strip_markdown_fence_without_json_tag`; `extract_json_middleware_wrap_generate_should_leave_text_without_fences_unchanged`; `extract_json_middleware_wrap_generate_should_use_custom_transform_function_when_provided`; `extract_json_middleware_wrap_generate_should_preserve_non_text_content_parts`; `extract_json_middleware_wrap_stream_should_strip_markdown_json_fence_from_streamed_text`; `extract_json_middleware_wrap_stream_should_strip_markdown_fence_without_json_tag`; `extract_json_middleware_wrap_stream_should_leave_text_without_fences_unchanged_in_stream`; `extract_json_middleware_wrap_stream_should_handle_fence_split_across_multiple_deltas`; `extract_json_middleware_wrap_stream_should_handle_content_that_starts_with_backtick_but_is_not_a_fence`; `extract_json_middleware_wrap_stream_should_pass_through_non_text_chunks_unchanged`; `extract_json_middleware_wrap_stream_should_handle_multiple_text_blocks_with_different_ids`; `extract_json_middleware_wrap_stream_should_handle_text_delta_without_prior_text_start`; `extract_json_middleware_wrap_stream_should_emit_text_start_when_stream_ends_while_still_in_prefix_phase`; `extract_json_middleware_wrap_stream_should_apply_custom_transform_to_streamed_content`; `extract_json_middleware_wrap_stream_should_handle_large_content_exceeding_suffix_buffer`; `extract_json_middleware_wrap_stream_should_handle_content_arriving_character_by_character`; `extract_json_middleware_wrap_stream_should_handle_fence_with_extra_whitespace`; `extract_json_middleware_wrap_stream_should_verify_stream_output_matches_expected_structure`; `extract_json_middleware_wrap_stream_should_handle_empty_content_between_fences`; `extract_json_middleware_wrap_stream_should_handle_content_starting_without_backtick_quickly_switching_to_streaming` | Mirrors current upstream `middleware/extract-json-middleware.test.ts` portable cases for non-streaming text parts and this crate's deterministic `Vec<LanguageModelStreamPart>` stream boundary, including split fences, non-fence backticks, non-text chunks, multiple text IDs, missing text-start edge, empty fenced content, custom transforms, large/character-by-character content, and closing fences with extra whitespace. Browser `ReadableStream` mechanics are represented as collected Rust stream transformation. |
| Extract reasoning middleware | verified | `src/language_model_middleware.rs` | `extract_reasoning_middleware_wrap_generate_should_extract_reasoning_from_think_tags`; `extract_reasoning_middleware_wrap_generate_should_extract_reasoning_from_think_tags_when_there_is_no_text`; `extract_reasoning_middleware_wrap_generate_should_extract_reasoning_from_multiple_think_tags`; `extract_reasoning_middleware_wrap_generate_should_prepend_think_tag_iff_start_with_reasoning_is_true`; `extract_reasoning_middleware_wrap_generate_should_preserve_reasoning_property_even_when_rest_contains_other_properties`; `extract_reasoning_middleware_wrap_stream_should_extract_reasoning_from_split_think_tags`; `extract_reasoning_middleware_wrap_stream_should_extract_reasoning_from_single_chunk_with_multiple_think_tags`; `extract_reasoning_middleware_wrap_stream_should_extract_reasoning_from_think_when_there_is_no_text`; `extract_reasoning_middleware_wrap_stream_should_prepend_think_tag_if_start_with_reasoning_is_true`; `extract_reasoning_middleware_wrap_stream_should_keep_original_text_when_think_tag_is_not_present`; `extract_reasoning_middleware_wrap_stream_should_handle_empty_think_tags_without_crashing` | Mirrors current upstream `middleware/extract-reasoning-middleware.test.ts` portable cases for non-streaming text parts and this crate's deterministic `Vec<LanguageModelStreamPart>` stream boundary, including split tags, delayed text starts, multiple reasoning blocks, separators, no-text reasoning, start-with-reasoning true/false behavior, original-text passthrough, and empty reasoning tags. |
| Simulate streaming middleware | verified | `src/language_model_middleware.rs` | `simulate_streaming_middleware_should_simulate_streaming_with_text_response`; `simulate_streaming_middleware_should_simulate_streaming_with_reasoning_as_string`; `simulate_streaming_middleware_should_simulate_streaming_with_reasoning_as_array_of_text_objects`; `simulate_streaming_middleware_should_simulate_streaming_with_reasoning_as_array_of_mixed_objects`; `simulate_streaming_middleware_should_simulate_streaming_with_tool_calls`; `simulate_streaming_middleware_should_preserve_additional_metadata_in_the_response`; `simulate_streaming_middleware_should_handle_empty_text_response`; `simulate_streaming_middleware_should_pass_through_warnings_from_the_model`; existing grouped regression | Mirrors current upstream `middleware/simulate-streaming-middleware.test.ts` portable cases one-to-one for text responses, reasoning parts with and without provider metadata, mixed reasoning/text order, tool calls, call-level provider metadata, empty text, and warnings. Rust models use the crate's deterministic `Vec<LanguageModelStreamPart>` stream boundary, so upstream browser `ReadableStream` collection is represented by direct stream-part assertions. |
| Prompt standardization, model-message conversion, request options | in-progress | `src/prompt.rs`, `src/language_model.rs`, `src/chat_transport.rs` | Prompt conversion and call-option tests; request timeout helper tests; language model call setting preparation tests; tool-choice preparation tests; `convert_ui_messages_maps_simple_system_message`; `convert_ui_messages_maps_simple_user_message`; `convert_ui_messages_maps_custom_assistant_part`; `convert_ui_messages_maps_simple_assistant_text_message`; `convert_ui_messages_maps_assistant_reasoning_parts`; `convert_ui_messages_maps_system_provider_metadata`; `convert_ui_messages_merges_system_provider_metadata_from_text_parts`; `convert_ui_messages_maps_system_anthropic_cache_control_metadata`; `convert_ui_messages_maps_user_text_provider_metadata`; `convert_ui_messages_maps_user_file_provider_metadata`; `convert_ui_messages_maps_assistant_text_provider_metadata`; `convert_ui_messages_maps_assistant_file_provider_metadata`; `convert_ui_messages_maps_user_file_url_part`; `convert_ui_messages_includes_user_file_filename`; `convert_ui_messages_maps_user_file_provider_reference`; `convert_ui_messages_omits_user_file_filename_when_absent`; `convert_ui_messages_maps_assistant_file_url_part`; `convert_ui_messages_includes_assistant_file_filename`; `convert_ui_messages_maps_assistant_file_provider_reference`; `convert_ui_messages_maps_static_tool_output_available_to_assistant_and_tool_messages`; `convert_ui_messages_maps_tool_output_error_raw_input_to_error_text`; `convert_ui_messages_maps_dynamic_tool_output_available_tool_name`; `convert_ui_messages_preserves_step_start_blocks_as_assistant_tool_pairs`; `convert_ui_messages_places_provider_executed_tool_result_in_assistant`; `convert_ui_messages_maps_provider_executed_tool_output_available`; `convert_ui_messages_maps_provider_executed_tool_output_error`; `convert_ui_messages_propagates_provider_metadata_to_provider_executed_tool_result`; `convert_ui_messages_prefers_result_provider_metadata_for_provider_executed_tool_result`; `convert_ui_messages_maps_denied_approval_response_to_execution_denied_result`; `convert_ui_messages_skips_unconverted_data_parts`; `convert_ui_messages_maps_file_provider_reference_and_metadata_parts` | Many prompt parts are covered; request timeout helpers and high-level language model call setting preparation now have named counterparts for upstream `prepare-language-model-call-options.test.ts`; `prepareToolChoice` now has named Rust counterparts for default auto, none, tool object, auto, and required cases; UI-to-model conversion now includes simple system/user/assistant messages, custom assistant parts, assistant reasoning parts, system/user/assistant text and file provider metadata, merged system provider metadata, Anthropic cache-control metadata, user/assistant file URL conversion, filename omission/inclusion, provider-reference file data, assistant static/dynamic tool history, output-error raw input, step-start block ordering, provider-executed tool-result placement, provider-executed output errors, provider metadata propagation to provider-executed results, result-provider-metadata precedence, denied approval responses, skipped unconverted data UI parts, file/provider-reference mapping, and custom/reasoning provider metadata. Broader approval-state edge coverage and remaining agent-specific call-option schema/type-level parity remain unported. Legacy v2/v3 adapter cases are documented separately as JavaScript package compatibility. |
| UI-to-model assistant tool output and conversation splitting | verified | `src/chat_transport.rs` | `convert_ui_messages_maps_tool_output_available_with_provider_metadata`; `convert_ui_messages_maps_tool_output_error_raw_input_to_error_text`; `convert_ui_messages_maps_tool_output_error_input_to_error_text`; `convert_ui_messages_maps_tool_invocation_multi_part_response`; `convert_ui_messages_maps_empty_tool_invocation_conversation`; `convert_ui_messages_maps_multiple_messages_conversation`; `convert_ui_messages_maps_multiple_tool_invocations_with_steps`; `convert_ui_messages_maps_tool_invocations_mixed_with_text`; `convert_ui_messages_maps_multiple_tool_invocations_with_trailing_user_message` | Mirrors the first upstream `convert-to-model-messages.test.ts` assistant tool-output and conversation-shaping cases one-to-one: local tool output with provider metadata, output-error with `rawInput`, output-error with ordinary `input`, snapshot-equivalent screenshot tool output, assistant/user conversation with no tools, multiple plain messages, `step-start` splitting for multiple tool calls/results, mixed text/tool invocation steps, and trailing user messages after split tool history. |
| UI-to-model incomplete tool filtering | verified | `src/chat_transport.rs` | `convert_ui_messages_can_ignore_incomplete_tool_calls` | Adds `ConvertUiMessagesToModelMessagesOptions::ignore_incomplete_tool_calls` and mirrors upstream `ignoreIncompleteToolCalls: true` by dropping `input-streaming` and `input-available` static/dynamic tool parts before model-message conversion while preserving completed tool history, following assistant text, and trailing user messages. |
| UI-to-model dynamic tool conversion | verified | `src/chat_transport.rs` | `convert_ui_messages_maps_dynamic_tool_output_available_tool_name`; `convert_ui_messages_maps_dynamic_tool_with_trailing_user_message`; `convert_ui_messages_maps_provider_executed_dynamic_tool_with_trailing_user_message` | Mirrors upstream `convert-to-model-messages.test.ts` dynamic tool conversion for ordinary dynamic tool results, trailing user message preservation under `ignoreIncompleteToolCalls`, and provider-executed dynamic tool result placement with provider metadata copied to both the tool call and in-assistant tool result. |
| UI-to-model approval response conversion | verified | `src/chat_transport.rs`, `crates/ai-sdk-provider/src/language_model.rs` | `convert_ui_messages_maps_approved_static_tool_approval_response`; `convert_ui_messages_maps_approved_dynamic_tool_approval_response`; `convert_ui_messages_preserves_automatic_approval_metadata_for_tool_result`; `convert_ui_messages_marks_provider_executed_denied_approval_response`; `convert_ui_messages_maps_denied_static_tool_approval_with_follow_up_text`; `convert_ui_messages_maps_denied_dynamic_tool_approval_with_follow_up_text`; `convert_ui_messages_maps_static_tool_output_denied`; `convert_ui_messages_maps_dynamic_tool_output_denied`; `convert_ui_messages_maps_approved_tool_result_with_follow_up_text`; `convert_ui_messages_maps_approved_tool_error_with_follow_up_text`; provider contract coverage in `tool_message_serializes_tool_result_and_approval_response_parts` | Mirrors upstream `packages/ai/src/ui/convert-to-model-messages.test.ts` approval-response matrix one-to-one for approved static/dynamic tools, automatic approval metadata on approved tool results, provider-executed denied approval responses, denied static/dynamic approval follow-up text, static/dynamic `output-denied`, and approved tool result/error follow-up text. The matching Rust prompt part preserves upstream `providerExecuted` on `tool-approval-response`. |
| UI-to-model data part conversion | verified | `src/chat_transport.rs` | `convert_ui_messages_skips_unconverted_data_parts`; `convert_ui_messages_converts_user_data_url_to_text_with_converter`; `convert_ui_messages_skips_user_data_parts_when_no_converter_provided`; `convert_ui_messages_selectively_converts_user_data_parts`; `convert_ui_messages_converts_user_data_parts_to_file_with_converter`; `convert_ui_messages_converts_multiple_user_data_part_types`; `convert_ui_messages_handles_user_message_without_data_parts_with_converter`; `convert_ui_messages_preserves_user_data_part_order_with_converter`; `convert_ui_messages_converts_assistant_data_url_to_text_with_converter`; `convert_ui_messages_skips_assistant_data_parts_when_no_converter_provided`; `convert_ui_messages_selectively_converts_assistant_data_parts`; `convert_ui_messages_converts_assistant_data_parts_to_file_with_converter`; `convert_ui_messages_converts_multiple_assistant_data_part_types`; `convert_ui_messages_handles_assistant_message_without_data_parts_with_converter`; `convert_ui_messages_preserves_assistant_data_part_order_with_converter` | Adds a `convert_data_part` Rust conversion hook for UI data parts and maps every portable upstream `data part conversion` test in `convert-to-model-messages.test.ts` one-to-one for user and assistant messages: data URL to text, no-converter skips, selective conversion, file conversion, multiple data-type conversion with skipped notes, no-data-message passthrough, and part-order preservation. Rust file data uses the typed `FileData::Data` JSON shape while preserving the same base64 payload, media type, and filename behavior. |
| Model resolution | verified | `src/resolve_model.rs` | `resolve_language_model_should_return_it_as_is`; `resolve_language_model_should_return_a_gateway_language_model`; `resolve_language_model_should_return_a_language_model_from_the_default_provider`; `resolve_embedding_model_should_return_it_as_is`; `resolve_embedding_model_should_return_a_gateway_embedding_model`; `resolve_embedding_model_should_return_an_embedding_model_from_the_default_provider`; `resolve_image_model_should_return_it_as_is`; `resolve_image_model_should_return_a_gateway_image_model`; `resolve_image_model_should_return_an_image_model_from_the_default_provider`; `resolve_video_model_should_return_it_as_is`; `resolve_video_model_should_return_a_gateway_video_model_converted_to_v4`; `resolve_video_model_should_return_a_video_model_from_the_default_provider`; `resolve_reranking_model_should_return_it_as_is`; `resolve_reranking_model_should_return_a_gateway_reranking_model_converted_to_v4`; `resolve_reranking_model_should_return_a_reranking_model_from_the_default_provider`; `resolve_reranking_model_should_report_missing_default_provider_support_as_no_such_model`; `resolve_video_model_should_report_missing_default_provider_support_as_no_such_model`; `resolve_transcription_model_supports_the_upstream_resolve_function_surface`; `resolve_speech_model_supports_the_upstream_resolve_function_surface`; `resolved_model_into_owned_clones_direct_models_and_moves_provider_models` | Adds Rust `ModelSource` and `ResolvedModel` helpers for upstream `model/resolve-model.ts`'s portable current-provider boundary. Direct current-version model inputs preserve borrowed identity, string model ids resolve through an explicit provider, and `GatewayProvider` covers the upstream no-global-default Gateway fallback for language, embedding, image, video, and reranking models. Explicit `MockProvider` coverage mirrors the upstream global default-provider cases without introducing process-global mutable state. Rust optional provider support for video/reranking is represented by trait bounds plus typed `NoSuchModelError` values for missing registrations. Legacy v2/v3 adapters, unsupported-version throw tests, JavaScript prototype preservation, and mutable `globalThis.AI_SDK_DEFAULT_PROVIDER` are JavaScript runtime/object boundaries documented in the compatibility row below. |
| Provider-v2/v3 compatibility adapters | js-only-documented | none | This row | Upstream `packages/ai/src/model/as-embedding-model-v3.test.ts` (18 cases), `as-embedding-model-v4.test.ts` (8), `as-image-model-v3.test.ts` (17), `as-image-model-v4.test.ts` (7), `as-language-model-v3.test.ts` (19), `as-language-model-v4.test.ts` (9), `as-provider-v4.test.ts` (7), `as-reranking-model-v4.test.ts` (6), `as-speech-model-v3.test.ts` (16), `as-speech-model-v4.test.ts` (7), `as-transcription-model-v3.test.ts` (20), `as-transcription-model-v4.test.ts` (7), and `as-video-model-v4.test.ts` (6), for 147 `it` cases. These helpers adapt legacy JavaScript provider object versions (`specificationVersion: "v2"` / `"v3"`) into the current JavaScript v4 model/provider surface while preserving prototype method identity, JavaScript property descriptors, promise-valued capability properties, Web `ReadableStream` instances, and compatibility warning calls. The Rust port exposes only the current provider-v4 traits and model structs; there are no public Rust v2/v3 trait objects, JavaScript prototypes, or JS package-version object identities to accept or adapt. Portable current-version model/provider behavior is covered by the provider-v4 contract, middleware, mock model, provider registry, and high-level API rows. |
| Public mock models and test fixtures | verified | `src/mock_models.rs` | `mock_language_model_v4_returns_array_backed_generate_results_from_the_first_entry`; `mock_language_model_v4_returns_array_backed_stream_results_from_the_first_entry`; `mock_embedding_model_v4_returns_array_backed_embed_results_from_the_first_entry`; `mock_language_model_records_calls_and_returns_scripted_results`; `mock_language_model_can_drive_generate_text`; `mock_provider_resolves_registered_models_and_reports_missing_ids` | Mirrors current-version upstream `test/mock-language-model.test.ts` and `test/mock-embedding-model.test.ts` portable v4 array-backed script cases for generate, stream, and embed calls. Provides scriptable provider-v4 mock language, embedding, image, speech, transcription, reranking, video models, and a mock provider with shared call recording. Upstream v2/v3 mock helpers are JavaScript compatibility fixtures for legacy provider object versions and are documented as non-portable because the Rust port exposes only current provider-v4 traits and model structs. |
| Telemetry and logger | in-progress | `src/logger.rs`, `src/telemetry.rs`, `src/generate_text.rs`, `src/stream_text.rs`, `src/generate_object.rs`, `src/stream_object.rs`, `src/embed.rs`, `src/rerank.rs`, `crates/ai-sdk-otel` | `format_warning_matches_upstream_warning_messages`; `warning_logger_emits_info_once_for_first_non_empty_batch`; `warning_logger_can_be_disabled_and_reset`; `process_wide_log_warnings_matches_upstream_first_call_state`; `custom_logger_receives_original_options_without_default_records`; `telemetry_registry_adds_global_integrations_in_order`; `telemetry_dispatcher_invokes_local_integration_with_augmented_event`; `telemetry_dispatcher_uses_global_integrations_when_local_integrations_are_absent`; `telemetry_dispatcher_publishes_diagnostics_without_integrations`; `telemetry_dispatcher_wraps_execute_tool_and_prefers_local_wrappers`; `open_telemetry_integration_exports_dispatcher_spans_to_local_otlp_receiver`; `legacy_open_telemetry_integration_exports_dispatcher_spans_to_local_otlp_receiver`; ignored `live_vercel_ai_gateway_openai_compatible_generate_text_with_otel`; ignored `live_vercel_ai_gateway_openai_compatible_stream_text_with_otel`; ignored `live_vercel_ai_gateway_openai_compatible_generate_object_with_otel`; ignored `live_vercel_ai_gateway_openai_compatible_stream_object_with_otel`; ignored `live_vercel_ai_gateway_openai_responses_generate_text_with_otel`; ignored `live_vercel_ai_gateway_openai_responses_stream_text_with_otel`; `generate_text_dispatches_telemetry_lifecycle_events`; `generate_text_dispatches_tool_execution_telemetry_events`; `stream_text_dispatches_telemetry_lifecycle_events`; `stream_text_dispatches_tool_execution_telemetry_events`; `generate_object_dispatches_telemetry_lifecycle_events`; `stream_object_dispatches_telemetry_lifecycle_events`; `embed_dispatches_telemetry_lifecycle_events`; `embed_many_dispatches_telemetry_lifecycle_events`; `rerank_dispatches_telemetry_lifecycle_events`; `select_attributes_matches_telemetry_recording_flags`; `formats_system_and_input_messages`; `record_span_records_exception_status_and_ends_on_error`; `open_telemetry_records_generate_text_root_step_and_chat_spans`; `open_telemetry_records_object_operation_and_step_spans`; `open_telemetry_records_embedding_operation_and_inner_span`; `open_telemetry_records_rerank_operation_and_inner_span`; `legacy_open_telemetry_records_generate_text_step_tool_and_root_spans`; `legacy_open_telemetry_records_object_embedding_and_rerank_spans`; `local_otlp_http_receiver_captures_exported_span_payload`; `real_opentelemetry_http_exporter_sends_json_to_local_receiver` | Upstream `packages/ai/src/logger/log-warnings.ts` warning formatting, suppression, custom logger callback, first-call info, reset state, and deprecation warning classification are covered as root-owned `packages/ai` API. Root-owned `packages/ai/src/telemetry/*` now has an initial Rust dispatcher covering telemetry options, global vs per-call integration resolution, lifecycle fan-out, diagnostic channel publication, disabled telemetry behavior, panic isolation, execute-tool wrapper composition, a `create_open_telemetry_integration` adapter that translates dispatcher events into package-owned OTel semantic-convention spans, and a `create_legacy_open_telemetry_integration` adapter that translates the same dispatcher events into legacy `ai.*` spans; both adapters export through the local OTLP receiver path. `generate_text`, `stream_text`, `generate_object`, `stream_object`, `embed`, `embed_many`, and `rerank` now dispatch high-level lifecycle telemetry events through that dispatcher. The matching `ai-sdk-otel` crate covers initial `@ai-sdk/otel` helper surfaces for attribute gating, GenAI semantic-convention formatting, dependency-free text/tool/object/embedding/reranking lifecycle span recording for both `OpenTelemetry` and `LegacyOpenTelemetry`, local OTLP/HTTP receiver/export validation, and real Rust OpenTelemetry SDK OTLP/HTTP JSON exporter proof under the `real-opentelemetry` feature. Initial provider-live telemetry proof exists for Gateway OpenAI-compatible `generate_text`, `stream_text`, `generate_object`, and `stream_object`, plus Gateway OpenAI Responses `generate_text` and `stream_text`, through the local receiver. Remaining work: broaden provider live tests with telemetry enabled across remaining provider-backed rows. |
| MCP client and tool bridge | in-progress | `crates/ai-sdk-mcp` | `mcp_client_initializes_and_sends_initialized_notification`; `mcp_client_lists_calls_reads_resources_and_prompts`; `mcp_client_reports_capability_protocol_and_json_rpc_errors`; `mcp_client_handles_elicitation_request_messages`; `mcp_client_reports_elicitation_request_errors_to_server`; `mcp_client_invokes_uncaught_error_callback_for_transport_start_errors`; `mcp_client_invokes_uncaught_error_callback_for_elicitation_handler_errors`; `mcp_client_builds_executable_dynamic_tools_from_definitions`; `mcp_client_builds_schema_typed_tools_from_structured_content`; `mcp_client_schema_typed_tools_parse_text_content_fallback`; `mcp_client_schema_typed_tools_report_output_validation_errors`; `mcp_client_runs_authenticated_http_tools_with_output_schema_and_provider_metadata`; `mcp_http_transport_refreshes_oauth_tokens_for_unauthorized_inbound_sse`; `mcp_http_transport_refreshes_oauth_tokens_and_retries_unauthorized_post`; `mcp_http_transport_reopens_inbound_sse_after_accepted_post`; `mcp_http_transport_sends_last_event_id_when_resuming_inbound_sse`; `mcp_http_transport_retries_inbound_sse_open_failures`; `mcp_http_transport_retries_resumed_inbound_sse_after_accepted_post`; `mcp_http_transport_computes_inbound_sse_reconnect_backoff`; `mcp_http_transport_reports_max_inbound_sse_reconnect_attempts`; `mcp_http_transport_reports_invalid_inbound_sse_messages`; `mcp_sse_transport_connects_to_endpoint_and_posts_messages`; `mcp_sse_transport_refreshes_oauth_tokens_for_unauthorized_connect`; `mcp_sse_transport_refreshes_oauth_tokens_and_retries_unauthorized_post`; `mcp_transport_config_http_builds_authenticated_transport`; `mcp_transport_config_sse_builds_authenticated_transport`; `mcp_http_transport_rejects_redirects_by_default`; `mcp_sse_transport_parses_post_sse_message_responses`; `mcp_sse_transport_rejects_endpoint_origin_mismatch`; `mcp_sse_transport_reports_http_errors_with_http_hint`; `mcp_sse_transport_reports_post_errors`; `discover_oauth_protected_resource_metadata_uses_path_query_and_protocol_header`; `discover_authorization_server_metadata_tries_urls_in_order`; `select_resource_url_uses_protected_metadata_when_allowed`; `oauth_pkce_challenge_generates_random_url_safe_verifier`; `start_authorization_builds_pkce_resource_scope_state_and_prompt_params`; `start_authorization_can_generate_pkce_material`; `exchange_authorization_posts_code_verifier_client_secret_and_resource`; `refresh_authorization_posts_refresh_token_and_preserves_missing_replacement`; `register_client_posts_metadata_and_parses_full_information`; `auth_registers_client_and_redirects_when_tokens_are_missing`; `auth_exchanges_callback_code_and_saves_tokens_with_resource`; `auth_rejects_mismatched_callback_state_before_token_exchange`; `auth_invalidates_rejected_refresh_token_and_retries_to_redirect`; `auth_invalidates_rejected_client_credentials_and_reregisters`; `cargo run -p ai-sdk-mcp --example local_mcp_client`; `cargo run -p ai-sdk-mcp --example http_auth_typed_tools`; `cargo run -p ai-sdk-mcp --example stdio_typed_tools`; `cargo run -p ai-sdk-mcp --example sse_typed_tools`; `cargo run -p ai-sdk-mcp --example hosted_oauth_http`; `vercel_ai_gateway_openai_compatible_runs_generate_text_with_mcp_tools`; ignored `live_vercel_ai_gateway_openai_compatible_generate_text_mcp_tool_loop` | Package-owned MCP client now covers deterministic transport initialization, JSON-RPC request/notification lifecycle, negotiated protocol version storage, server info/instructions access, capability gating, tool listing/calling, resource and prompt methods, JSON-RPC error data, client-side `elicitation/create` request handling, upstream-shaped uncaught error callbacks, schema-filtered typed tool output extraction/validation, authenticated loopback Streamable HTTP tool execution with bearer headers, typed output validation, session cleanup, and MCP provider metadata assertions, transport-owned OAuth provider bearer-token injection plus one-shot 401 authorization retry for HTTP inbound SSE GET, HTTP JSON-RPC POST, standalone SSE connect, and standalone SSE endpoint POST using protected-resource metadata and refresh-token auth, upstream-shaped `McpTransportConfig`/`create_mcp_transport` HTTP and SSE factories with hosted-auth header/OAuth propagation, and default redirect rejection, standalone SSE transport endpoint parsing/POST dispatch/bounded POST message response parsing/error reporting, Streamable HTTP inbound SSE opening after start and `202 Accepted`, upstream-shaped inbound SSE reconnect delay/backoff/max-retry behavior, `Last-Event-ID` resume headers across retries, invalid inbound SSE message errors, dynamic AI SDK tool generation/execution from MCP definitions, OAuth resource/safe-URL helpers, protected-resource selection, protected-resource and authorization-server discovery over real loopback HTTP, authorization URL construction with generated or caller-supplied PKCE/resource/scope/state handling, token exchange, refresh, standard error response parsing, dynamic client registration, high-level OAuth provider orchestration, callback state validation, credential invalidation retry behavior, and package-owned local MCP examples for listing/calling deterministic in-process tools plus authenticated Streamable HTTP, stdio, SSE, and hosted OAuth HTTP execution with provider metadata and protected auth-flow coverage, plus a Vercel AI Gateway OpenAI-compatible `generate_text` integration that consumes MCP tool definitions and has an ignored live Gateway proof for the same tool-loop path. Protected live-service auth validation remains unported without suitable live service credentials. |
| Workflow agent package | in-progress | `crates/ai-sdk-workflow` | `serialize_tool_set_serializes_function_tools_with_description_and_input_schema`; `resolve_serializable_tools_reconstructs_provider_tools`; `to_ui_message_chunk_maps_text_reasoning_and_tool_call_parts`; `to_ui_message_chunk_maps_files_sources_results_approval_and_errors`; `model_call_stream_to_ui_chunks_adds_lifecycle_chunks_and_drops_internal_parts`; `workflow_chat_transport_reconnects_after_interrupted_send_using_run_id_and_chunk_index`; `workflow_chat_transport_reconnect_resolves_negative_start_index_from_tail_header`; `workflow_chat_transport_reconnect_formats_consecutive_errors`; `stream_text_iterator_maps_provider_metadata_to_provider_options_for_continuation`; `stream_text_iterator_upstream_should_not_add_provider_options_when_provider_metadata_is_undefined`; `stream_text_iterator_upstream_should_strip_openai_item_id_from_provider_metadata_to_avoid_reasoning_item_errors`; `stream_text_iterator_upstream_should_preserve_other_openai_metadata_while_stripping_item_id`; `stream_text_iterator_upstream_should_preserve_gemini_metadata_while_stripping_openai_item_id_in_mixed_provider_metadata`; `stream_text_iterator_strips_openai_item_id_and_preserves_other_metadata`; `stream_text_iterator_passes_contexts_to_executor_and_yields_them`; `stream_text_iterator_upstream_should_allow_prepare_step_to_modify_messages`; `stream_text_iterator_upstream_should_apply_prepare_step_system_after_messages_override`; `stream_text_iterator_upstream_should_allow_prepare_step_to_change_model_dynamically`; `stream_text_iterator_upstream_should_allow_prepare_step_to_set_active_tools_and_tool_choice`; `stream_text_iterator_upstream_should_update_runtime_and_tools_context_from_prepare_step`; `do_stream_step_from_parts_collects_provider_executed_results_and_valid_step_content`; `workflow_agent_upstream_should_expose_id_when_provided_in_constructor`; `workflow_agent_upstream_should_have_undefined_id_when_not_provided`; `workflow_agent_upstream_should_convert_tool_execution_error_to_error_text_result`; `workflow_agent_upstream_should_successfully_execute_tools_that_return_normally`; `workflow_agent_upstream_should_skip_local_execution_for_provider_executed_tools`; `workflow_agent_upstream_should_handle_provider_executed_tool_errors_with_is_error_flag`; `workflow_agent_upstream_should_return_empty_result_when_provider_executed_tool_result_is_missing`; `workflow_agent_upstream_should_stop_the_loop_for_client_side_tools_without_execute`; `workflow_agent_upstream_should_call_on_finish_when_stopping_for_client_side_tools`; `workflow_agent_compat_should_call_on_finish_from_constructor`; `workflow_agent_compat_should_call_on_finish_from_stream_method`; `workflow_agent_compat_should_call_both_constructor_and_method_on_finish_in_correct_order`; `workflow_agent_compat_should_pass_finish_event_information`; `workflow_agent_compat_should_call_experimental_on_start_from_constructor`; `workflow_agent_compat_should_call_experimental_on_start_from_stream_method`; `workflow_agent_compat_should_call_both_constructor_and_method_experimental_on_start_in_correct_order`; `workflow_agent_compat_should_pass_experimental_on_start_event_information`; `workflow_agent_compat_should_call_experimental_on_step_start_from_constructor`; `workflow_agent_compat_should_call_experimental_on_step_start_from_stream_method`; `workflow_agent_compat_should_call_both_constructor_and_method_experimental_on_step_start_in_correct_order`; `workflow_agent_compat_should_pass_experimental_on_step_start_event_information`; `workflow_agent_compat_should_call_on_step_finish_from_constructor`; `workflow_agent_compat_should_call_on_step_finish_from_stream_method`; `workflow_agent_compat_should_call_both_constructor_and_method_on_step_finish_in_correct_order`; `workflow_agent_compat_should_pass_step_result_to_on_step_finish_callback`; `workflow_agent_compat_should_call_on_tool_execution_start_from_constructor`; `workflow_agent_compat_should_call_on_tool_execution_start_from_stream_method`; `workflow_agent_compat_should_call_both_constructor_and_method_on_tool_execution_start_in_correct_order`; `workflow_agent_compat_should_pass_tool_execution_start_event_information`; `workflow_agent_compat_should_call_on_tool_execution_end_from_constructor`; `workflow_agent_compat_should_call_on_tool_execution_end_from_stream_method`; `workflow_agent_compat_should_call_both_constructor_and_method_on_tool_execution_end_in_correct_order`; `workflow_agent_compat_should_pass_tool_execution_end_event_information_on_success`; `workflow_agent_upstream_should_have_empty_tool_calls_when_all_tools_complete_normally`; `workflow_agent_upstream_should_pass_generation_settings_from_constructor_to_stream_text_iterator`; `workflow_agent_upstream_should_allow_stream_options_to_override_constructor_generation_settings`; `workflow_agent_upstream_should_pass_tool_choice_from_constructor_to_stream_text_iterator`; `workflow_agent_upstream_should_allow_stream_options_to_override_constructor_tool_choice`; `workflow_agent_upstream_should_filter_tools_when_active_tools_is_specified`; `workflow_agent_upstream_should_pass_conversation_messages_to_tool_execute_function`; `workflow_agent_upstream_should_pass_per_tool_tools_context_entry_as_execute_context`; `workflow_agent_upstream_should_validate_per_tool_context_against_context_schema`; `workflow_agent_upstream_should_pass_prepare_step_callback_to_stream_text_iterator`; `workflow_agent_upstream_prepare_step_updates_runtime_context_for_agent_loop` | Initial Rust crate-owned `@ai-sdk/workflow` foundation covers serializable tool definitions, reconstruction across workflow step boundaries, model-call stream to UI-message chunk conversion with lifecycle wrappers, deterministic chat transport request/reconnect behavior, a deterministic stream-text iterator stepper that appends assistant tool-call messages, consumes tool-result continuations, preserves provider metadata as provider options for single, parallel, mixed, and absent tool-call continuations with OpenAI item-id sanitization, forwards runtime/tool contexts, applies prepare-step message/system/model/generation/active-tool/tool-choice/runtime/tool-context overrides, and collects provider-executed tool results, plus the first deterministic WorkflowAgent facade for optional id, local tool success/errors, provider-executed tool results/errors/missing-result fallback, client-side tool stopping, finish callbacks for client-side stops, constructor-then-stream finish callback ordering with event payloads, start and step-start callbacks with constructor-then-stream ordering and event payloads, step-finish callbacks with constructor-then-stream ordering and step payloads, tool-execution start/end callbacks with constructor-then-stream ordering and event payloads, empty final tool calls after completed tool rounds, accumulated-message execution options, per-tool context delivery, agent-level prepare-step forwarding, constructor and stream-level generation-setting and tool-choice forwarding, active-tool filtering, runtime context prepare-step updates, and Rust-side context validator failures. Real model-backed iterator execution, real HTTP/SSE transport adapters, integration workflows, and full JSON Schema runtime validation remain unported. |
| WorkflowAgent mixed tool execution and invalid calls | verified | `crates/ai-sdk-workflow/src/workflow_agent.rs` | `workflow_agent_upstream_should_handle_mixed_provider_executed_and_local_tools`; `workflow_agent_upstream_should_handle_mixed_executable_and_client_side_tools_in_same_step`; `workflow_agent_upstream_should_keep_invalid_tool_calls_on_error_path_without_executing` | Adds named Rust counterparts for upstream `workflow-agent.test.ts` mixed provider/local tool rounds, mixed executable/client-side tool rounds, and invalid tool-call error-path behavior. The agent executes local tools, consumes provider-executed stream results, returns only server-executed results when a client-side tool stops the loop, and converts invalid calls to `error-text` without invoking the local executor. |
| Gateway provider and metadata APIs | verified | `crates/ai-sdk-gateway`, root facade shims in `src/gateway.rs`, `src/gateway_error.rs`, and `src/gateway_tools.rs` | `gateway_model_generates_text_through_generate_text`; `gateway_model_generates_object_through_generate_object`; `gateway_model_maps_standard_generate_content_parts`; `gateway_model_maps_standard_generate_content_parts_through_generate_text`; `gateway_model_runs_generate_text_tool_loop_end_to_end`; `gateway_model_streams_text_through_stream_text`; `gateway_model_streams_object_through_stream_object`; `gateway_model_streams_standard_content_parts_through_stream_text`; `gateway_model_runs_stream_text_tool_loop_end_to_end`; `gateway_model_encodes_language_prompt_file_bytes_for_generate`; `gateway_model_encodes_language_prompt_file_bytes_for_stream`; `gateway_provider_options_serialize_upstream_shape`; `gateway_provider_options_validation_matches_timeout_schema`; `gateway_model_passes_typed_gateway_provider_options_for_generate`; `gateway_model_passes_typed_gateway_provider_options_for_stream`; `gateway_embedding_model_embeds_through_embed`; `gateway_image_model_generates_through_generate_image`; `gateway_image_model_preserves_metadata_entries_without_images`; `gateway_image_model_encodes_files_and_mask`; `gateway_reranking_model_reranks_through_rerank`; `gateway_reranking_model_omits_optional_body_fields`; `gateway_video_model_generates_through_generate_video`; `gateway_video_model_preserves_empty_and_nested_provider_metadata`; `gateway_video_model_encodes_image_inputs_and_returns_url_videos`; `create_gateway_language_model_uses_custom_configuration`; `create_gateway_language_model_uses_oidc_when_api_key_is_absent`; `gateway_provider_language_model_handles_model_specification_errors`; `gateway_provider_language_model_accepts_any_model_id`; `gateway_provider_language_model_accepts_non_existent_model_id`; `create_gateway_embedding_model_returns_gateway_embedding_model`; `create_gateway_image_model_uses_custom_base_url`; `create_gateway_image_model_reuses_headers_transport_and_observability`; `create_gateway_video_model_uses_custom_base_url`; `create_gateway_video_model_reuses_headers_transport_and_observability`; `create_gateway_reranking_model_uses_custom_base_url`; `create_gateway_reranking_alias_returns_gateway_reranking_model`; `create_gateway_fetches_available_models_with_custom_base_url`; `create_gateway_caches_metadata_for_configured_refresh_interval`; `create_gateway_uses_default_five_minute_metadata_refresh_interval`; `create_gateway_language_model_passes_observability_headers_from_environment`; `create_gateway_language_model_omits_missing_observability_headers`; `default_gateway_export_exposes_provider_instance`; `create_gateway_uses_default_base_url_when_none_is_provided`; `create_gateway_accepts_empty_options`; `default_gateway_export_constructs_image_model`; `default_gateway_export_constructs_video_model`; `create_gateway_overrides_default_base_url_when_provided`; `create_gateway_prefers_api_key_over_oidc_token`; `gateway_provider_real_world_vercel_deployment_uses_oidc_authentication`; `gateway_provider_real_world_local_development_uses_api_key_authentication`; `gateway_provider_real_world_explicit_api_key_override_wins_over_environment`; `create_gateway_authentication_handles_no_auth_at_all`; `create_gateway_authentication_handles_valid_oidc_invalid_api_key`; `create_gateway_authentication_handles_invalid_oidc_valid_api_key`; `create_gateway_authentication_handles_no_oidc_invalid_api_key`; `create_gateway_authentication_handles_no_oidc_valid_api_key`; `create_gateway_authentication_handles_valid_oidc_no_api_key`; `create_gateway_authentication_handles_valid_oidc_valid_api_key`; `create_gateway_authentication_handles_valid_oidc_valid_options_api_key`; `create_gateway_authentication_handles_invalid_oidc_invalid_api_key`; `gateway_provider_creates_embedding_model_aliases`; `gateway_provider_creates_image_model_aliases`; `gateway_provider_creates_reranking_model_aliases`; `gateway_provider_creates_video_model_aliases`; `gateway_provider_implements_provider_traits`; `gateway_provider_exposes_gateway_tools`; `perplexity_search_tool_factory_matches_gateway_provider_tool_contract`; `parallel_search_tool_factory_matches_gateway_provider_tool_contract`; `gateway_tools_create_provider_executed_perplexity_search_tool`; `gateway_tools_create_provider_executed_parallel_search_tool`; `get_gateway_auth_token_matches_upstream_precedence`; `get_gateway_auth_token_ignores_empty_values_without_trimming_whitespace`; `get_gateway_auth_token_handles_no_auth_at_all`; `get_gateway_auth_token_handles_valid_oidc_invalid_api_key`; `get_gateway_auth_token_handles_invalid_oidc_valid_api_key`; `get_gateway_auth_token_handles_no_oidc_invalid_api_key`; `get_gateway_auth_token_handles_no_oidc_valid_api_key`; `get_gateway_auth_token_handles_valid_oidc_no_api_key`; `get_gateway_auth_token_handles_valid_oidc_valid_api_key`; `get_gateway_auth_token_handles_valid_oidc_valid_options_api_key`; `get_gateway_auth_token_handles_invalid_oidc_invalid_api_key`; `get_gateway_auth_token_treats_empty_environment_variables_as_missing`; `get_gateway_auth_token_uses_whitespace_environment_api_key`; `get_gateway_auth_token_prioritizes_options_api_key_over_all_environment_variables`; `get_gateway_auth_token_prefers_options_api_key_over_ai_gateway_api_key`; `get_gateway_auth_token_prefers_ai_gateway_api_key_over_oidc_token`; `get_gateway_auth_token_falls_back_to_oidc_when_no_api_keys_are_available`; `gateway_provider_headers_support_oidc_auth_method`; `gateway_observability_headers_map_vercel_environment`; `gateway_observability_headers_skip_empty_values_and_use_request_env_fallback`; `gateway_provider_fetches_available_models_metadata`; `gateway_provider_caches_available_models_until_refresh`; `gateway_provider_refreshes_available_models_after_refresh_interval`; `gateway_provider_uses_default_metadata_cache_refresh_interval`; `gateway_provider_refreshes_available_models_when_cache_disabled`; `gateway_provider_fetches_credits_from_gateway_origin`; `gateway_provider_get_credits_includes_upstream_headers`; `gateway_provider_get_credits_surfaces_endpoint_errors`; `gateway_provider_get_credits_fetches_successfully`; `gateway_provider_get_credits_handles_authentication_errors`; `gateway_provider_get_credits_uses_custom_base_url`; `gateway_provider_get_credits_uses_oidc_authentication_headers`; `gateway_provider_get_credits_is_available_on_provider_interface`; `gateway_provider_account_methods_use_default_gateway_urls`; `gateway_provider_fetches_spend_report_with_query_params`; `gateway_provider_get_spend_report_fetches_successfully`; `gateway_provider_get_spend_report_passes_params_through`; `gateway_provider_get_spend_report_uses_custom_base_url`; `gateway_provider_get_spend_report_uses_custom_transport`; `gateway_provider_get_spend_report_is_available_on_provider_interface`; `default_gateway_export_get_spend_report_is_available`; `gateway_provider_get_spend_report_surfaces_endpoint_errors`; `gateway_provider_fetches_generation_info_and_unwraps_data`; `gateway_provider_metadata_surfaces_api_errors`; `gateway_provider_metadata_fetch_errors_convert_to_gateway_errors`; `gateway_provider_metadata_gateway_errors_are_not_double_wrapped`; `gateway_provider_account_apis_surface_malformed_json_error_responses`; `gateway_error_types_expose_upstream_names_status_and_retryability`; ignored `live_gateway_openai_generate_text`; ignored `live_gateway_openai_generate_object`; ignored `live_gateway_openai_stream_text`; ignored `live_gateway_openai_stream_object`; ignored `live_gateway_openai_embed`; ignored `live_gateway_openai_generate_image`; ignored `live_gateway_rerank`; ignored `live_gateway_generate_video`; ignored `live_gateway_available_models` | Gateway provider implementation, error classification, metadata/account APIs, and provider-executed Gateway tools now live in the matching `ai-sdk-gateway` crate. Minimal provider settings, `create_gateway`, deprecated `create_gateway_provider` alias, `gateway`, provider-v4 trait and optional reranking/video trait integration, non-streaming language/object model calls, provider-v4 generated content part parsing, high-level generated/streamed content-part mapping, SSE streaming, high-level generate/stream tool-loop continuation from Gateway tool-call content, high-level object and stream-object JSON response-format calls, language prompt file byte encoding, typed Gateway provider options with upstream-minimum `providerTimeouts.byok` validation helpers, embedding model calls, image model calls, image/video provider metadata edges, reranking model calls, video model calls, raw-chunk filtering, model request headers, `createGateway` custom base URL/API-key/custom headers for language-model requests plus OIDC fallback when no API key is configured, model factories for embedding/image/video/reranking, image/video header/transport/observability reuse, reranking alias, metadata fetch/cache/default-base/error routing, default provider image/video construction, observability header resolution, API-key precedence over OIDC, portable auth scenario/environment edge-case coverage, and API-key/OIDC auth resolution, Vercel observability headers, cached `get_available_models`, metadata cache expiry, default cache timing, default metadata/account routing, `get_credits` success/auth-error/custom-base/OIDC-header/provider-interface cases, credit request headers, `get_spend_report` success/parameter-forwarding/custom-base/custom-transport/provider-interface/default-export cases, credit/spend endpoint error propagation, `get_generation_info`, and malformed account API error responses are implemented and tested in `crates/ai-sdk-gateway`; root `src/gateway.rs` is now only a compatibility shim plus high-level SDK integration tests. Upstream mocked `getVercelOidcToken` rejection-only cases are JavaScript OIDC-provider-mock-specific and are documented as non-portable for Rust, which reads the configured token source directly. The current upstream Gateway package test corpus is fully mapped, with JavaScript OIDC mock plumbing and callable-constructor identity checks documented as non-portable. |
| Gateway metadata/account edge cases | verified | `crates/ai-sdk-gateway/src/gateway.rs` | `gateway_provider_metadata_preserves_known_model_types_and_filters_unknown`; `gateway_provider_metadata_rejects_invalid_pricing_format`; `gateway_provider_caches_available_models_until_refresh`; `gateway_provider_refreshes_available_models_after_refresh_interval`; `gateway_provider_uses_default_metadata_cache_refresh_interval`; `gateway_provider_account_methods_use_default_gateway_urls`; `gateway_provider_get_credits_includes_upstream_headers`; `gateway_provider_get_credits_surfaces_endpoint_errors`; `gateway_provider_fetches_empty_metadata_and_zero_credits`; `gateway_provider_spend_report_omits_optional_query_params_and_metrics`; `gateway_provider_get_spend_report_surfaces_endpoint_errors`; `gateway_provider_generation_info_encodes_special_ids_and_byok_response`; `gateway_provider_account_apis_surface_malformed_json_error_responses` | Mirrors upstream Gateway metadata, credits, spend report, and generation info tests for known/unknown model types, malformed pricing, immediate metadata cache reuse, metadata cache expiry after the configured refresh interval, default cache timing, default Gateway metadata/account endpoint routing, credit request headers, credit/spend endpoint transport errors, empty account data, omitted optional query parameters, sparse metric rows, BYOK generation data, URL encoding, and malformed account API error responses. |
| Gateway fetch metadata upstream case list | verified | `crates/ai-sdk-gateway/src/gateway.rs`, `crates/ai-sdk-gateway/src/gateway_error.rs` | `gateway_fetch_metadata_fetches_available_models_from_correct_endpoint`; `gateway_fetch_metadata_handles_models_with_pricing_information`; `gateway_fetch_metadata_maps_cache_pricing_fields_to_sdk_names`; `gateway_fetch_metadata_handles_models_without_pricing_information`; `gateway_fetch_metadata_handles_mixed_models_with_and_without_pricing`; `gateway_fetch_metadata_handles_models_with_description`; `gateway_fetch_metadata_accepts_top_level_model_type_when_present`; `gateway_fetch_metadata_filters_unknown_model_type_values`; `gateway_fetch_metadata_preserves_all_known_model_type_values`; `gateway_fetch_metadata_keeps_known_models_and_filters_unknown_from_mixed_response`; `gateway_fetch_metadata_passes_headers_correctly`; `gateway_fetch_metadata_handles_api_errors`; `gateway_fetch_metadata_converts_api_call_errors_to_gateway_errors`; `gateway_fetch_metadata_handles_malformed_json_error_responses`; `gateway_fetch_metadata_handles_malformed_response_data`; `gateway_fetch_metadata_rejects_models_with_invalid_pricing_format`; `gateway_fetch_metadata_does_not_double_wrap_existing_gateway_errors`; `gateway_fetch_metadata_handles_rate_limit_server_errors`; `gateway_fetch_metadata_handles_internal_server_errors`; `gateway_fetch_metadata_preserves_error_cause_chain`; `gateway_fetch_metadata_uses_custom_fetch_function_when_provided`; `gateway_fetch_metadata_handles_empty_response`; `gateway_fetch_metadata_fetches_credits_from_correct_endpoint`; `gateway_fetch_metadata_passes_headers_correctly_to_credits_endpoint`; `gateway_fetch_metadata_handles_api_errors_for_credits_endpoint`; `gateway_fetch_metadata_handles_rate_limit_errors_for_credits_endpoint`; `gateway_fetch_metadata_handles_internal_server_errors_for_credits_endpoint`; `gateway_fetch_metadata_handles_malformed_credits_response`; `gateway_fetch_metadata_uses_custom_fetch_function_for_credits`; `gateway_fetch_metadata_converts_credits_api_call_errors_to_gateway_errors`; `gateway_fetch_metadata_handles_credits_malformed_json_error_responses`; `gateway_fetch_metadata_does_not_double_wrap_existing_credit_gateway_errors`; `gateway_fetch_metadata_preserves_credits_error_cause_chain`; `gateway_fetch_metadata_handles_empty_credits_response` | Mirrors every portable upstream `gateway-fetch-metadata.test.ts` case one-to-one for available-model endpoint/method construction, pricing and cache-pricing field mapping, optional pricing and description preservation, top-level `modelType` parsing/filtering, request headers, 401/403/429/500 Gateway error classification, malformed success/error responses, existing Gateway error preservation, error cause preservation, injected-transport custom fetch behavior, empty metadata, credits endpoint/header/request mapping, malformed-but-string credits values, and empty credits. Rust represents JavaScript custom `fetch` through the injected `GatewayTransport` boundary and preserves HTTP error causes through `GatewayError::cause_message`. |
| Gateway embedding model upstream case list | verified | `crates/ai-sdk-gateway/src/gateway.rs` | `gateway_embedding_model_passes_headers_correctly`; `gateway_embedding_model_includes_observability_headers`; `gateway_embedding_model_extracts_embeddings_and_usage`; `gateway_embedding_model_sends_values_as_array`; `gateway_embedding_model_passes_provider_options_into_request_body`; `gateway_embedding_model_omits_provider_options_when_not_provided`; `gateway_embedding_model_converts_gateway_error_responses`; `gateway_embedding_model_includes_provider_metadata_in_response_body`; `gateway_embedding_model_extracts_provider_metadata_to_top_level` | Mirrors every portable upstream `gateway-embedding-model.test.ts` case one-to-one for request headers, observability headers, embeddings and usage extraction, `values` request array shape, providerOptions body passthrough and omission, 400/500 Gateway error classification, raw response body providerMetadata preservation, and top-level providerMetadata extraction. Rust represents JavaScript rejected error classes through the typed `EmbeddingModelResult` error metadata because the Rust embedding provider trait returns a result value rather than throwing. |
| Gateway image model upstream case list | verified | `crates/ai-sdk-gateway/src/gateway.rs` | `gateway_image_model_creates_instance_with_correct_properties`; `gateway_image_model_avoids_client_side_splitting_even_for_bfl_models`; `gateway_image_model_accepts_custom_provider_name`; `gateway_image_model_sends_correct_request_headers`; `gateway_image_model_sends_correct_request_body_with_all_parameters`; `gateway_image_model_omits_optional_parameters_when_not_provided`; `gateway_image_model_returns_images_array_correctly`; `gateway_image_model_returns_provider_metadata_correctly`; `gateway_image_model_handles_provider_metadata_without_images_field`; `gateway_image_model_handles_empty_provider_metadata`; `gateway_image_model_handles_undefined_provider_metadata`; `gateway_image_model_returns_warnings_when_provided`; `gateway_image_model_returns_unsupported_warnings_correctly`; `gateway_image_model_returns_compatibility_warnings_correctly`; `gateway_image_model_handles_mixed_warning_types`; `gateway_image_model_returns_empty_warnings_array_when_not_provided`; `gateway_image_model_includes_response_metadata`; `gateway_image_model_returns_usage_when_provided`; `gateway_image_model_returns_usage_with_partial_token_counts`; `gateway_image_model_does_not_include_usage_when_not_provided`; `gateway_image_model_merges_custom_headers_with_config_headers`; `gateway_image_model_includes_o11y_headers`; `gateway_image_model_passes_abort_signal_to_fetch`; `gateway_image_model_handles_api_errors_correctly`; `gateway_image_model_handles_authentication_errors`; `gateway_image_model_includes_provider_options_object_in_request_body`; `gateway_image_model_handles_empty_provider_options`; `gateway_image_model_handles_different_model_ids`; `gateway_image_model_handles_complex_provider_metadata_with_multiple_providers`; `gateway_image_model_encodes_uint8_array_files_to_base64_strings`; `gateway_image_model_passes_through_files_with_string_data_unchanged`; `gateway_image_model_passes_through_url_type_files_unchanged`; `gateway_image_model_encodes_uint8_array_mask_to_base64_string`; `gateway_image_model_handles_mixed_file_types_with_encoding`; `gateway_image_model_preserves_provider_options_on_files_during_encoding` | Mirrors every portable upstream `gateway-image-model.test.ts` case one-to-one for constructor properties, `Number.MAX_SAFE_INTEGER`-equivalent no client-side splitting, custom provider id support, request headers/body shape, optional field omission, image result mapping, provider metadata with/without `images`, empty and absent metadata, warning variants, default empty warnings, response metadata, usage variants, provider plus call headers, observability request headers, abort-signal forwarding, API/auth error metadata, providerOptions passthrough, alternate model ids, complex multi-provider metadata, and file/mask data encoding. Rust represents JavaScript rejected error classes through typed `ImageModelResult` error metadata because the Rust image provider trait returns a result value rather than throwing. |
| Gateway video model upstream case list | verified | `crates/ai-sdk-gateway/src/gateway.rs` | `gateway_video_model_creates_instance_with_correct_properties`; `gateway_video_model_avoids_client_side_splitting_for_video_models`; `gateway_video_model_accepts_custom_provider_name`; `gateway_video_model_sends_correct_request_headers`; `gateway_video_model_sends_correct_request_body_with_all_parameters`; `gateway_video_model_omits_optional_parameters_when_not_provided`; `gateway_video_model_returns_videos_array_correctly`; `gateway_video_model_returns_url_type_videos_correctly`; `gateway_video_model_returns_provider_metadata_correctly`; `gateway_video_model_handles_provider_metadata_without_videos_field`; `gateway_video_model_handles_empty_provider_metadata`; `gateway_video_model_handles_undefined_provider_metadata`; `gateway_video_model_returns_warnings_when_provided`; `gateway_video_model_returns_unsupported_warnings_correctly`; `gateway_video_model_returns_compatibility_warnings_correctly`; `gateway_video_model_returns_empty_warnings_array_when_not_provided`; `gateway_video_model_includes_response_metadata`; `gateway_video_model_merges_custom_headers_with_config_headers`; `gateway_video_model_includes_o11y_headers`; `gateway_video_model_passes_abort_signal_to_fetch`; `gateway_video_model_handles_api_errors_correctly`; `gateway_video_model_handles_authentication_errors`; `gateway_video_model_throws_on_sse_error_event_with_correct_message_and_status`; `gateway_video_model_throws_on_sse_error_event_with_provider_routing_failure`; `gateway_video_model_throws_on_empty_sse_stream`; `gateway_video_model_ignores_sse_heartbeat_comments_and_parses_data_event`; `gateway_video_model_includes_provider_options_object_in_request_body`; `gateway_video_model_handles_empty_provider_options`; `gateway_video_model_handles_different_model_ids`; `gateway_video_model_handles_complex_provider_metadata_with_multiple_providers`; `gateway_video_model_encodes_uint8_array_image_to_base64_string`; `gateway_video_model_passes_through_image_with_string_data_unchanged`; `gateway_video_model_passes_through_url_type_image_unchanged`; `gateway_video_model_preserves_provider_options_on_image_during_encoding` | Mirrors every portable upstream `gateway-video-model.test.ts` case one-to-one for constructor properties, max-video no client-side splitting, custom provider id support, request headers/body shape, optional field omission, base64 and URL video result mapping, provider metadata with/without `videos`, empty and absent metadata, warning variants, default empty warnings, response metadata, merged headers, observability request headers, abort-signal forwarding, HTTP API/auth error metadata, SSE error events, empty/heartbeat-only SSE handling, providerOptions passthrough, alternate model ids, complex multi-provider metadata, and image-to-video file/URL/providerOptions encoding. Rust represents JavaScript rejected errors through typed `VideoModelResult` Gateway error metadata because the Rust video provider trait returns a result value rather than throwing. |
| Gateway reranking model upstream case list | verified | `crates/ai-sdk-gateway/src/gateway.rs` | `gateway_reranking_model_passes_headers_correctly`; `gateway_reranking_model_includes_observability_headers`; `gateway_reranking_model_extracts_ranking_from_response`; `gateway_reranking_model_sends_documents_and_query_in_request_body`; `gateway_reranking_model_passes_provider_options_into_request_body`; `gateway_reranking_model_omits_top_n_when_not_provided`; `gateway_reranking_model_returns_response_headers`; `gateway_reranking_model_returns_provider_metadata`; `gateway_reranking_model_maps_invalid_request_error_response`; `gateway_reranking_model_maps_internal_server_error_response`; `gateway_reranking_model_posts_to_reranking_model_endpoint` | Mirrors every portable upstream `gateway-reranking-model.test.ts` case one-to-one for request headers, observability headers, ranking extraction, documents/query/topN request shape, providerOptions body passthrough, topN omission, response headers, providerMetadata extraction, 400 invalid-request and 500 internal-server error classification, and `/reranking-model` endpoint construction. Rust represents JavaScript rejected error classes through typed `RerankingModelResult` error metadata because the Rust reranking provider trait returns a result value rather than throwing. |
| Gateway spend report upstream case list | verified | `crates/ai-sdk-gateway/src/gateway.rs` | `gateway_provider_spend_report_fetches_from_correct_endpoint_with_required_params`; `gateway_provider_spend_report_serializes_all_optional_query_params`; `gateway_provider_spend_report_omits_optional_params_when_not_provided`; `gateway_provider_spend_report_omits_empty_tags_query_param`; `gateway_provider_spend_report_transforms_snake_case_response_fields_to_camel_case`; `gateway_provider_spend_report_transforms_credential_type_response_field`; `gateway_provider_spend_report_handles_group_by_model_response`; `gateway_provider_spend_report_handles_empty_results`; `gateway_provider_spend_report_omits_optional_metric_fields_when_not_present`; `gateway_provider_spend_report_passes_headers_correctly`; `gateway_provider_spend_report_handles_401_authentication_errors`; `gateway_provider_spend_report_handles_429_rate_limit_errors`; `gateway_provider_spend_report_handles_500_internal_server_errors`; `gateway_provider_spend_report_handles_malformed_json_error_responses`; `gateway_provider_spend_report_uses_custom_transport` | Mirrors every portable upstream `gateway-spend-report.test.ts` case one-to-one. Rust represents JavaScript camelCase result checks through serde serialization of the Rust row type, and represents the custom `fetch` case through the injected `GatewayTransport` boundary. |
| Gateway generation info upstream case list | verified | `crates/ai-sdk-gateway/src/gateway.rs` | `gateway_provider_generation_info_fetches_from_correct_endpoint_with_generation_id`; `gateway_provider_generation_info_transforms_snake_case_response_fields_to_camel_case`; `gateway_provider_generation_info_unwraps_data_envelope`; `gateway_provider_generation_info_omits_snake_case_fields_from_serialized_result`; `gateway_provider_generation_info_passes_headers_correctly`; `gateway_provider_generation_info_handles_401_authentication_errors`; `gateway_provider_generation_info_handles_500_internal_server_errors`; `gateway_provider_generation_info_handles_malformed_json_error_responses`; `gateway_provider_generation_info_uses_custom_transport`; `gateway_provider_generation_info_encodes_special_characters_in_generation_id`; `gateway_provider_generation_info_handles_byok_generation_response` | Mirrors every portable upstream `gateway-generation-info.test.ts` case one-to-one. Rust represents JavaScript camelCase result checks through serde serialization of the Rust struct, and represents the custom `fetch` case through the injected `GatewayTransport` boundary. |
| Gateway Vercel request-context environment helper | js-only-documented | none; Rust equivalent request-id coverage is in `crates/ai-sdk-gateway/src/gateway.rs` | upstream `vercel-environment.test.ts`: `should get request ID from request headers when available`; `should return undefined when request ID header is not available`; `should return undefined when no headers are available`; `should handle missing request context gracefully`; `should handle missing get method in request context` | Upstream `getVercelRequestId` reads Vercel's JavaScript runtime request context from `globalThis[Symbol.for('@vercel/request-context')]`. Rust has no equivalent JS global request context, so these cases are intentionally non-portable. Rust callers pass `GatewayProviderSettings::with_vercel_request_id` or use environment fallbacks; `gateway_observability_headers_map_vercel_environment` and `gateway_observability_headers_skip_empty_values_and_use_request_env_fallback` cover the portable request-id/header behavior. |
| Gateway direct model error classification | verified | `crates/ai-sdk-gateway/src/gateway.rs`, `crates/ai-sdk-gateway/src/gateway_error.rs` | `gateway_model_maps_gateway_error_to_error_finish_reason`; `gateway_model_preserves_structured_gateway_error_metadata`; `gateway_model_classifies_transport_timeout_errors`; `gateway_model_stream_classifies_transport_timeout_errors` | Mirrors portable upstream Gateway model error behavior by classifying API and transport failures into Rust result metadata or stream error parts, preserving Gateway error type, status code, retryability, generation id, and timeout classification while omitting JavaScript-only thrown-error identity and AbortSignal behavior. |
| Gateway video SSE error classification | verified | `crates/ai-sdk-gateway/src/gateway.rs` | `gateway_video_model_maps_sse_error_to_metadata`; `gateway_video_model_maps_heartbeat_only_sse_to_metadata` | Gateway video-model SSE handling now mirrors upstream error-event and empty-stream behavior in Rust result metadata, preserving provider error type, status code, retryability, and response-error metadata for heartbeat-only streams that end without a result event. |
| Gateway image usage edge cases | verified | `crates/ai-sdk-gateway/src/gateway.rs` | `gateway_image_model_maps_partial_and_missing_usage` | Gateway image-model responses now directly cover upstream partial usage payloads such as `inputTokens` without output/total counts, and absence of the usage block remains `None` instead of creating empty usage metadata. |
| Gateway media warning variants | verified | `crates/ai-sdk-gateway/src/gateway.rs` | `gateway_image_model_preserves_warning_variants`; `gateway_video_model_preserves_warning_variants` | Gateway image and video models now directly verify upstream warning payload preservation for unsupported warnings without details, compatibility warnings with details, and provider `other` warnings. |
| OpenAI-compatible chat provider foundation | in-progress | `crates/ai-sdk-openai-compatible`; root facade shim in `src/openai_compatible.rs`; Vercel AI Gateway integration in `src/vercel_ai_gateway.rs` | `openai_compatible_provider_configures_headers_urls_and_model_aliases`; `openai_compatible_provider_lists_models`; `openai_compatible_chat_generates_text_through_generate_text`; `openai_compatible_chat_streams_text_through_stream_text`; `openai_compatible_chat_streams_reasoning_raw_chunks_and_parse_errors`; `openai_compatible_chat_passes_tools_tool_choice_and_provider_options`; `openai_compatible_chat_converts_multimodal_user_messages`; `openai_compatible_chat_rejects_unsupported_file_messages_before_transport`; `openai_compatible_chat_converts_assistant_tool_history`; `openai_compatible_chat_runs_generate_text_tool_loop_end_to_end`; `openai_compatible_chat_runs_stream_text_tool_loop_end_to_end`; `openai_compatible_chat_maps_tool_calls_from_generate`; `openai_compatible_chat_streams_tool_calls`; `openai_compatible_chat_maps_response_formats_and_warnings`; `openai_compatible_chat_injects_json_instruction_when_response_format_body_is_disabled`; `vercel_ai_gateway_openai_compatible_generates_text_through_openai_chat`; `vercel_ai_gateway_openai_compatible_streams_text_through_openai_chat`; ignored `live_vercel_ai_gateway_openai_compatible_generate_text`; ignored `live_vercel_ai_gateway_openai_compatible_stream_text` | Mirrors the first `createOpenAICompatible` provider construction boundary plus `/models` discovery and non-streaming and streaming chat `/chat/completions` calls with request shaping, chat provider options, multimodal user messages, assistant reasoning/tool-call history, tool-result messages, provider metadata, Google thought signatures, high-level `generate_text` and `stream_text` tool-loop continuation, function tools/tool choice, response metadata, finish reason, usage, warnings, raw chunks, reasoning, parse errors, non-streaming tool calls, streamed tool calls, and JSON instruction injection when a provider rejects `response_format`. Vercel AI Gateway's OpenAI-compatible text and streaming routes are covered for `openai/...` models. Remaining provider-specific edge cases are tracked in the package row. |
| OpenAI-compatible chat prompt conversion | verified | `crates/ai-sdk-openai-compatible/src/openai_compatible.rs` | `openai_compatible_convert_messages_with_only_a_text_part_to_string_content`; `openai_compatible_convert_messages_with_image_parts`; `openai_compatible_convert_messages_with_image_parts_from_uint8_array`; `openai_compatible_handle_url_based_images`; `openai_compatible_convert_messages_with_audio_wav_parts`; `openai_compatible_convert_messages_with_audio_mp3_parts`; `openai_compatible_convert_messages_with_audio_mpeg_parts_to_mp3_format`; `openai_compatible_throw_error_for_audio_parts_with_urls`; `openai_compatible_throw_error_for_unsupported_audio_format`; `openai_compatible_convert_messages_with_pdf_parts`; `openai_compatible_convert_messages_with_pdf_parts_using_provided_filename`; `openai_compatible_throw_error_for_pdf_parts_with_urls`; `openai_compatible_convert_messages_with_base64_encoded_text_markdown_parts`; `openai_compatible_convert_messages_with_text_plain_parts_from_uint8_array`; `openai_compatible_decode_base64_string_data_for_text_file_parts`; `openai_compatible_convert_text_file_url_to_string`; `openai_compatible_throw_error_for_unsupported_file_types`; `openai_compatible_throw_error_for_file_parts_with_provider_references`; `openai_compatible_stringify_arguments_to_tool_calls`; `openai_compatible_send_empty_string_content_for_assistant_messages_with_no_tool_calls`; `openai_compatible_handle_text_output_type_in_tool_results`; `openai_compatible_merge_system_message_metadata`; `openai_compatible_merge_user_message_content_metadata`; `openai_compatible_prioritize_content_level_metadata_when_merging`; `openai_compatible_handle_tool_calls_with_metadata`; `openai_compatible_handle_image_content_with_metadata`; `openai_compatible_omit_non_openai_compatible_metadata`; `openai_compatible_handle_user_message_with_multiple_content_parts_text_and_image`; `openai_compatible_handle_user_message_with_multiple_text_parts_flattening_disabled`; `openai_compatible_handle_assistant_message_with_text_plus_multiple_tool_calls`; `openai_compatible_handle_single_tool_role_message_with_multiple_tool_result_parts`; `openai_compatible_handle_multiple_content_parts_with_multiple_metadata_layers`; `openai_compatible_handle_different_tool_metadata_vs_message_level_metadata`; `openai_compatible_handle_metadata_collisions_and_overwrites_in_tool_calls`; `openai_compatible_serialize_thought_signature_to_extra_content_for_single_tool_call`; `openai_compatible_handle_sequential_tool_calls_with_separate_signatures`; `openai_compatible_handle_parallel_tool_calls_with_signature_only_on_first_call`; `openai_compatible_not_include_extra_content_when_no_thought_signature_is_present`; `openai_compatible_passes_full_image_png_through_unchanged_for_inline_data`; `openai_compatible_detects_image_subtype_from_inline_bytes_for_top_level_image`; `openai_compatible_passes_through_url_source_for_top_level_only_image`; `openai_compatible_normalizes_image_wildcard_via_detection` | Maps every portable upstream `packages/openai-compatible/src/chat/convert-to-openai-compatible-chat-messages.test.ts` case one-to-one. Rust now has named counterparts for user text flattening, images from base64/bytes/URLs, audio wav/mp3/mpeg plus unsupported audio cases, PDF data/default and explicit filenames plus URL rejection, text file base64/bytes/URL conversion, unsupported files and provider references, assistant tool-call argument stringification, assistant empty text content, text and JSON tool-result content, system/user/content/tool/image metadata merging and provider filtering, multi-part metadata precedence, Google thought-signature `extra_content`, and top-level-only image media-type detection/wildcard normalization. Existing high-level prompt conversion tests remain as additive integration coverage. |
| OpenAI-compatible embeddings | verified | `crates/ai-sdk-openai-compatible`; root facade shim in `src/openai_compatible.rs` | `openai_compatible_embedding_extracts_embedding`; `openai_compatible_embedding_exposes_raw_response_headers`; `openai_compatible_embedding_extracts_usage`; `openai_compatible_embedding_passes_model_and_values`; `openai_compatible_embedding_passes_dimensions_setting`; `openai_compatible_embedding_passes_deprecated_openai_compatible_key_and_warns`; `openai_compatible_embedding_warns_when_raw_provider_name_key_is_used`; `openai_compatible_embedding_does_not_warn_when_camel_case_provider_name_key_is_used`; `openai_compatible_embedding_passes_headers`; `openai_compatible_embedding_model_embeds_through_embed_many`; `openai_compatible_embedding_model_passes_options_and_errors` | Maps every portable upstream `openai-compatible-embedding-model.test.ts` case one-to-one: embeddings, response headers, usage, `model`/`input`/`encoding_format` request body, dimensions through `openaiCompatible`, deprecated `openai-compatible` provider-option key warning, raw provider-name key warning, camelCase provider-name key without warnings, and merged provider/request headers. The existing Rust high-level and API-error regression tests remain as extra coverage. |
| OpenAI-compatible completions | verified | `crates/ai-sdk-openai-compatible`; root facade shim in `src/openai_compatible.rs` | `openai_compatible_completion_generates_text_through_generate_text`; `openai_compatible_completion_streams_text_through_stream_text`; `openai_compatible_completion_passes_options_warnings_and_errors`; see the verified completion non-stream and streaming rows below for the named upstream test map | Mirrors every portable upstream `openai-compatible-completion-language-model.test.ts` case one-to-one for `/completions` config, generate, and SSE stream behavior. Existing Rust high-level completion tests remain as extra coverage for `generate_text`, `stream_text`, unsupported warnings, API error metadata, and integration through the root facade. |
| OpenAI-compatible completion non-stream generation | verified | `crates/ai-sdk-openai-compatible/src/openai_compatible.rs` | `openai_compatible_completion_config_extracts_base_name_from_provider_string`; `openai_compatible_completion_config_handles_provider_without_dot_notation`; `openai_compatible_completion_config_returns_empty_for_empty_provider`; `openai_compatible_completion_extracts_text_response`; `openai_compatible_completion_extracts_usage`; `openai_compatible_completion_sends_request_body`; `openai_compatible_completion_sends_additional_response_information`; `openai_compatible_completion_extracts_finish_reason`; `openai_compatible_completion_supports_unknown_finish_reason`; `openai_compatible_completion_exposes_raw_response_headers`; `openai_compatible_completion_passes_model_and_prompt`; `openai_compatible_completion_passes_headers`; `openai_compatible_completion_includes_provider_specific_options`; `openai_compatible_completion_omits_provider_specific_options_for_different_provider`; `openai_compatible_completion_accepts_camel_case_provider_options_key_for_hyphenated_provider_name`; `openai_compatible_completion_prefers_camel_case_options_over_raw_name_options`; `openai_compatible_completion_warns_when_raw_provider_options_key_is_used`; `openai_compatible_completion_does_not_warn_when_camel_case_provider_options_key_is_used` | Maps upstream `openai-compatible-completion-language-model.test.ts` `config` and non-stream `doGenerate` cases one-to-one: provider option name extraction, text/usage/response metadata/finish reason handling, request body and headers, provider-specific option inclusion/exclusion, camelCase precedence for hyphenated provider names, and deprecated raw-key warnings. |
| OpenAI-compatible completion streaming | verified | `crates/ai-sdk-openai-compatible/src/openai_compatible.rs` | `openai_compatible_completion_streams_text_deltas`; `openai_compatible_completion_stream_handles_error_stream_parts`; `openai_compatible_completion_stream_handles_unparsable_stream_parts`; `openai_compatible_completion_stream_sends_request_body`; `openai_compatible_completion_stream_exposes_raw_response_headers`; `openai_compatible_completion_stream_passes_model_and_prompt`; `openai_compatible_completion_stream_passes_headers`; `openai_compatible_completion_stream_includes_provider_specific_options`; `openai_compatible_completion_stream_omits_provider_specific_options_for_different_provider`; `openai_compatible_completion_stream_accepts_camel_case_provider_options_key_for_hyphenated_provider_name`; `openai_compatible_completion_stream_prefers_camel_case_options_over_raw_name_options` | Maps upstream `openai-compatible-completion-language-model.test.ts` `doStream` cases one-to-one: text deltas, provider error stream parts, unparsable chunks, stream request body, raw response headers, model/prompt request serialization, provider/request headers, provider-specific option inclusion/exclusion, camelCase provider options for hyphenated provider names, and raw-plus-camel precedence. |
| OpenAI-compatible images | verified | `crates/ai-sdk-openai-compatible`; root facade shim in `src/openai_compatible.rs` | `openai_compatible_image_constructor_exposes_provider_and_model_information`; `openai_compatible_image_generate_passes_correct_parameters`; `openai_compatible_image_uses_provider_name_from_config_for_provider_options_key`; `openai_compatible_image_emits_deprecated_warning_for_raw_hyphenated_provider_options_key`; `openai_compatible_image_does_not_warn_for_camel_case_provider_options_key`; `openai_compatible_image_adds_warnings_for_unsupported_settings`; `openai_compatible_image_passes_headers`; `openai_compatible_image_handles_api_errors_with_custom_error_structure`; `openai_compatible_image_handles_api_errors_with_default_error_structure`; `openai_compatible_image_returns_raw_b64_json_content`; `openai_compatible_image_response_metadata_includes_timestamp_headers_and_model_id`; `openai_compatible_image_uses_real_date_when_no_custom_date_provider_is_specified`; `openai_compatible_image_passes_user_setting_in_request`; `openai_compatible_image_omits_user_field_when_not_set_via_provider_options`; `openai_compatible_image_edit_sends_request_with_files`; `openai_compatible_image_edit_sends_request_with_files_and_mask`; `openai_compatible_image_edit_sends_request_with_uint8_array_data`; `openai_compatible_image_edit_sends_request_with_multiple_images`; `openai_compatible_image_edit_response_metadata_includes_timestamp_headers_and_model_id`; additive root facade coverage `openai_compatible_image_model_generates_through_generate_image`; `openai_compatible_image_model_edits_with_files_and_mask`; `openai_compatible_image_model_passes_options_warnings_and_errors` | Mirrors every portable upstream `openai-compatible-image-model.test.ts` case one-to-one for constructor metadata, `/images/generations` JSON request shape, provider-name option key selection, raw/camel-case hyphenated provider-option warnings, unsupported aspect-ratio/seed warnings, request headers, custom/default API error message extraction, raw `b64_json` image content, response metadata timestamps/headers/model id, provider-option `user`, omitted `user`, `/images/edits` form-data requests with files/mask/byte data/multiple images, and edit response metadata. Rust keeps dependency-free multipart `FormData` entries and exposes an injected timestamp provider instead of upstream's private `_internal.currentDate`; those are Rust equivalents of the same portable behavior. |
| OpenAI-compatible chat config provider-options name | verified | `crates/ai-sdk-openai-compatible/src/openai_compatible.rs` | `openai_compatible_chat_config_extracts_base_name_from_provider_string`; `openai_compatible_chat_config_handles_provider_without_dot_notation`; `openai_compatible_chat_config_returns_empty_for_empty_provider` | Maps upstream `openai-compatible-chat-language-model.test.ts` `config` cases one-to-one. Rust centralizes the provider-options name extraction used by chat, completion, embedding, and image provider-option handling: provider ids split at the first dot, provider ids without dot notation are preserved, and empty provider ids stay empty. |
| OpenAI-compatible provider option key normalization | verified | `crates/ai-sdk-openai-compatible`; root facade shim in `src/openai_compatible.rs` | `to_camel_case_upstream_should_convert_hyphenated_names_to_camel_case`; `to_camel_case_upstream_should_convert_underscored_names_to_camel_case`; `to_camel_case_upstream_should_handle_multiple_separators`; `to_camel_case_upstream_should_return_same_string_when_already_camel_case`; `to_camel_case_upstream_should_return_same_string_when_no_separators`; `to_camel_case_upstream_should_handle_empty_string`; `resolve_provider_options_key_upstream_should_return_camel_case_key_when_camel_case_options_present`; `resolve_provider_options_key_upstream_should_return_raw_key_when_only_raw_options_present`; `resolve_provider_options_key_upstream_should_return_camel_case_key_when_both_are_present`; `resolve_provider_options_key_upstream_should_return_raw_key_when_no_options_are_present`; `resolve_provider_options_key_upstream_should_return_raw_key_when_provider_options_is_undefined`; `resolve_provider_options_key_upstream_should_return_raw_key_when_name_has_no_separators`; `deprecated_provider_options_key_upstream_should_push_warning_when_raw_key_is_used_and_differs`; `deprecated_provider_options_key_upstream_should_not_warn_when_only_camel_case_key_is_used`; `deprecated_provider_options_key_upstream_should_not_warn_when_raw_name_is_already_camel_case`; `deprecated_provider_options_key_upstream_should_not_warn_when_raw_key_is_not_present`; `deprecated_provider_options_key_upstream_should_not_warn_when_provider_options_is_undefined` | Mirrors every portable upstream `packages/openai-compatible/src/utils/to-camel-case.test.ts` case one-to-one: camelCase conversion for hyphen, underscore, multiple separators, already-camel, no-separator, and empty strings; raw-versus-camel provider-option key resolution; deprecated raw-key warnings; and raw-plus-camel precedence across chat, completion, embedding, and image provider-option paths. |
| Open Responses LMStudio basic stream fixture | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`, `crates/ai-sdk-open-responses/src/fixtures/lmstudio-basic.1.chunks.txt` | `open_responses_provider_streams_lmstudio_basic_content` | Maps upstream `open-responses-language-model.test.ts` `doStream > basic generation > should stream content` one-to-one using the original `lmstudio-basic.1` stream fixture. The Rust test asserts the exact streamed text-delta sequence and completed text, stream-start warnings, text start/end ids, stop finish reason, usage/cache token mapping, raw usage retention, and `stream: true` request body in the package-owned Open Responses crate. |
| Open Responses LMStudio tool-call fixtures | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_parses_lmstudio_tool_call_from_response`; `open_responses_provider_returns_lmstudio_tool_calls_finish_reason`; `open_responses_provider_extracts_lmstudio_tool_call_usage`; `open_responses_provider_streams_lmstudio_tool_call_fixture` | Mirrors upstream `open-responses-language-model.test.ts` `doGenerate > tool call parsing` cases one-to-one for `lmstudio-tool-call.1`: tool-call content parsing, `tool-calls` finish reason, and usage/cache/reasoning-token mapping each have a dedicated Rust counterpart. The streaming `lmstudio-tool-call.2` fixture remains covered for streamed reasoning/text/tool-call output, LMStudio item metadata, and normal `reasoning_text` stream ids. |
| Open Responses upstream generated function tool calls | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_generates_upstream_function_tool_calls`; `open_responses_provider_sets_tool_calls_finish_reason_for_function_calls`; `open_responses_provider_preserves_namespace_on_function_call_output`; `open_responses_provider_omits_namespace_on_function_call_when_absent`; `open_responses_provider_maps_allowed_tools_to_tool_choice`; `open_responses_provider_maps_allowed_tools_required_mode`; `open_responses_provider_allowed_tools_overrides_request_tool_choice` | Maps upstream OpenAI Responses non-streaming function-tool tests: multiple function-call content parts preserve call ids, names, arguments, item metadata, and absent namespace state; function-call outputs set `tool-calls` finish reason; OpenAI namespace metadata is included only when upstream sends it; `allowedTools` maps to `tool_choice.allowed_tools`, preserves the full tools list, supports required mode, and overrides request-level tool choice as a separate upstream test case. |
| Open Responses upstream streamed incomplete/tool-call/service-tier edge cases | verified | `crates/ai-sdk-open-responses/src/open_responses.rs` | `open_responses_provider_streams_upstream_incomplete_response_finish_reason`; `open_responses_provider_streams_upstream_tool_calls`; `open_responses_provider_preserves_namespace_on_streaming_function_call_output`; `open_responses_provider_omits_namespace_on_streaming_function_call_when_absent`; `open_responses_provider_streams_upstream_service_tier` | Maps upstream `should send finish reason for incomplete response`, `should send streaming tool calls`, `should preserve namespace on streaming function_call output`, `should not set namespace on streaming function_call when absent`, and `Should handle service tier`. The Rust tests preserve provisional streamed ids for text/tool deltas, use final done ids for `text-end` and `tool-input-end`, dedupe completed response tool-call payloads by item id even when OpenAI changes `call_id`, keep OpenAI provider metadata under the `openai` key, retain namespace metadata only when present, emit empty reasoning start/end metadata for the service-tier stream, and assert length/tool-calls/stop finish reasons plus usage and service-tier metadata. |
| Open Responses upstream error fixtures | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`, `crates/ai-sdk-open-responses/src/fixtures/openai-error.1.json`, `crates/ai-sdk-open-responses/src/fixtures/openai-error.1.chunks.txt` | `open_responses_provider_generates_error_fixture_result`; `open_responses_provider_maps_generate_error_fixture_like_upstream_throw_case`; `open_responses_provider_streams_error_fixture_parts`; `open_responses_provider_streams_openai_error_event_without_synthetic_message`; `open_responses_provider_streams_failed_response_incomplete_details_finish_reason`; `open_responses_provider_stream_failed_response_sets_raw_reason_and_usage` | Mirrors upstream `openai-error.1` non-streaming and streaming fixtures. Rust maps the upstream `should throw an error` generate case into the Rust provider trait's error-result shape: empty content, `FinishReason::Error`, the same error message in provider metadata, and the parsed response body retained. It also preserves streamed error events without synthetic text, maps upstream `response.failed` incomplete details such as `max_output_tokens` to length finish reasons even after an error event, and still exposes `response.failed` raw error codes when no incomplete reason is present. |
| OpenAI Responses current upstream corpus audit | verified | `crates/ai-sdk-open-responses/src/open_responses.rs`; current upstream `packages/openai/src/responses/**/*.test.ts` | 523 `ai-sdk-open-responses` tests; detailed OpenAI Responses rows above and below | The 2026-05-22 refreshed `npx opensrc@latest path github:vercel/ai` inventory found 322 explicit current upstream `it`/`test` cases across `convert-to-openai-responses-input.test.ts`, `openai-responses-api.test.ts`, `openai-responses-language-model.test.ts`, and `openai-responses-prepare-tools.test.ts`; the four `it.each` reasoning/provider-option matrices are represented as dedicated Rust test cases/macros per model id or effort value. The package-owned Rust crate lists 523 tests and now has named counterparts for every portable OpenAI Responses prompt-conversion, schema-alignment, prepare-tools, provider-option, hosted-tool, fixture, streaming, annotation, error, reasoning, and tool-history case in the current upstream corpus. No JavaScript-only exception is needed for this Responses subcorpus; broader `packages/openai` files, skills, speech, transcription, and non-Responses package surfaces remain tracked by the main OpenAI row. |
| OpenAI-compatible prepareTools request mapping | verified | `crates/ai-sdk-openai-compatible/src/openai_compatible.rs` | `openai_compatible_prepare_tools_returns_undefined_tools_and_tool_choice_when_tools_are_null`; `openai_compatible_prepare_tools_returns_undefined_tools_and_tool_choice_when_tools_are_empty`; `openai_compatible_prepare_tools_prepares_function_tools`; `openai_compatible_prepare_tools_warns_for_unsupported_provider_defined_tools`; `openai_compatible_prepare_tools_handles_auto_tool_choice`; `openai_compatible_prepare_tools_handles_required_tool_choice`; `openai_compatible_prepare_tools_handles_none_tool_choice`; `openai_compatible_prepare_tools_handles_specific_tool_choice`; `openai_compatible_prepare_tools_passes_through_strict_true`; `openai_compatible_prepare_tools_passes_through_strict_false`; `openai_compatible_prepare_tools_omits_undefined_strict`; `openai_compatible_prepare_tools_passes_mixed_strict_settings` | Maps every portable upstream `packages/openai-compatible/src/chat/openai-compatible-prepare-tools.test.ts` case one-to-one: absent and empty tools omit `tools` and `tool_choice`, function tools serialize names/descriptions/input schemas, unsupported provider-defined tools are omitted with an upstream-shaped warning, `auto`/`required`/`none`/specific tool choices serialize to OpenAI-compatible request shape, and strict true/false/omitted/mixed function tools preserve the same serialized `strict` behavior. |
| OpenAI-compatible includeUsage provider setting | verified | `crates/ai-sdk-openai-compatible/src/openai_compatible.rs` | `openai_compatible_provider_passes_include_usage_true_to_created_language_models`; `openai_compatible_provider_passes_include_usage_false_to_created_language_models`; `openai_compatible_provider_passes_unspecified_include_usage_to_created_language_models`; `openai_compatible_provider_passes_include_usage_true_to_all_language_model_streams`; `openai_compatible_provider_omits_include_usage_false_from_all_language_model_streams`; `openai_compatible_provider_omits_unspecified_include_usage_from_all_language_model_streams` | Maps upstream `openai-compatible-provider.test.ts` includeUsage setting cases one-to-one for `true`, `false`, and unspecified provider settings. Rust now has direct config-level counterparts proving chat models, the `language_model` chat alias, and completion models receive the same setting, plus observable stream request-body coverage proving `true` emits `stream_options.include_usage: true` while `false` and unspecified omit `stream_options` like upstream. |
| OpenAI-compatible provider factory and model configuration | verified | `crates/ai-sdk-openai-compatible/src/openai_compatible.rs` | `openai_compatible_provider_creates_provider_with_correct_configuration`; `openai_compatible_provider_creates_headers_without_authorization_when_no_api_key_provided`; `openai_compatible_provider_creates_chat_model_with_correct_configuration`; `openai_compatible_provider_creates_completion_model_with_correct_configuration`; `openai_compatible_provider_creates_embedding_model_with_correct_configuration`; `openai_compatible_provider_uses_language_model_as_default_chat_model_alias`; `openai_compatible_provider_creates_url_without_query_parameters_when_unspecified`; `openai_compatible_provider_passes_structured_outputs_to_chat_and_language_models_only` | Maps the portable upstream `openai-compatible-provider.test.ts` provider construction and model factory cases one-to-one: provider defaults build chat configuration with bearer/custom/user-agent headers and query parameters; missing API keys omit authorization; chat, completion, and embedding factories preserve model ids, provider ids, headers, and URLs; Rust's `language_model` method is the default chat alias counterpart for upstream callable provider usage; URLs omit query strings when no `queryParams` exist; and `supportsStructuredOutputs` is carried only to chat/language models. |
| OpenAI-compatible metadata extractor setting and chat metadata | verified | `crates/ai-sdk-openai-compatible/src/openai_compatible.rs`; `crates/ai-sdk-openai-compatible/src/lib.rs` | `openai_compatible_provider_passes_metadata_extractor_to_chat_model`; `openai_compatible_chat_processes_metadata_from_complete_response`; `openai_compatible_chat_processes_metadata_from_streaming_response` | Maps the upstream `openai-compatible-provider.test.ts` `metadataExtractor` provider-setting case and the portable upstream `openai-compatible-chat-language-model.test.ts` metadata extractor cases one-to-one. Rust exposes `OpenAICompatibleMetadataExtractor` and `OpenAICompatibleStreamMetadataExtractor` callbacks, passes them only to chat/language models, merges complete-response metadata into `provider_metadata`, and processes streaming raw chunks before attaching final metadata to the finish part. |
| OpenAI-compatible chat request body transformation | verified | `crates/ai-sdk-openai-compatible/src/openai_compatible.rs`; `crates/ai-sdk-openai-compatible/src/lib.rs` | `openai_compatible_chat_transforms_request_body_in_do_generate_when_transform_request_body_is_provided`; `openai_compatible_chat_transforms_request_body_in_do_stream_when_transform_request_body_is_provided`; `openai_compatible_chat_works_without_transform_request_body` | Maps upstream `openai-compatible-chat-language-model.test.ts` `transformRequestBody` cases one-to-one. Rust exposes `OpenAICompatibleRequestBodyTransformer` through `OpenAICompatibleProviderSettings::with_transform_request_body`, applies it only to chat/language model generate and stream request bodies after normal OpenAI-compatible body construction, sends the transformed body to the transport, records the transformed body in request metadata, and leaves request bodies unchanged when no transformer is configured. |
| OpenAI-compatible chat non-stream doGenerate basics | verified | `crates/ai-sdk-openai-compatible/src/openai_compatible.rs` | `openai_compatible_chat_extracts_text_content`; `openai_compatible_chat_extracts_tool_call_content`; `openai_compatible_chat_extracts_usage`; `openai_compatible_chat_sends_additional_response_information`; `openai_compatible_chat_exposes_raw_response_headers`; `openai_compatible_chat_does_not_apply_xai_user_setting_to_test_provider_request`; `openai_compatible_chat_ignores_reasoning_field_when_reasoning_content_is_not_provided`; `openai_compatible_chat_prefers_reasoning_content_over_reasoning_field`; `openai_compatible_chat_supports_partial_usage`; `openai_compatible_chat_supports_unknown_finish_reason`; `openai_compatible_chat_passes_model_and_messages`; `openai_compatible_chat_passes_settings`; `openai_compatible_chat_passes_settings_with_deprecated_key_and_emits_warning`; `openai_compatible_chat_includes_provider_specific_options`; `openai_compatible_chat_does_not_include_provider_specific_options_for_different_provider`; `openai_compatible_chat_passes_headers` | Maps upstream `openai-compatible-chat-language-model.test.ts` non-stream `doGenerate` basics one-to-one for text/tool-call content, usage and response metadata, raw response headers, mismatched-provider `xai` user options, `reasoning_content` precedence over `reasoning`, partial usage, unknown finish reasons, model/message request body, global and deprecated OpenAI-compatible settings, provider-specific option filtering, and merged provider/request headers. Rust uses unsigned token usage, so the xAI-style fixture case with reasoning tokens above completion tokens saturates text output tokens at `0` while preserving the raw usage object. |
| OpenAI-compatible chat response format and GPT-5 request options | verified | `crates/ai-sdk-openai-compatible/src/openai_compatible.rs` | `openai_compatible_chat_omits_response_format_when_response_format_is_text`; `openai_compatible_chat_forwards_json_response_format_as_json_object_without_schema`; `openai_compatible_chat_omits_json_schema_when_structured_outputs_disabled`; `openai_compatible_chat_includes_json_schema_when_structured_outputs_enabled`; `openai_compatible_chat_passes_reasoning_effort_from_provider_options`; `openai_compatible_chat_does_not_duplicate_reasoning_effort_in_request_body`; `openai_compatible_chat_passes_top_level_reasoning_as_reasoning_effort`; `openai_compatible_chat_omits_top_level_reasoning_none_as_reasoning_effort`; `openai_compatible_chat_prefers_provider_options_reasoning_effort`; `openai_compatible_chat_passes_text_verbosity_from_provider_options`; `openai_compatible_chat_does_not_duplicate_text_verbosity_in_request_body`; `openai_compatible_chat_uses_json_schema_and_strict_for_structured_outputs`; `openai_compatible_chat_sets_json_schema_name_and_description`; `openai_compatible_chat_sends_strict_false_when_strict_json_schema_disabled`; `openai_compatible_chat_allows_undefined_schema_with_structured_outputs` | Maps upstream `openai-compatible-chat-language-model.test.ts` non-stream `doGenerate > response format` cases one-to-one. Rust proves text formats omit `response_format`, JSON without schema uses `json_object`, schemas are omitted with the upstream unsupported warning when `structuredOutputs` is disabled, schemas use `json_schema` with default `name: "response"` and `strict: true` when enabled, custom JSON schema names/descriptions are preserved, `strictJsonSchema: false` maps to `strict: false`, undefined schemas still use `json_object`, provider `reasoningEffort` maps to `reasoning_effort` without duplicating the original provider option, top-level reasoning maps except for `none`, provider options override top-level reasoning, and provider `textVerbosity` maps to `verbosity` without duplicating the original option while preserving custom body options. |
| OpenAI-compatible chat returned request body and raw chunks | verified | `crates/ai-sdk-openai-compatible/src/openai_compatible.rs` | `openai_compatible_chat_sends_request_body`; `openai_compatible_chat_stream_includes_raw_chunks_when_include_raw_chunks_true` | Maps upstream `openai-compatible-chat-language-model.test.ts` non-stream `doGenerate` `should send request body` and `raw chunks` `should include raw chunks when includeRawChunks is true` cases one-to-one. Rust records the returned `LanguageModelRequest.body` for non-streaming calls and verifies the raw stream sequence: start, raw content chunk, response metadata, text start/delta, raw finish chunk, text end, and finish metadata with the upstream stop reason and empty provider metadata. |
| OpenAI-compatible chat streaming doStream basics | verified | `crates/ai-sdk-openai-compatible/src/openai_compatible.rs` | `openai_compatible_chat_stream_streams_text_content`; `openai_compatible_chat_stream_exposes_raw_response_headers`; `openai_compatible_chat_stream_respects_include_usage_option`; `openai_compatible_chat_streams_reasoning_content_before_text_deltas`; `openai_compatible_chat_streams_reasoning_from_reasoning_field_when_reasoning_content_missing`; `openai_compatible_chat_stream_prefers_reasoning_content_over_reasoning_field`; `openai_compatible_chat_stream_handles_error_stream_parts`; `openai_compatible_chat_stream_handles_unparsable_stream_parts`; `openai_compatible_chat_stream_passes_messages_and_model`; `openai_compatible_chat_stream_sends_request_body`; `openai_compatible_chat_stream_passes_headers`; `openai_compatible_chat_stream_includes_provider_specific_options`; `openai_compatible_chat_stream_does_not_include_provider_specific_options_for_different_provider` | Maps upstream `openai-compatible-chat-language-model.test.ts` streaming `doStream` basics one-to-one for text streaming, raw response headers, provider-level `includeUsage`, reasoning-content ordering, streaming `reasoning` fallback, reasoning-content precedence, error and unparsable stream parts, model/message and full stream request body, merged provider/request headers, and provider-specific option filtering. Dedicated streaming tool-call delta coverage is tracked in the row below. |
| OpenAI-compatible chat streaming fixture snapshots | verified | `crates/ai-sdk-openai-compatible/src/openai_compatible.rs`; upstream chunk fixtures copied to `crates/ai-sdk-openai-compatible/src/fixtures` | `openai_compatible_chat_streams_xai_text_fixture_content`; `openai_compatible_chat_streams_xai_tool_call_fixture_content` | Maps upstream `openai-compatible-chat-language-model.test.ts` `doStream > text (fixture) > should stream text content` and `doStream > tool call (fixture) > should stream tool call content` snapshot cases one-to-one against the original `xai-text.chunks.txt` and `xai-tool-call.chunks.txt` streams. Rust asserts the full fixture line inventory, response metadata, reasoning delta counts and text, streamed text/tool-call output, finish reason, usage, and raw usage metadata while keeping `includeRawChunks: false` behavior. |
| OpenAI-compatible chat streaming tool-call deltas | verified | `crates/ai-sdk-openai-compatible/src/openai_compatible.rs` | `openai_compatible_chat_streams_tool_deltas`; `openai_compatible_chat_streams_tool_deltas_when_function_name_arrives_later`; `openai_compatible_chat_stream_errors_when_tool_call_never_receives_function_name`; `openai_compatible_chat_streams_tool_call_with_thought_signature_from_extra_content`; `openai_compatible_chat_streams_parallel_tool_calls_with_signature_only_on_first_call`; `openai_compatible_chat_streams_tool_call_deltas_when_arguments_are_in_first_chunk`; `openai_compatible_chat_stream_does_not_duplicate_tool_calls_after_completed_empty_chunk`; `openai_compatible_chat_streams_tool_call_sent_in_one_chunk`; `openai_compatible_chat_streams_empty_tool_call_sent_in_one_chunk` | Maps upstream `openai-compatible-chat-language-model.test.ts` streaming tool-call delta cases one-to-one: normal argument deltas, late `function.name`, missing `function.name` errors, Google thought signatures from `extra_content`, parallel call metadata isolation, arguments arriving in the first chunk, duplicate empty post-completion chunks, one-chunk tool calls, and empty one-chunk tool calls. Rust represents the upstream rejected stream case as a `LanguageModelStreamPart::Error` plus error finish part because the package-owned Rust stream API is eager rather than a rejecting JavaScript `ReadableStream`. |
| OpenAI-compatible chat non-stream usage details | verified | `crates/ai-sdk-openai-compatible/src/openai_compatible.rs` | `openai_compatible_chat_extracts_detailed_token_usage_when_available`; `openai_compatible_chat_handles_missing_token_details`; `openai_compatible_chat_handles_partial_token_details`; `openai_compatible_chat_preserves_extra_usage_fields_from_provider_specific_responses` | Maps upstream `openai-compatible-chat-language-model.test.ts` non-stream `usage details` cases one-to-one. Rust maps prompt/completion totals, cached prompt tokens, reasoning output tokens, text output tokens, accepted/rejected prediction token provider metadata, missing and partial token-detail shapes, and preserves provider-specific raw usage fields such as queue/prompt/completion/total timing. When token details are absent, chat usage now defaults `cache_read` and `reasoning` to `0`, maps `no_cache` to the full prompt total, and retains the provider metadata key as upstream's empty metadata object. |
| OpenAI-compatible chat streaming usage details | verified | `crates/ai-sdk-openai-compatible/src/openai_compatible.rs` | `openai_compatible_chat_stream_extracts_detailed_token_usage_from_stream_finish`; `openai_compatible_chat_stream_handles_missing_token_details_in_stream`; `openai_compatible_chat_stream_handles_partial_token_details_in_stream` | Maps upstream `openai-compatible-chat-language-model.test.ts` `doStream > usage details in streaming` cases one-to-one. Rust parses the final streamed usage block into prompt/completion totals, cache-read/no-cache input tokens, reasoning/text output tokens, raw usage retention, accepted/rejected prediction token provider metadata, and the upstream empty provider-metadata object for missing or partial token-detail cases. |
| OpenAI-compatible chat non-stream thought signatures | verified | `crates/ai-sdk-openai-compatible/src/openai_compatible.rs` | `openai_compatible_chat_parses_thought_signature_from_extra_content_and_includes_provider_metadata`; `openai_compatible_chat_handles_parallel_tool_calls_with_signature_only_on_first_call`; `openai_compatible_chat_does_not_include_provider_metadata_when_no_thought_signature_is_present` | Maps upstream `openai-compatible-chat-language-model.test.ts` non-stream `doGenerate > Google Gemini thought signatures (OpenAI compatibility)` cases one-to-one. Rust verifies `extra_content.google.thought_signature` is exposed as `provider_metadata.test-provider.thoughtSignature` on generated tool-call content, parallel tool calls attach metadata only to the signed call, and unsigned tool calls omit provider metadata entirely. |
| OpenAI-compatible chat provider-option metadata keys | verified | `crates/ai-sdk-openai-compatible/src/openai_compatible.rs` | `openai_compatible_chat_accepts_camel_case_provider_options_key_for_hyphenated_provider_name`; `openai_compatible_chat_prefers_camel_case_options_over_raw_name_options`; `openai_compatible_chat_uses_camel_case_metadata_key_when_camel_case_provider_options_are_used`; `openai_compatible_chat_uses_raw_metadata_key_when_raw_provider_options_are_used`; `openai_compatible_chat_emits_deprecated_warning_when_raw_provider_options_key_is_used`; `openai_compatible_chat_does_not_warn_when_camel_case_provider_options_key_is_used`; `openai_compatible_chat_uses_raw_metadata_key_when_no_provider_options_are_passed`; `openai_compatible_chat_includes_thought_signature_in_provider_metadata_with_camel_case_key`; `openai_compatible_chat_includes_thought_signature_in_provider_metadata_with_raw_key` | Maps upstream `openai-compatible-chat-language-model.test.ts` non-stream `doGenerate` provider-option metadata key cases one-to-one. Rust now accepts camelCase provider option keys for hyphenated provider names, prefers camelCase over raw keys, emits deprecated raw-key warnings only for raw hyphenated keys, selects the provider metadata key from the request's resolved provider option key, falls back to the raw provider name when no provider options are passed, and stores Google thought signatures under the selected metadata key. |
| OpenAI-compatible chat streaming provider-option metadata keys | verified | `crates/ai-sdk-openai-compatible/src/openai_compatible.rs` | `openai_compatible_chat_stream_accepts_camel_case_provider_options_key_for_hyphenated_provider_name`; `openai_compatible_chat_stream_prefers_camel_case_options_over_raw_name_options`; `openai_compatible_chat_stream_emits_deprecated_warning_when_raw_provider_options_key_is_used`; `openai_compatible_chat_stream_does_not_warn_when_camel_case_provider_options_key_is_used`; `openai_compatible_chat_stream_uses_camel_case_metadata_key_in_finish_event_when_camel_case_options_are_used`; `openai_compatible_chat_stream_uses_raw_metadata_key_in_finish_event_when_raw_options_are_used`; `openai_compatible_chat_stream_uses_raw_metadata_key_in_finish_event_when_no_provider_options_are_passed`; `openai_compatible_chat_stream_uses_camel_case_metadata_key_for_thought_signatures_in_streamed_tool_calls` | Maps upstream `openai-compatible-chat-language-model.test.ts` `doStream` camelCase/raw provider-option cases one-to-one. Rust now verifies streaming request-body option merging, camel-over-raw precedence, stream-start warning behavior, finish-event provider metadata key selection for camel/raw/no options, and streamed tool-call thought signatures under the selected camelCase metadata key. |
| OpenAI-compatible model discovery and retrieval | verified | `crates/ai-sdk-openai-compatible`; root facade shim in `src/openai_compatible.rs`; Vercel AI Gateway integration through `crates/ai-sdk-gateway/src/vercel_ai_gateway.rs` with root tests in `src/vercel_ai_gateway.rs` | `openai_compatible_provider_lists_models`; `openai_compatible_provider_retrieves_model_by_id`; `vercel_ai_gateway_openai_compatible_lists_models`; `vercel_ai_gateway_openai_compatible_retrieves_model`; ignored `live_vercel_ai_gateway_openai_compatible_list_models`; ignored `live_vercel_ai_gateway_openai_compatible_retrieve_model` | Covers OpenAI-compatible `GET /models` and `GET /models/{model}` for provider-qualified Gateway model ids, including URL-encoding ids such as `openai/gpt-4.1-mini`, custom headers, query parameters, typed Gateway metadata fields (`name`, `description`, `released`, `context_window`/`contextWindow`, `max_tokens`/`maxTokens`, `type`/`modelType`, `tags`, `pricing`), metadata flattening for unknown fields, and optional `.env.local` live validation. |
| Vercel AI Gateway OpenAI-compatible text, objects, embeddings, images, and model discovery | in-progress | `crates/ai-sdk-gateway/src/vercel_ai_gateway.rs`, root facade shim and high-level integration tests in `src/vercel_ai_gateway.rs`, `crates/ai-sdk-openai-compatible`, root facade shim in `src/openai_compatible.rs` | `vercel_ai_gateway_openai_compatible_factory_uses_default_base_url`; `vercel_ai_gateway_openai_compatible_implements_provider_trait`; `vercel_ai_gateway_openai_compatible_auth_token_matches_gateway_precedence`; `vercel_ai_gateway_openai_compatible_generates_text_through_openai_chat`; `vercel_ai_gateway_openai_compatible_passes_gateway_provider_options_through_openai_chat`; `vercel_ai_gateway_openai_compatible_generates_object_through_openai_chat`; `vercel_ai_gateway_openai_compatible_streams_object_through_openai_chat`; `vercel_ai_gateway_openai_compatible_runs_generate_text_tool_loop_end_to_end`; `vercel_ai_gateway_openai_compatible_streams_text_through_openai_chat`; `vercel_ai_gateway_openai_compatible_runs_stream_text_tool_loop_end_to_end`; `vercel_ai_gateway_openai_compatible_embeds_through_openai_embeddings`; `vercel_ai_gateway_openai_compatible_generates_images_through_openai_images_endpoint`; `vercel_ai_gateway_openai_compatible_maps_chat_image_outputs_through_generate_text`; `vercel_ai_gateway_openai_compatible_streams_chat_image_outputs_through_stream_text`; `vercel_ai_gateway_openai_compatible_lists_models`; `openai_compatible_provider_lists_models`; `openai_compatible_chat_injects_json_instruction_when_response_format_body_is_disabled`; `openai_compatible_chat_passes_tools_tool_choice_and_provider_options`; `openai_compatible_chat_converts_multimodal_user_messages`; `openai_compatible_chat_rejects_unsupported_file_messages_before_transport`; `openai_compatible_chat_converts_assistant_tool_history`; `openai_compatible_chat_runs_generate_text_tool_loop_end_to_end`; `openai_compatible_chat_runs_stream_text_tool_loop_end_to_end`; `openai_compatible_chat_maps_tool_calls_from_generate`; `openai_compatible_chat_streams_tool_calls`; ignored `live_vercel_ai_gateway_openai_compatible_generate_text`; ignored `live_vercel_ai_gateway_openai_compatible_generate_text_with_otel`; ignored `live_vercel_ai_gateway_openai_compatible_stream_text_with_otel`; ignored `live_vercel_ai_gateway_openai_compatible_generate_object_with_otel`; ignored `live_vercel_ai_gateway_openai_compatible_stream_object_with_otel`; ignored `live_vercel_ai_gateway_openai_compatible_generate_text_tool_loop`; ignored `live_vercel_ai_gateway_openai_compatible_generate_object`; ignored `live_vercel_ai_gateway_openai_compatible_stream_text`; ignored `live_vercel_ai_gateway_openai_compatible_stream_object`; ignored `live_vercel_ai_gateway_openai_compatible_embed`; ignored `live_vercel_ai_gateway_openai_compatible_generate_image`; ignored `live_vercel_ai_gateway_openai_compatible_generate_text_with_image_output`; ignored `live_vercel_ai_gateway_openai_compatible_list_models` | Thin Rust provider factory over `https://ai-gateway.vercel.sh/v1` now lives in the matching `ai-sdk-gateway` crate and proves `generate_text`, `generate_object`, `stream_object`, local tool-loop continuation, `stream_text`, streaming local tool-loop continuation, `embed`/`embed_many`, `generate_image`, and `list_models` can call Gateway OpenAI-compatible `/chat/completions`, `/embeddings`, `/images/generations`, and `/models` with provider-qualified model ids. The wrapper implements the provider-v4 trait for registry/middleware lookup of language, embedding, and image models, supports explicit/API-key/OIDC auth precedence, and maps typed `GatewayProviderOptions` into OpenAI-compatible request `providerOptions.gateway` for routing, fallbacks, compliance, and provider timeouts. The Gateway OpenAI-compatible wrapper omits the unsupported `response_format` body field for object calls and uses shared prompt JSON/schema instruction injection instead; the restored live object and stream-object tests passed against `.env.local` on `2026-05-18`. The ignored live telemetry tests pair real Gateway OpenAI-compatible `generate_text`, `stream_text`, `generate_object`, and `stream_object` calls with the root OTel integration and loopback OTLP receiver, asserting emitted wire payloads without printing credentials; all four passed against `.env.local` on `2026-05-20`. The shared OpenAI-compatible chat transport now covers tools, tool choice, chat provider options, streamed tool calls, multimodal user prompt parts, assistant/tool history, high-level generate, object, stream-object, and stream tool-loop continuations, provider metadata, typed model-list discovery, image generation, and chat image-output file mapping used by the Gateway OpenAI-compatible route. Broader Gateway OpenAI-compatible endpoint coverage can expand from this slice. |
| Vercel AI Gateway OpenAI Responses API | in-progress | `crates/ai-sdk-gateway/src/vercel_ai_gateway.rs`, root facade shim and high-level integration tests in `src/vercel_ai_gateway.rs`, `crates/ai-sdk-open-responses` | `vercel_ai_gateway_openai_responses_generates_text`; `vercel_ai_gateway_openai_responses_converts_file_prompt_parts`; `vercel_ai_gateway_openai_responses_generates_object`; `vercel_ai_gateway_openai_responses_streams_text`; `vercel_ai_gateway_openai_responses_streams_object`; `vercel_ai_gateway_openai_responses_streams_file_prompt_parts`; `vercel_ai_gateway_openai_responses_runs_generate_text_tool_loop_end_to_end`; `vercel_ai_gateway_openai_responses_runs_stream_text_tool_loop_end_to_end`; `vercel_ai_gateway_openai_responses_prepares_openai_hosted_tools`; `vercel_ai_gateway_openai_responses_passes_gateway_provider_options`; `vercel_ai_gateway_openai_responses_maps_api_error_data_to_gateway_metadata_key`; ignored `live_vercel_ai_gateway_openai_responses_generate_text`; ignored `live_vercel_ai_gateway_openai_responses_stream_text`; ignored `live_vercel_ai_gateway_openai_responses_generate_object`; ignored `live_vercel_ai_gateway_openai_responses_stream_object`; ignored `live_vercel_ai_gateway_openai_responses_generate_text_tool_loop`; ignored `live_vercel_ai_gateway_openai_responses_stream_text_tool_loop`; ignored `live_vercel_ai_gateway_openai_responses_generate_text_with_otel`; ignored `live_vercel_ai_gateway_openai_responses_stream_text_with_otel` | Exposes Gateway's documented OpenAI Responses API at `https://ai-gateway.vercel.sh/v1/responses` from the matching `ai-sdk-gateway` crate by reusing the Rust Open Responses provider with `provider/model` ids. The slice covers high-level `generate_text`, `generate_object`, `stream_text`, `stream_object`, and non-streaming plus streamed local tool-loop continuation through `/responses`, request headers/body shape including URL and inline image/file prompt parts, streaming URL file prompt parts with `input_file.file_url`, JSON schema response formats for generated and streamed objects, function tools, OpenAI hosted/provider-defined tool request preparation, hosted tool-choice name mapping, and `stream: true`, standard response id metadata, stream response headers, usage, Gateway-keyed API error metadata, the `vercel_ai_gateway_openai_responses` helper, typed `GatewayProviderOptions` mapping into `providerOptions.gateway`, provider-specific OpenResponses body passthrough such as `caching: "auto"` and `vercelAiGateway.store=false`, and optional `.env.local` live validation; the live Responses stream test passed on `2026-05-18`, and the live Responses object, stream-object, generate-text tool-loop, and stream-text tool-loop tests passed on `2026-05-20`. The ignored live Responses telemetry tests pair real Gateway Responses `generate_text` and `stream_text` calls with the root OTel integration and loopback OTLP receiver, asserting exported wire payloads without printing credentials; both passed against `.env.local` on `2026-05-20`. The deterministic tool-loop tests keep upstream-compatible stored `item_reference` continuation coverage, while the live Gateway tool-loop tests set `store=false` because current Gateway `/v1/responses` rejects stored item-reference continuation with `input.1.output: Invalid input` but accepts full `function_call` replay. Broader Open Responses tools/structured-output gaps remain tracked in the Open Responses package row. |
| Vercel AI Gateway OpenAI-compatible prompt conversion | verified | `crates/ai-sdk-gateway/src/vercel_ai_gateway.rs`, root high-level integration tests in `src/vercel_ai_gateway.rs`, `crates/ai-sdk-openai-compatible`, root facade shim in `src/openai_compatible.rs` | `vercel_ai_gateway_openai_compatible_converts_multimodal_and_tool_history`; `openai_compatible_chat_converts_multimodal_user_messages`; `openai_compatible_chat_converts_assistant_tool_history` | Proves the Vercel AI Gateway OpenAI-compatible factory itself sends multimodal user content, OpenAI-compatible metadata, Google thought signatures, assistant reasoning/tool calls, and tool results through the Gateway `/chat/completions` URL/header path. |
| Vercel AI Gateway OpenAI-compatible tool-loop generation and streaming | verified | `crates/ai-sdk-gateway/src/vercel_ai_gateway.rs`, root high-level integration tests in `src/vercel_ai_gateway.rs`, `crates/ai-sdk-openai-compatible`, root facade shim in `src/openai_compatible.rs`, `src/generate_text.rs`, `src/stream_text.rs` | `vercel_ai_gateway_openai_compatible_runs_generate_text_tool_loop_end_to_end`; `vercel_ai_gateway_openai_compatible_runs_stream_text_tool_loop_end_to_end`; ignored `live_vercel_ai_gateway_openai_compatible_generate_text_tool_loop` | Proves the Vercel AI Gateway OpenAI-compatible factory itself drives high-level `generate_text` and `stream_text` through OpenAI tool-call responses, executes the Rust local tool, sends the assistant/tool-result continuation request back to Gateway `/chat/completions`, and reaches final text output. The ignored live non-streaming tool-loop test passed against `.env.local` on `2026-05-18` using an `openai/...` Gateway model. |
| Vercel AI Gateway OpenAI-compatible object generation | verified | `crates/ai-sdk-gateway/src/vercel_ai_gateway.rs`, root high-level integration tests in `src/vercel_ai_gateway.rs`, `crates/ai-sdk-openai-compatible`, root facade shim in `src/openai_compatible.rs`, `src/generate_object.rs` | `vercel_ai_gateway_openai_compatible_generates_object_through_openai_chat`; ignored `live_vercel_ai_gateway_openai_compatible_generate_object`; ignored `live_vercel_ai_gateway_openai_compatible_generate_object_with_otel` | Proves high-level `generate_object` reaches the Vercel AI Gateway OpenAI-compatible `/chat/completions` route, omits Gateway's unsupported `response_format` body field, injects JSON/schema prompt guidance, parses returned JSON text into an object, surfaces usage/response metadata, passes against `.env.local` live Gateway credentials, and exports live object telemetry through the local OTLP receiver; the OTel live test passed on `2026-05-20`. |
| Vercel AI Gateway OpenAI-compatible streamed object generation | verified | `src/vercel_ai_gateway.rs`, `crates/ai-sdk-openai-compatible`, root facade shim in `src/openai_compatible.rs`, `src/stream_object.rs` | `vercel_ai_gateway_openai_compatible_streams_object_through_openai_chat`; ignored `live_vercel_ai_gateway_openai_compatible_stream_object`; ignored `live_vercel_ai_gateway_openai_compatible_stream_object_with_otel` | Proves high-level `stream_object` reaches the Vercel AI Gateway OpenAI-compatible `/chat/completions` SSE route, omits Gateway's unsupported `response_format` body field, injects JSON/schema prompt guidance, collects JSON text deltas, parses the final object, surfaces usage/response metadata, passes against `.env.local` live Gateway credentials, and exports live stream-object telemetry through the local OTLP receiver; the OTel live test passed on `2026-05-20`. |
| Vercel v0 provider package | in-progress | `src/vercel.rs`, `src/openai_compatible.rs` | `vercel_provider_creates_openai_compatible_chat_model`; `vercel_provider_uses_default_base_url_and_function_alias`; `vercel_provider_reports_unsupported_model_families`; `vercel_provider_implements_provider_trait` | Mirrors upstream `createVercel` construction around OpenAI-compatible chat models with default/custom base URLs, headers, `VERCEL_API_KEY`, provider id `vercel.chat`, Vercel-specific user-agent suffix, and unsupported embedding/image lookups. Live v0 API validation remains optional and unported because this goal currently only has AI Gateway credentials. |
| Concrete provider packages | in-progress | `src/openai.rs`, `src/open_responses.rs`, `src/vercel.rs`, `src/vercel_ai_gateway.rs`, `src/deepinfra.rs`, `src/togetherai.rs`, `src/huggingface.rs`, `src/cerebras.rs`, `src/baseten.rs`, `src/voyage.rs`, `crates/ai-sdk-deepseek`, `crates/ai-sdk-lmnt`, `crates/ai-sdk-luma`, `crates/ai-sdk-moonshotai`, `crates/ai-sdk-perplexity`, `crates/ai-sdk-revai`, `crates/ai-sdk-assemblyai`, `crates/ai-sdk-azure`, `crates/ai-sdk-bytedance`, `crates/ai-sdk-mistral`, `crates/ai-sdk-black-forest-labs`, `crates/ai-sdk-hume`, `crates/ai-sdk-deepgram` | OpenAI, Open Responses, Vercel, Vercel AI Gateway, DeepInfra, TogetherAI, Hugging Face, Cerebras, Baseten, Voyage, DeepSeek, LMNT, Luma, MoonshotAI, Perplexity, RevAI, AssemblyAI, Azure, ByteDance, Mistral, Black Forest Labs, Hume, and Deepgram provider-wrapper tests listed above | OpenAI, Open Responses, Vercel, Vercel AI Gateway, DeepInfra, TogetherAI, Hugging Face, Cerebras, Baseten, Voyage, DeepSeek, LMNT, Luma, MoonshotAI, Perplexity, RevAI, AssemblyAI, Azure, ByteDance, Mistral, Black Forest Labs, Hume, and Deepgram have initial Rust provider-wrapper slices. DeepSeek, LMNT, Luma, MoonshotAI, Perplexity, RevAI, AssemblyAI, Azure, ByteDance, Mistral, Black Forest Labs, Hume, and Deepgram are intentionally package-owned crates instead of new root modules, establishing the extraction direction required by the crate-splitting acceptance rule. Most concrete provider package rows above remain unported. |

## Examples Inventory

| Upstream example | Status | Rust path | Evidence | Notes |
| --- | --- | --- | --- | --- |
| Rust kitchen sink equivalent | verified | `examples/kitchen_sink.rs` | `cargo run --example kitchen_sink` is part of validation target when run manually | Rust-only example currently demonstrates deterministic `generate_text` with a tool loop. |
| Rust Vercel AI Gateway text example | verified | `examples/vercel_ai_gateway_text.rs` | `cargo run --example vercel_ai_gateway_text` | Rust-only Gateway example loads `AI_GATEWAY_API_KEY`, `AI_SDK_RUST_AI_GATEWAY_API_KEY`, or `VERCEL_OIDC_TOKEN` from the environment or `.env.local`, uses an `openai/...` Gateway model, and calls high-level `generate_text` end to end without printing credentials. |
| Rust Vercel AI Gateway OpenAI Responses example | verified | `examples/vercel_ai_gateway_responses.rs` | `cargo run --example vercel_ai_gateway_responses` | Rust-only Gateway example loads `AI_GATEWAY_API_KEY`, `AI_SDK_RUST_AI_GATEWAY_API_KEY`, or `VERCEL_OIDC_TOKEN` from the environment or `.env.local`, uses a provider-qualified Gateway model through the OpenAI Responses API endpoint, and calls high-level `generate_text` without printing credentials. |
| Rust Vercel AI Gateway model list example | verified | `examples/vercel_ai_gateway_models.rs` | `cargo run --example vercel_ai_gateway_models` | Rust-only Gateway example calls the OpenAI-compatible `/models` discovery endpoint with optional `AI_GATEWAY_API_KEY`, `AI_SDK_RUST_AI_GATEWAY_API_KEY`, or `VERCEL_OIDC_TOKEN`, retrieves the first listed model through `/models/{model}`, and prints model ids, types, and tags without printing credentials. |
| Rust Vercel AI Gateway image example | verified | `examples/vercel_ai_gateway_image.rs` | `cargo run --example vercel_ai_gateway_image` | Rust-only Gateway example loads `AI_GATEWAY_API_KEY`, `AI_SDK_RUST_AI_GATEWAY_API_KEY`, or `VERCEL_OIDC_TOKEN` from the environment or `.env.local`, uses an image-capable Gateway model through the OpenAI-compatible `/images/generations` endpoint, and prints image counts and base64 lengths without printing image data or credentials. |
| `examples/ai-e2e-next` | not-started | none | none | Needs portable API coverage before Rust equivalent can be planned. |
| `examples/ai-functions` | not-started | none | none | Needs Rust equivalent for AI functions patterns. |
| `examples/angular` | js-only-documented | none | This row | Angular adapter example is JavaScript framework-specific. |
| `examples/express` | not-started | none | none | Portable server example should map to a Rust HTTP framework once stream/chat APIs exist. |
| `examples/fastify` | not-started | none | none | Portable server example should map to a Rust HTTP framework once stream/chat APIs exist. |
| `examples/hono` | not-started | none | none | Portable server example should map to a Rust HTTP framework once stream/chat APIs exist. |
| `examples/mcp` | in-progress | `crates/ai-sdk-mcp/examples/local_mcp_client.rs`, `crates/ai-sdk-mcp/examples/http_auth_typed_tools.rs`, `crates/ai-sdk-mcp/examples/stdio_typed_tools.rs`, `crates/ai-sdk-mcp/examples/sse_typed_tools.rs`, `crates/ai-sdk-mcp/examples/tool_meta.rs`, `crates/ai-sdk-mcp/examples/image_content.rs`, `crates/ai-sdk-mcp/examples/server_metadata.rs`, `crates/ai-sdk-mcp/examples/elicitation_multi_step.rs`, `crates/ai-sdk-mcp/examples/hosted_oauth_http.rs` | `cargo run -p ai-sdk-mcp --example local_mcp_client`; `cargo run -p ai-sdk-mcp --example http_auth_typed_tools`; `cargo run -p ai-sdk-mcp --example stdio_typed_tools`; `cargo run -p ai-sdk-mcp --example sse_typed_tools`; `cargo run -p ai-sdk-mcp --example tool_meta`; `cargo run -p ai-sdk-mcp --example image_content`; `cargo run -p ai-sdk-mcp --example server_metadata`; `cargo run -p ai-sdk-mcp --example elicitation_multi_step`; `cargo run -p ai-sdk-mcp --example hosted_oauth_http` | Rust package-owned MCP examples now mirror upstream `examples/mcp/src/tool-definitions`, `elicitation-multi-step`, `http`, `image-content`, `mcp-with-auth`, `output-schema`, `provider-metadata`, `server-info`, `server-instructions`, `tool-meta`, and `stdio` portable flows: deterministic in-process tools/resources/prompts/elicitation, a deterministic two-step elicitation flow for event creation, a real local Streamable HTTP server with bearer auth, typed output-schema validation, raw MCP tool results, initialized server metadata/instructions access, provider metadata printing, OpenAI Apps `openai/outputTemplate` metadata normalization, app resource reads, image content conversion from MCP `type: "image"` to AI SDK `file` model output, session cleanup, and request-count proof, plus a self-spawned stdio server over `StdioMcpTransport`, a bounded local SSE server over `SseMcpTransport` with the same typed-tool bridge, and a hosted OAuth HTTP server that performs local dynamic client registration, PKCE redirect/callback exchange, token exchange, auth-provider transport config, and protected tool execution. Protected live MCP service auth validation remains credential-gated. |
| `examples/nest` | js-only-documented | none | This row | NestJS framework wiring is JavaScript-specific; portable server behavior is tracked separately. |
| `examples/next` | js-only-documented | none | This row | Next.js framework wiring is JavaScript-specific; portable server behavior is tracked separately. |
| `examples/next-agent` | js-only-documented | none | This row | Depends on Next.js and unported agent APIs. |
| `examples/next-fastapi` | not-started | none | none | Mixed Next/FastAPI example; portable API behavior should be covered by Rust server examples. |
| `examples/next-google-vertex` | js-only-documented | none | This row | Next.js wiring is JavaScript-specific; Google Vertex provider remains unported. |
| `examples/next-langchain` | js-only-documented | none | This row | Depends on JavaScript LangChain adapter and Next.js wiring. |
| `examples/next-openai-kasada-bot-protection` | js-only-documented | none | This row | Depends on Next.js and JavaScript bot-protection integration. |
| `examples/next-openai-pages` | js-only-documented | none | This row | Depends on Next.js Pages Router. |
| `examples/next-openai-telemetry` | js-only-documented | none | This row | Depends on Next.js; portable telemetry remains unported. |
| `examples/next-openai-telemetry-sentry` | js-only-documented | none | This row | Depends on Next.js and JavaScript Sentry integration. |
| `examples/next-openai-upstash-rate-limits` | js-only-documented | none | This row | Depends on Next.js and JavaScript Upstash integration. |
| `examples/next-workflow` | js-only-documented | none | This row | Depends on Next.js and unported workflow package. |
| `examples/node-http-server` | not-started | none | none | Portable HTTP server equivalent should be added after stream/chat APIs. |
| `examples/nuxt-openai` | js-only-documented | none | This row | Nuxt adapter example is JavaScript framework-specific. |
| `examples/sveltekit-openai` | js-only-documented | none | This row | SvelteKit adapter example is JavaScript framework-specific. |

## Upstream Test Corpus And Tooling Inventory

The upstream scan found 521 package test files. Rust parity must continue adding
focused tests for each portable behavior before changing rows to `verified`.

| Upstream area | Test files scanned | Status | Notes |
| --- | ---: | --- | --- |
| `packages/ai` | 128 | in-progress | Many non-streaming high-level API tests are represented in Rust, including non-language high-level abort-signal forwarding for embedding, image, speech, video, transcription, reranking calls, current-provider model resolution, named restricted telemetry dispatcher context-filtering counterparts, and named token-rate/token-count/header/deep-equal/merge-object/split-array/start-index/cosine/abort/callback/serial-job/prepare-retries/request-timeout/language-model-call-options/standardize-prompt/file-part-data/prepare-tools/tool-choice/tool-model-output/filter-active-tools/collect-tool-approvals/validate-tool-context/prune-messages/stop-condition/simulate-readable-stream/server-response/async-iterable-stream/stitchable-stream/download utility counterparts; initial ToolLoopAgent wrapper, model/request option forwarding, instruction-shape forwarding, include request-message retention, prepare-call sandbox/stream provider-option shaping, sandbox propagation into local tool execution, user-approval blocking, onStart callback/event forwarding, callback-merging, and per-call abort/timeout request-control coverage exists, and the legacy v2/v3 model/provider adapter inventory is documented as JavaScript package compatibility, while stream, UI, remaining agent call-options type-level parity, and broader edge coverage remain. |
| Provider package tests | 251 | in-progress | Gateway, Vercel AI Gateway OpenAI-compatible, Vercel v0, OpenAI-compatible non-language abort request forwarding, OpenAI foundation, OpenAI speech, OpenAI transcription, Open Responses foundation, DeepInfra foundation, TogetherAI, Hugging Face, Cerebras, Baseten, Voyage, Luma, RevAI, AssemblyAI, Azure, ByteDance, Mistral, Black Forest Labs, Hume, and Deepgram provider tests now exist. Concrete provider package test files remain largely unported across OpenAI's broader Responses streaming/tools/files surfaces, Hugging Face SSE/tool parity, Anthropic, Google, Bedrock, xAI, and the remaining provider packages. |
| `packages/provider` | 1 | in-progress | Upstream `get-error-message.test.ts` now has a one-to-one Rust test split for every portable original case, including null/undefined, strings, named/custom errors, custom `toString`, and JSON-like values. The package row remains in progress while v2/v3 compatibility surfaces and exact stream abstractions remain unported. |
| `packages/provider-utils` | 77 | in-progress | Many provider support behaviors are represented in the matching `ai-sdk-provider-utils` crate, including one-to-one `filterNullable`, `removeUndefinedEntries`, complete portable `validateTypes`/`safeValidateTypes`, complete portable `secureJsonParse`, complete portable `parseJSON`/`safeParseJSON`/`isParsableJson`, complete portable `injectJsonInstruction`, exact `mediaTypeToExtension` table rows, complete portable `normalizeHeaders` cases, complete portable `mapReasoningToProvider*` cases, complete portable `resolve` cases, complete portable `createToolNameMapping` cases, complete portable `withUserAgentSuffix`, `getRuntimeEnvironmentUserAgent`, `isUrlSupported`, `validateDownloadUrl`, `downloadBlob`/`DownloadError`, `getFromApi`, `delay`, `executeTool`, `isExecutableTool`, portable `asSchema`/`StandardSchema`, `readResponseWithSizeLimit`, `responseHandler`, `handleFetchError`, `convertAsyncIteratorToReadableStream`, `isJSONSerializable`, `StreamingToolCallTracker`, `serializeModelOptions`, and provider-utils `content-part` type-contract cases, complete portable `createIdGenerator`/`generateId` cases, complete portable `DelayedPromise` cases, portable `isProviderReference` cases, and complete portable `resolveProviderReference` cases plus abort propagation for GET, JSON, form-data, and generic POST request helpers, but exact browser stream/fetch parity and Zod adapter snapshots are incomplete or JavaScript-specific. |
| Framework adapter tests | 21 | js-only-documented | Angular, React, RSC, Svelte, and Vue bindings are JavaScript framework-specific; portable transport/message semantics are tracked separately. |
| MCP, Gateway, OTel, Workflow, test server | 39 | in-progress | Gateway package coverage is fully represented in the current upstream audit: 372 upstream `packages/gateway` `it`/`test` cases are mapped by the 380-test `ai-sdk-gateway` crate, with JavaScript-only request-context and class-identity cases documented as non-portable; MCP now has package-owned protocol/type, deterministic client/request lifecycle, mock transport, initial real HTTP transport with inbound SSE GET/resumption/retry coverage, standalone SSE transport, child-process stdio transport, MCP Apps, resource/prompt methods, dynamic and schema-typed tool creation/execution, client-side elicitation request handling, uncaught error callbacks, tool-output conversion coverage, OAuth discovery/authorization/token/registration foundations with generated PKCE, protected-resource selection, high-level provider orchestration, real loopback HTTP validation including authenticated Streamable HTTP tool execution, upstream-shaped HTTP/SSE transport config factories with hosted-auth OAuth propagation and default redirect rejection, and a local MCP client example covering tools/resources/prompts/elicitation; OTel now has package-owned helper, lifecycle, local OTLP/HTTP receiver/export coverage, real Rust OpenTelemetry SDK exporter proof, `OpenTelemetry` and `LegacyOpenTelemetry` recorders, root dispatcher adapters that export dispatcher-produced spans through the local receiver, and initial Gateway provider-live telemetry proof; root telemetry has initial dispatcher/registry/diagnostic-channel coverage plus text, object, embedding, and reranking operation dispatch; Workflow now has an initial package-owned crate with serializable tool schema helper coverage, model-call stream to UI-message chunk conversion, deterministic chat transport request/reconnect planning, deterministic stream-text iterator continuation coverage, and first deterministic WorkflowAgent loop coverage for id, local/provider-executed tool results, client-side tool stopping, messages, per-tool context, prepare-step callback overrides, start/step-start callback behavior, finish callback behavior including constructor-then-stream ordering with event payloads, step-finish callback behavior with step payloads, and local tool-execution callback behavior; test-server is verified for portable Rust parity with MSW and Vitest lifecycle hooks documented as JavaScript-runtime-specific. Gateway JavaScript Date-object identity and thrown Gateway error instance identity cases are documented as non-portable; protected live MCP auth validation, broader Workflow real model/real transport execution, and broader OTel provider-live integration beyond Gateway text and streaming remain unported. |
| Codemod tests | 54 | js-only-documented | JavaScript migration tooling is intentionally not part of the Rust runtime. |
| JavaScript library adapter/devtools tests | 6 | js-only-documented | DevTools, LangChain, LlamaIndex, and Valibot rows are documented as JavaScript-specific above. |
| Repo docs, content, architecture, tools, skills, Changesets, package publishing | not counted | in-progress | Public behavior docs/examples still need Rust equivalents; JS-only repository operations are documented as non-portable where applicable. |

### Recent First-Phase Proof Slices

- 2026-05-23: `packages/openai` Speech parity added `OpenAISpeechModel`
  and `ProviderWithSpeechModel` support plus named Rust counterparts for every
  portable upstream `src/speech/openai-speech-model.test.ts` case:
  `openai_speech_should_pass_the_model_and_text`,
  `openai_speech_should_pass_headers`,
  `openai_speech_should_pass_options`,
  `openai_speech_should_return_audio_data_with_correct_content_type`,
  `openai_speech_should_include_response_data_with_timestamp_model_id_and_headers`,
  `openai_speech_should_use_real_date_when_no_custom_date_provider_is_specified`,
  `openai_speech_should_handle_different_audio_formats`, and
  `openai_speech_should_include_warnings_if_any_are_generated`. Rust now maps
  OpenAI `/audio/speech` JSON request shaping, OpenAI/provider/request headers,
  default voice and response format handling, voice/output-format/speed options,
  binary audio response handling, response headers, timestamp/model metadata,
  provider options for typed OpenAI speech fields, and empty-warning behavior.
  Additive `openai_speech_should_set_specification_version_and_provider` proves
  the provider-v4 speech identity.
- 2026-05-23: `packages/openai` Transcription parity added
  `OpenAITranscriptionModel` and `ProviderWithTranscriptionModel` support plus
  named Rust counterparts for every portable upstream
  `src/transcription/openai-transcription-model.test.ts` case:
  `openai_transcription_should_pass_the_model`,
  `openai_transcription_should_pass_headers`,
  `openai_transcription_should_extract_the_transcription_text`,
  `openai_transcription_should_include_response_data_with_timestamp_model_id_and_headers`,
  `openai_transcription_should_use_real_date_when_no_custom_date_provider_is_specified`,
  `openai_transcription_should_pass_response_format_when_timestamp_granularities_is_set`,
  `openai_transcription_should_not_set_verbose_json_for_gpt_4o_transcribe`,
  `openai_transcription_should_pass_timestamp_granularities_when_specified`,
  `openai_transcription_should_work_when_no_words_language_or_duration_are_returned`,
  `openai_transcription_should_parse_segments_when_provided_in_response`,
  `openai_transcription_should_fallback_to_words_when_segments_are_not_available`,
  `openai_transcription_should_handle_empty_segments_array`, and
  `openai_transcription_should_handle_segments_with_missing_optional_fields`.
  Rust now maps OpenAI `/audio/transcriptions` multipart request shaping,
  OpenAI/provider/request headers, typed provider options for response format,
  temperature and timestamp granularities, JSON response parsing, language
  normalization, duration, segment and word fallback mapping, response
  headers/body, and timestamp/model metadata. Additive
  `openai_transcription_should_set_specification_version_and_provider` proves
  the provider-v4 transcription identity.
- 2026-05-24: `packages/openai` Image model parity added `OpenAIImageModel`
  support plus named Rust counterparts for every portable upstream
  `src/image/openai-image-model.test.ts` case:
  `openai_image_should_pass_the_model_and_the_settings`,
  `openai_image_should_map_provider_options_to_snake_case_for_images_generations`,
  `openai_image_should_pass_headers`,
  `openai_image_should_extract_the_generated_images`,
  `openai_image_should_return_warnings_for_unsupported_settings`,
  `openai_image_should_respect_max_images_per_call_setting`,
  `openai_image_should_include_response_data_with_timestamp_model_id_and_headers`,
  `openai_image_should_use_real_date_when_no_custom_date_provider_is_specified`,
  `openai_image_should_not_include_response_format_for_gpt_image_1`,
  `openai_image_should_not_include_response_format_for_gpt_image_2`,
  `openai_image_should_not_include_response_format_for_chatgpt_image_latest`,
  `openai_image_should_not_include_response_format_for_date_suffixed_gpt_image_model_ids`,
  `openai_image_should_handle_null_revised_prompt_responses`,
  `openai_image_should_include_response_format_for_dall_e_3`,
  `openai_image_should_return_image_meta_data`,
  `openai_image_should_map_openai_usage_to_usage`,
  `openai_image_should_distribute_input_token_details_evenly_across_images`,
  `openai_image_should_call_images_edits_endpoint_when_files_are_provided`,
  `openai_image_should_send_image_as_form_data_with_uint8array_input`,
  `openai_image_should_send_image_as_form_data_with_base64_string_input`,
  `openai_image_should_send_multiple_images_as_form_data_array`,
  `openai_image_should_pass_provider_options_in_form_data`,
  `openai_image_should_map_provider_options_to_snake_case_for_images_edits`,
  `openai_image_should_extract_the_edited_images_from_response`,
  `openai_image_should_include_response_metadata_for_edited_images`,
  `openai_image_should_return_warnings_for_unsupported_settings_in_edit_mode`,
  and `openai_image_should_return_usage_information_for_edited_images`. Rust
  now maps `/images/generations` and `/images/edits` request shaping,
  OpenAI/provider/request headers, provider-option snake casing, unsupported
  aspect-ratio/seed warnings, model-specific `maxImagesPerCall`, model-specific
  `response_format` defaults, generated/edited image extraction, response
  headers/timestamp/model metadata, provider image metadata, usage mapping, and
  image/text token detail distribution.
- 2026-05-24: `packages/openai` provider base URL precedence parity split
  upstream `openai-provider.test.ts` into named Rust counterparts for default
  OpenAI base URL resolution, `OPENAI_BASE_URL` fallback, and explicit
  `baseURL` precedence:
  `openai_provider_uses_the_default_openai_base_url_when_not_provided`,
  `openai_provider_uses_openai_base_url_when_set`, and
  `openai_provider_prefers_the_base_url_option_over_openai_base_url`.
- 2026-05-24: `packages/openai` Embedding parity split upstream
  `src/embedding/openai-embedding-model.test.ts` into named Rust counterparts:
  `openai_embedding_should_extract_embedding`,
  `openai_embedding_should_expose_the_raw_response_headers`,
  `openai_embedding_should_expose_the_raw_response_body`,
  `openai_embedding_should_extract_usage`,
  `openai_embedding_should_pass_the_model_and_the_values`,
  `openai_embedding_should_pass_the_dimensions_setting`, and
  `openai_embedding_should_pass_headers`. Rust now maps OpenAI embedding
  vector extraction, raw response headers/body, token usage, request body
  model/input/`encoding_format`, `dimensions` provider option forwarding, and
  OpenAI provider/request header merging.
- 2026-05-24: `packages/openai` Completion non-stream parity split upstream
  `src/completion/openai-completion-language-model.test.ts` `doGenerate`
  cases into named Rust counterparts:
  `openai_completion_should_extract_text_response`,
  `openai_completion_should_extract_usage`,
  `openai_completion_should_send_request_body`,
  `openai_completion_should_send_additional_response_information`,
  `openai_completion_should_extract_logprobs`,
  `openai_completion_should_extract_finish_reason`,
  `openai_completion_should_support_unknown_finish_reason`,
  `openai_completion_should_expose_the_raw_response_headers`,
  `openai_completion_should_pass_the_model_and_the_prompt`, and
  `openai_completion_should_pass_headers`. Rust now maps OpenAI completion
  text extraction, usage, request body, response metadata, logprobs provider
  metadata, finish reason mapping, response headers, model/prompt shaping, and
  provider/request header merging.
- 2026-05-24: `packages/openai` Completion streaming parity split upstream
  `src/completion/openai-completion-language-model.test.ts` `doStream` cases
  into named Rust counterparts:
  `openai_completion_stream_should_stream_text_deltas`,
  `openai_completion_stream_should_handle_error_stream_parts`,
  `openai_completion_stream_should_handle_unparsable_stream_parts`,
  `openai_completion_stream_should_send_request_body`,
  `openai_completion_stream_should_expose_the_raw_response_headers`,
  `openai_completion_stream_should_pass_the_model_and_the_prompt`, and
  `openai_completion_stream_should_pass_headers`. Rust now maps OpenAI
  completion SSE text deltas, provider-error and parse-error stream parts,
  streamed usage and logprobs finish metadata, request body `stream: true`,
  response headers, model/prompt shaping, and provider/request header merging.
- 2026-05-23: `packages/openai` Skills upload parity added `OpenAISkills`
  and `ProviderWithSkills` support plus named Rust counterparts for every
  portable upstream `src/skills/openai-skills.test.ts` case:
  `openai_skills_should_send_files_as_multipart_form_data`,
  `openai_skills_should_pass_authorization_headers`,
  `openai_skills_should_map_response_to_provider_reference`,
  `openai_skills_should_emit_unsupported_warning_for_display_title`,
  `openai_skills_should_return_no_warnings_when_display_title_is_not_set`,
  and `openai_skills_should_handle_uint8array_file_content`. Rust now maps
  OpenAI `/skills` upload request shaping through multipart form data,
  OpenAI auth headers, base64/text/raw byte file conversion, provider
  references, response name/description/latest-version fields, OpenAI provider
  metadata, and the unsupported `displayTitle` warning. Additive
  `openai_skills_should_set_specification_version_and_provider` proves the
  provider-v4 skills identity.
- 2026-05-23: `packages/openai` Files upload parity added `OpenAIFiles`
  and `ProviderWithFiles` support plus named Rust counterparts for the portable
  upstream `src/files/openai-files.test.ts` cases:
  `openai_files_should_send_correct_multipart_request_with_purpose`,
  `openai_files_should_return_provider_reference_with_openai_key`,
  `openai_files_should_return_provider_metadata_from_response`,
  `openai_files_should_default_purpose_to_assistants_when_not_provided`,
  `openai_files_should_pass_expires_after_when_provided`,
  `openai_files_should_pass_auth_headers`,
  `openai_files_should_handle_base64_string_data`, and
  `openai_files_should_set_specification_version_and_provider`. Rust now maps
  OpenAI `/files` upload request shaping through multipart form data, default
  `purpose: assistants`, provider `expiresAfter` to `expires_after`,
  OpenAI auth headers, base64 data conversion, provider references, provider
  metadata, and provider-v4 files identity.
- 2026-05-23: `packages/ai` `smoothStream` portable edge-case parity
  added named Rust counterparts for the remaining portable upstream
  `generate-text/smooth-stream.test.ts` cases:
  `smooth_stream_should_split_larger_text_chunks`,
  `smooth_stream_should_keep_longer_whitespace_sequences_together`,
  `smooth_stream_should_flush_text_buffer_before_tool_call_starts`,
  `smooth_stream_should_flush_text_buffer_before_streaming_tool_input_starts`,
  `smooth_stream_should_not_return_chunks_with_just_spaces`,
  `smooth_stream_should_split_text_by_lines_when_using_line_chunking_mode`,
  `smooth_stream_should_handle_text_without_line_endings_in_line_chunking_mode`,
  `smooth_stream_should_support_custom_chunking_regexps_character_level`,
  `smooth_stream_should_change_the_id_when_the_text_part_id_changes`,
  `smooth_stream_should_split_larger_reasoning_chunks`,
  `smooth_stream_should_flush_reasoning_buffer_before_tool_call`,
  `smooth_stream_should_use_line_chunking_for_reasoning`,
  `smooth_stream_should_flush_text_buffer_when_switching_to_reasoning`,
  `smooth_stream_should_flush_reasoning_buffer_when_switching_to_text`,
  `smooth_stream_should_handle_multiple_switches_between_text_and_reasoning`,
  and
  `smooth_stream_preserves_provider_metadata_on_reasoning_start_for_redacted_thinking`.
  Rust now has one-to-one portable coverage for upstream word, line, regex,
  callback-detector, delay, text-id switching, reasoning, interleaving, tool-call
  flush, streamed tool-input flush, whitespace buffering, and Anthropic
  reasoning metadata preservation behavior. Upstream invalid `chunking` option
  construction is covered by Rust's typed `SmoothStreamChunking` API plus
  detector/pattern validation tests; `Intl.Segmenter` cases remain
  JavaScript-runtime-specific unless a future dependency-backed segmentation
  strategy is intentionally added.
- 2026-05-23: `packages/openai` non-Responses error schema parity added
  `OpenAIErrorData` / `OpenAIErrorDetails` plus
  `openai_error_data_schema_should_parse_openrouter_resource_exhausted_error`
  in `src/openai.rs`, mapping upstream `openai-error.test.ts` for OpenRouter's
  nested resource-exhausted error body with a numeric `code`. The Rust schema
  keeps upstream's loose OpenAI-compatible boundary: required `message`,
  optional `type`, optional arbitrary `param`, and optional string-or-number
  `code`.
- 2026-05-23: `packages/ai` restricted telemetry dispatcher parity added
  named Rust counterparts for every portable upstream
  `generate-text/restricted-telemetry-dispatcher.test.ts` case:
  `restricted_telemetry_dispatcher_excludes_runtime_context_when_no_include_context_is_configured`,
  `restricted_telemetry_dispatcher_only_includes_runtime_context_properties_marked_as_true`,
  `restricted_telemetry_dispatcher_includes_configured_runtime_context_for_start_events_without_mutating_source_event`,
  `restricted_telemetry_dispatcher_filters_tools_context_per_tool_for_start_events_without_mutating_source_event`,
  `restricted_telemetry_dispatcher_excludes_tools_context_properties_when_no_include_context_is_configured`,
  `restricted_telemetry_dispatcher_includes_configured_runtime_context_for_step_start_events_and_previous_steps`,
  `restricted_telemetry_dispatcher_filters_tools_context_for_step_start_events_and_previous_steps`,
  `restricted_telemetry_dispatcher_includes_configured_runtime_context_for_step_finish_events_without_mutating_source_step`,
  `restricted_telemetry_dispatcher_filters_tools_context_for_step_finish_events_without_mutating_source_step`,
  `restricted_telemetry_dispatcher_includes_configured_runtime_context_for_end_events_and_all_steps_without_mutating_source_steps`,
  `restricted_telemetry_dispatcher_filters_tools_context_for_end_events_and_all_steps_without_mutating_source_steps`,
  `restricted_telemetry_dispatcher_filters_tool_execution_start_events_without_mutating_the_source_event`,
  `restricted_telemetry_dispatcher_filters_tool_execution_end_events_without_mutating_the_source_event`,
  and
  `restricted_telemetry_dispatcher_passes_through_execute_tool_without_filtering`.
  `TelemetryDispatcher` now applies `include_runtime_context` and
  `include_tools_context` to top-level events, prior step payloads, and
  tool-execution `toolContext` payloads before integrations or diagnostics see
  them, while execute-tool wrappers remain unfiltered as upstream requires.
- 2026-05-23: `packages/ai` `resolveToolApproval` remaining portable
  callback/static/context parity added named Rust counterparts
  `resolve_tool_approval_resolves_async_status_from_generic_function`,
  `resolve_tool_approval_passes_tool_call_tools_context_messages_and_runtime_to_generic_function`,
  `resolve_tool_approval_passes_through_object_status_reason_from_generic_function`,
  `resolve_tool_approval_passes_same_messages_and_validated_tool_context_to_per_tool_function`,
  `resolve_tool_approval_passes_tools_context_entry_through_after_schema_validation`,
  `resolve_tool_approval_normalizes_static_string_before_tool_defined_approval`,
  `resolve_tool_approval_passes_through_static_object_status_reason`,
  `resolve_tool_approval_uses_user_defined_callback_before_tool_defined_approval`,
  `resolve_tool_approval_passes_reason_returned_by_user_defined_callback`,
  and
  `resolve_tool_approval_normalizes_string_status_returned_by_user_defined_callback`.
  Rust now has named value-equivalent coverage for the upstream generic
  callback option payload, async callback resolution, object-status reasons,
  static status precedence, per-tool validated context, schema-defaulted
  context transformation, and per-tool callback reason/string normalization.
  The upstream JavaScript reference-identity assertions are represented by
  cloned Rust value-equivalence checks because the Rust callback boundary owns
  typed values rather than JavaScript object references.
- 2026-05-23: `packages/ai` `resolveToolApproval` callback
  normalization parity added named Rust counterparts
  `resolve_tool_approval_treats_none_from_generic_callback_as_not_applicable`,
  `resolve_tool_approval_uses_generic_callback_before_tool_defined_approval`,
  `resolve_tool_approval_treats_none_from_per_tool_callback_as_not_applicable`,
  and `resolve_tool_approval_passes_no_tool_context_without_context_schema`.
  Rust now covers the upstream generic and per-tool callback precedence,
  missing-return normalization, and omitted `toolContext` behavior.
- 2026-05-23: `packages/ai` `resolveToolApproval` context-validation
  parity added named Rust counterparts
  `resolve_tool_approval_passes_validated_context_to_user_defined_approval_callback`,
  `resolve_tool_approval_validates_context_before_user_defined_approval_callback`,
  and
  `resolve_tool_approval_validates_context_before_tool_defined_approval_callback`.
  Rust now validates a matching tool's `contextSchema` before per-tool
  user-defined or tool-defined approval callbacks, passes the validated context
  to the callback, and returns `TypeValidationError` without invoking the
  callback on invalid context.
- 2026-05-23: `packages/ai` `createAgentUIStreamResponse` parity added
  named Rust counterparts
  `create_agent_ui_stream_response_uses_tool_model_output_for_ui_tool_results`
  and
  `create_agent_ui_stream_response_calls_on_finish_with_auto_original_messages`.
  Rust now converts prior UI tool-result parts through matching tool
  `toModelOutput`, streams the agent response into standard UI-message SSE
  chunks, and auto-populates original messages for `onFinish` persistence
  callbacks.
- 2026-05-23: `packages/ai` `ToolLoopAgent` onStart parity added
  named Rust counterparts for constructor and method callback registration
  plus start-event payload forwarding for generate and stream. The tests are
  `tool_loop_agent_generate_calls_on_start_from_constructor`,
  `tool_loop_agent_generate_calls_on_start_from_method`,
  `tool_loop_agent_generate_on_start_passes_event_information`,
  `tool_loop_agent_generate_on_start_passes_messages_option`,
  `tool_loop_agent_stream_calls_on_start_from_constructor`,
  `tool_loop_agent_stream_calls_on_start_from_method`, and
  `tool_loop_agent_stream_on_start_passes_event_information`.
- 2026-05-24: `packages/ai` `ToolLoopAgent` step callback parity added
  named Rust counterparts for the upstream `experimental_onStepStart` and
  `onStepFinish` constructor/method/order/event cases across generate and
  stream. The tests are
  `tool_loop_agent_generate_calls_on_step_start_from_constructor`,
  `tool_loop_agent_generate_calls_on_step_start_from_method`,
  `tool_loop_agent_generate_merges_on_step_start_callbacks_in_order`,
  `tool_loop_agent_generate_on_step_start_passes_event_information`,
  `tool_loop_agent_generate_calls_on_step_finish_from_constructor`,
  `tool_loop_agent_generate_calls_on_step_finish_from_method`,
  `tool_loop_agent_generate_merges_on_step_finish_callbacks_in_order`,
  `tool_loop_agent_generate_on_step_finish_passes_step_result_to_callback`,
  `tool_loop_agent_stream_merges_on_step_start_callbacks_in_order`,
  `tool_loop_agent_stream_on_step_start_passes_event_information`,
  `tool_loop_agent_stream_merges_on_step_finish_callbacks_in_order`, and
  `tool_loop_agent_stream_on_step_finish_passes_step_result_to_callback`.
- 2026-05-24: `packages/ai` `ToolLoopAgent` finish callback parity added
  named Rust counterparts for the upstream `onFinish` constructor, method,
  combined-order, and final-event payload cases across generate and stream.
  The tests are `tool_loop_agent_generate_calls_on_finish_from_constructor`,
  `tool_loop_agent_generate_calls_on_finish_from_method`,
  `tool_loop_agent_generate_merges_on_finish_callbacks_in_order`,
  `tool_loop_agent_generate_on_finish_passes_event_information`,
  `tool_loop_agent_stream_calls_on_finish_from_constructor`,
  `tool_loop_agent_stream_calls_on_finish_from_method`,
  `tool_loop_agent_merges_stream_finish_callbacks_in_order`, and
  `tool_loop_agent_stream_on_finish_passes_event_information`.
- 2026-05-24: `packages/ai` `ToolLoopAgent` tool-execution callback parity
  added named Rust counterparts for upstream `onToolExecutionStart` and
  `onToolExecutionEnd` constructor, method, callback-order, and event-payload
  cases across generate and stream. The tests are
  `tool_loop_agent_generate_calls_on_tool_execution_start_from_constructor`,
  `tool_loop_agent_generate_calls_on_tool_execution_start_from_method`,
  `tool_loop_agent_generate_merges_on_tool_execution_start_callbacks_in_order`,
  `tool_loop_agent_generate_on_tool_execution_start_passes_event_information`,
  `tool_loop_agent_generate_calls_on_tool_execution_end_from_constructor`,
  `tool_loop_agent_generate_calls_on_tool_execution_end_from_method`,
  `tool_loop_agent_generate_merges_on_tool_execution_end_callbacks_in_order`,
  `tool_loop_agent_generate_on_tool_execution_end_passes_event_information_on_success`,
  `tool_loop_agent_stream_calls_on_tool_execution_start_from_constructor`,
  `tool_loop_agent_stream_calls_on_tool_execution_start_from_method`,
  `tool_loop_agent_stream_merges_on_tool_execution_start_callbacks_in_order`,
  `tool_loop_agent_stream_on_tool_execution_start_passes_event_information`,
  `tool_loop_agent_stream_calls_on_tool_execution_end_from_constructor`,
  `tool_loop_agent_stream_calls_on_tool_execution_end_from_method`,
  `tool_loop_agent_stream_merges_on_tool_execution_end_callbacks_in_order`,
  and
  `tool_loop_agent_stream_on_tool_execution_end_passes_event_information_on_success`.
- 2026-05-24: `packages/ai` `ToolLoopAgent` telemetry integration parity
  added named Rust counterparts for upstream generate and stream per-call and
  global integration listener lifecycle ordering, runtime-context filtering,
  agent-callback interleaving, and listener panic isolation. The tests are
  `tool_loop_agent_generate_calls_per_call_integration_listeners_for_all_lifecycle_events`,
  `tool_loop_agent_stream_calls_per_call_integration_listeners_for_all_lifecycle_events`,
  `tool_loop_agent_generate_calls_globally_registered_integration_listeners`,
  `tool_loop_agent_stream_calls_globally_registered_integration_listeners`,
  `tool_loop_agent_generate_includes_configured_runtime_context_properties_in_telemetry`,
  `tool_loop_agent_stream_includes_configured_runtime_context_properties_in_telemetry`,
  `tool_loop_agent_generate_calls_integration_listeners_alongside_agent_callbacks`,
  `tool_loop_agent_stream_calls_integration_listeners_alongside_agent_callbacks`,
  `tool_loop_agent_generate_does_not_break_when_an_integration_listener_panics`,
  and `tool_loop_agent_stream_does_not_break_when_an_integration_listener_panics`.
- 2026-05-24: `packages/ai` `ToolLoopAgent` call-options schema parity
  added `ToolLoopAgentSettings::with_call_options_schema`,
  `ToolLoopAgentCallOptions::with_options`, validated options propagation into
  `prepare_call`, and named Rust counterparts for upstream
  `callOptionsSchema` tests:
  `tool_loop_agent_generate_rejects_invalid_call_options_schema_before_model_call`
  and
  `tool_loop_agent_generate_passes_valid_call_options_schema`.
- 2026-05-24: `packages/ai` `InferAgentUIMessage` type-level parity added
  named Rust counterparts for upstream `infer-agent-ui-message.test-d.ts`:
  `infer_agent_ui_message_should_not_contain_arbitrary_static_tools_when_no_tools_are_provided`
  and `infer_agent_ui_message_should_include_metadata_when_provided`. Rust
  proves the equivalent typed boundary by exposing no configured static tools
  for a no-tool `ToolLoopAgent`, keeping dynamic/data UI parts representable,
  and preserving caller-provided UI message metadata through serialization.
- 2026-05-24: `packages/ai` `handleUIMessageStreamFinish` parity added
  named Rust counterparts for the portable upstream
  `handle-ui-message-stream-finish.test.ts` pass-through, injected message id,
  finish callback, continuation, abort, multi-step `onStepFinish`, combined
  step/final callback, continuation step, cloned-message, and no-callback
  pass-through cases:
  `handle_ui_message_stream_finish_passes_through_chunks_without_callbacks`,
  `handle_ui_message_stream_finish_handles_empty_original_messages_array`,
  `handle_ui_message_stream_finish_handles_continuation_when_last_message_is_assistant`,
  `handle_ui_message_stream_finish_does_not_treat_user_message_as_continuation`,
  `handle_ui_message_stream_finish_sets_is_aborted_when_abort_chunk_is_encountered`,
  `handle_ui_message_stream_finish_sets_is_aborted_false_without_abort_chunk`,
  `handle_ui_message_stream_finish_passes_through_abort_chunk_without_callbacks`,
  `handle_ui_message_stream_finish_handles_multiple_abort_chunks`,
  `handle_ui_message_stream_finish_calls_on_step_finish_when_finish_step_chunk_is_encountered`,
  `handle_ui_message_stream_finish_calls_on_step_finish_multiple_times_for_multiple_steps`,
  `handle_ui_message_stream_finish_calls_both_on_step_finish_and_on_finish`,
  `handle_ui_message_stream_finish_handles_continuation_scenario_with_on_step_finish`,
  `handle_ui_message_stream_finish_provides_cloned_messages_to_on_step_finish`,
  and
  `handle_ui_message_stream_finish_does_not_process_stream_when_no_callbacks_are_provided`.
  Rust now preserves open text/reasoning part tracking after abort chunks so
  later chunks in the same upstream-style stream remain processable. Upstream's
  reader-cancellation and rejected async callback logging cases remain
  JavaScript Web-stream/Promise-runtime boundaries for the current
  materialized synchronous Rust API.
- 2026-05-24: `packages/ai` `createUIMessageStreamResponse`
  `consumeSseStream` parity added `UiMessageSseConsumer` and named Rust
  counterparts for upstream `create-ui-message-stream-response.test.ts`
  consumer cases:
  `create_ui_message_stream_response_calls_consume_sse_stream_with_teed_stream`,
  `create_ui_message_stream_response_does_not_block_response_for_consume_sse_stream`,
  `create_ui_message_stream_response_handles_synchronous_consume_sse_stream`,
  and
  `create_ui_message_stream_response_handles_consume_sse_stream_errors_gracefully`.
  Rust represents upstream's teed `ReadableStream<string>` as a cloned
  materialized encoded SSE body; consumer errors are ignored so the returned
  response body remains readable. Exact asynchronous Web-stream scheduling is
  JavaScript-runtime-specific for the current Rust helper.
- 2026-05-24: `packages/ai` `createUIMessageStream` writer basics parity
  added named Rust counterparts for portable upstream
  `create-ui-message-stream.test.ts` write/merge/error cases:
  `create_ui_message_stream_should_send_data_stream_part_and_close_the_stream`,
  `create_ui_message_stream_should_forward_a_single_stream_with_two_elements`,
  and `create_ui_message_stream_should_add_error_parts_when_stream_errors`.
  Rust maps upstream `writer.write` and `writer.merge(ReadableStream)` to the
  existing materialized writer and `merge`/`merge_result` APIs. Browser
  `ReadableStream` scheduling cases such as delayed merged streams and writes
  after close remain JavaScript-runtime-specific until a live stream
  abstraction is introduced.
- 2026-05-24: `packages/ai` UI-message text/response edge parity added named
  Rust counterparts for the remaining portable upstream
  `transform-text-to-ui-message-stream.test.ts` single-chunk case and
  `pipe-ui-message-stream-to-response.test.ts` error-stream case:
  `transform_text_to_ui_message_stream_should_handle_single_chunk_streams` and
  `pipe_ui_message_stream_to_response_should_handle_errors_in_the_stream`.
  Together with the existing multi-chunk, empty-stream, header, and encoded SSE
  tests, the portable upstream transform and pipe test files are now mapped in
  Rust; Node `ServerResponse` and Web `ReadableStream` details remain covered
  through the crate's response-writer and collected-stream boundaries.
- 2026-05-24: `packages/ai` provider-registry middleware parity added
  `create_provider_registry_with_language_model_middleware` plus the named Rust
  counterpart
  `create_provider_registry_should_wrap_all_language_models_accessed_through_the_provider_registry`
  for upstream `provider-registry.test.ts` language-model registry middleware
  wrapping. Rust returns an explicit `ProviderRegistry<WrappedProvider<...>>`
  so the middleware-adjusted model type is visible at compile time. The
  adjacent upstream image-model registry middleware case remains in-progress.
- 2026-05-23: `packages/ai` `streamText` automatic tool approval stream
  parity added the named Rust counterpart
  `stream_text_automatic_tool_approval_response_streams_before_tool_result`
  and hardened
  `stream_text_applies_denied_tool_approval_to_continuation_messages`.
  Rust now emits automatic approval request metadata and approval-response
  chunks into both `fullStream` and `toUIMessageStream`, while preserving the
  continuation prompt ordering for approved and denied local tools.
- 2026-05-23: `packages/ai` `ToolLoopAgent` tool approval parity added
  named Rust counterparts for the upstream generate and stream
  `toolApproval: { testTool: 'user-approval' }` blocking cases. The tests are
  `tool_loop_agent_generate_honors_tool_approval` and
  `tool_loop_agent_stream_honors_tool_approval`.
- 2026-05-23: `packages/ai` `ToolLoopAgent` instruction-shape parity
  added named Rust counterparts for the upstream generate string instructions,
  generate system-message instructions, generate array-of-system-message
  instructions, stream string instructions, and stream system-message
  instructions cases. The tests are
  `tool_loop_agent_generate_passes_string_instructions`,
  `tool_loop_agent_generate_passes_system_message_instructions`,
  `tool_loop_agent_generate_passes_array_of_system_message_instructions`,
  `tool_loop_agent_stream_passes_string_instructions`, and
  `tool_loop_agent_stream_passes_system_message_instructions`.
- 2026-05-23: `packages/ai` `ToolLoopAgent` tool-execution sandbox parity
  added named Rust counterparts for the upstream generate and stream
  `experimental_sandbox` forwarding into local tool execution. The tests are
  `tool_loop_agent_generate_passes_sandbox_to_tool_execution` and
  `tool_loop_agent_stream_passes_sandbox_to_tool_execution`.
- 2026-05-23: `packages/ai` `ToolLoopAgent` include and prepare-call parity
  added named Rust counterparts for the upstream generate
  `include.requestMessages`, generate and stream `prepareCall` sandbox
  forwarding, and stream `prepareCall` provider-option shaping cases. The tests
  are
  `tool_loop_agent_generate_forwards_include_request_messages_to_generate_text`,
  `tool_loop_agent_generate_passes_sandbox_to_prepare_call`,
  `tool_loop_agent_stream_prepare_call_can_shape_provider_options`, and
  `tool_loop_agent_stream_passes_sandbox_to_prepare_call`.
- 2026-05-23: `packages/ai` `ToolLoopAgent` model/request option forwarding
  added named Rust counterparts for the upstream generate `temperature`,
  `maxOutputTokens`, `topP`, `topK`, `presencePenalty`, `frequencyPenalty`,
  `stopSequences`, `seed`, and `headers` cases, plus the stream
  `include.rawChunks` case. The tests are
  `tool_loop_agent_generate_forwards_temperature_to_generate_text`,
  `tool_loop_agent_generate_forwards_max_output_tokens_to_generate_text`,
  `tool_loop_agent_generate_forwards_top_p_to_generate_text`,
  `tool_loop_agent_generate_forwards_top_k_to_generate_text`,
  `tool_loop_agent_generate_forwards_presence_penalty_to_generate_text`,
  `tool_loop_agent_generate_forwards_frequency_penalty_to_generate_text`,
  `tool_loop_agent_generate_forwards_stop_sequences_to_generate_text`,
  `tool_loop_agent_generate_forwards_seed_to_generate_text`,
  `tool_loop_agent_generate_forwards_headers_to_generate_text`, and
  `tool_loop_agent_stream_forwards_include_raw_chunks_to_stream_text`.
- 2026-05-23: `packages/ai` `ToolLoopAgent` per-call request controls now
  mirror the portable upstream generate/stream abort and timeout cases with
  `tool_loop_agent_generate_passes_abort_signal_to_generate_text`,
  `tool_loop_agent_generate_passes_timeout_to_tool_execution`,
  `tool_loop_agent_stream_passes_abort_signal_to_stream_text`, and
  `tool_loop_agent_stream_passes_timeout_to_tool_execution`. Rust forwards
  caller abort signals into provider call options and forwards timeout
  configuration into `generate_text`/`stream_text`; the deterministic timeout
  assertions prove the current Rust timeout boundary through local tool
  execution abort signals.
- 2026-05-23: `packages/ai` UI last-assistant completion predicates now have
  named Rust counterparts for every portable upstream
  `last-assistant-message-is-complete-with-tool-calls.test.ts` and
  `last-assistant-message-is-complete-with-approval-responses.test.ts` case.
  The split replaced the aggregate catch-all tests with
  `last_assistant_tool_calls_false_when_last_step_only_has_text`,
  `last_assistant_tool_calls_true_when_text_follows_last_tool_result`,
  `last_assistant_tool_calls_true_when_tool_has_output_error`,
  `last_assistant_tool_calls_true_when_dynamic_tool_is_complete`,
  `last_assistant_tool_calls_false_when_dynamic_tool_input_streaming`,
  `last_assistant_tool_calls_false_when_dynamic_tool_has_input_only`,
  `last_assistant_tool_calls_true_when_dynamic_tool_has_output_error`,
  `last_assistant_tool_calls_true_when_regular_and_dynamic_tools_complete`,
  `last_assistant_tool_calls_false_when_mixed_tools_include_incomplete`,
  `last_assistant_tool_calls_true_when_last_step_dynamic_tool_complete`,
  `last_assistant_tool_calls_false_when_last_step_dynamic_tool_incomplete`,
  `last_assistant_tool_calls_false_for_provider_executed_tool_only`,
  `last_assistant_approval_responses_false_when_messages_empty`,
  `last_assistant_approval_responses_false_when_last_message_is_user`,
  `last_assistant_approval_responses_false_when_last_step_has_no_tools`,
  `last_assistant_approval_responses_false_when_no_tool_approval_responded`,
  `last_assistant_approval_responses_false_when_any_tool_approval_requested`,
  `last_assistant_approval_responses_true_when_non_provider_tool_approval_responded`,
  `last_assistant_approval_responses_true_when_provider_tool_approval_responded`,
  `last_assistant_approval_responses_true_when_terminal_tools_include_approval_response`,
  `last_assistant_approval_responses_true_when_provider_approval_and_regular_output`,
  `last_assistant_approval_responses_false_when_regular_tool_still_needs_approval`,
  and
  `last_assistant_approval_responses_false_when_only_prior_step_has_approval`.
  The approval matrix now also covers the upstream provider-executed
  approval-response plus regular output-available tool mix.
- 2026-05-23: `packages/ai` `executeToolCall` timeout/callback-array/
  telemetry-wrapper parity added named counterparts for per-tool timeout abort
  signal creation and merging, per-tool timeout lookup without a default
  timeout, callback array fan-out and panic tolerance, and
  `executeToolInTelemetryContext` wrapper execution plus inner-only duration
  accounting.
- 2026-05-23: `packages/ai` `executeToolCall` abort/preliminary parity
  wired existing Rust abort signals into local tool execution, switched local
  execution to the provider-utils `execute_tool` final/preliminary contract,
  and added named counterparts for preliminary-result callbacks and final
  result selection from streamed tool outputs.
- 2026-05-23: `packages/ai` `executeToolCall` callback/sandbox parity
  added named counterparts for sandbox forwarding, missing-tool/empty-tools
  no-op behavior, tool-execution start/end callback payloads, callback panic
  tolerance, and returned/event duration propagation.
- 2026-05-23: `packages/ai` `executeToolCall` basic parity added named
  counterparts for no-execute tools, successful tool-result shaping,
  provider/tool metadata propagation on success and error, dynamic result
  flags, and tool-context schema validation failure.
- 2026-05-23: `packages/ai` `parseToolCall` helper parity added named
  counterparts for provider-executed dynamic calls, provider metadata, empty
  inputs, unavailable tool errors, runtime dynamic flags, title propagation,
  tool metadata propagation, and provider/tool metadata separation.
- 2026-05-23: `packages/ai` completed the portable `parseToolCall` repair and
  refinement matrix with named Rust counterparts for post-parse input
  refinement, repair callback invocation/result use, instruction prompt
  forwarding, repair returning `None`, and repair callback errors.
- 2026-05-23: `packages/ai` `parseToolCall` input-schema validation
  parity added Rust-side tool input validation for the common JSON Schema
  object/property/required/type shapes used by first-phase tool schemas, plus
  named counterparts for valid parsed inputs, invalid schema arguments,
  validation-failure repair, and pre-execution invalid tool-result generation.
- 2026-05-23: `packages/ai` `toResponseMessages` parity added 27 named Rust
  counterparts for upstream `generate-text/to-response-messages.test.ts`,
  covering assistant text/custom/reasoning/file parts, provider metadata, tool
  calls, client tool results and errors, `toModelOutput`, provider-executed
  tool call/result placement, tool approval requests/responses, denied approval
  execution-denied results, invalid tool-call input sanitization, and empty
  assistant-message suppression.
- 2026-05-23: `packages/ai` token metric utility parity documented the
  existing named Rust counterparts for upstream
  `generate-text/sum-token-counts.test.ts` and
  `generate-text/calculate-tokens-per-second.test.ts`: `sum_token_counts_should_*`
  covers known counts, one unknown count, and both unknown counts, while
  `calculate_tokens_per_second_should_*` covers average rate, unknown token
  count, zero duration, zero duration plus unknown token count, and unknown
  duration. The upstream non-finite `Number.POSITIVE_INFINITY`/`Number.NaN`
  case is documented as JavaScript-number-only because Rust token counts enter
  this helper as `Option<u64>` and cannot represent non-finite numbers through
  the public typed boundary.
- 2026-05-23: `packages/ai` `convertToLanguageModelPrompt` validation parity
  added the public Rust `convert_to_language_model_prompt` helper plus
  `StandardizedPrompt::try_into_language_model_prompt`, with 4 named
  counterparts for upstream
  `prompt/convert-to-language-model-prompt.validation.test.ts`:
  `convert_to_language_model_prompt_validation_should_pass_for_provider_executed_tools_deferred_results`,
  `convert_to_language_model_prompt_validation_should_pass_for_tool_approval_response`,
  `convert_to_language_model_prompt_validation_should_preserve_provider_executed_tool_approval_response`,
  and
  `convert_to_language_model_prompt_validation_should_throw_error_for_actual_missing_results`.
  Rust now filters assistant approval requests, uses non-provider approval
  responses only for validation, preserves provider-executed approval responses
  in provider-facing tool messages, and raises `MissingToolResultsError` for
  unresolved local tool calls. The high-level text/object stream and generate
  `from_prompt` constructors now use the validated conversion path before model
  calls while preserving their existing `InvalidPromptError` boundary, with
  `generate_text_from_prompt_rejects_missing_tool_results_before_model_call`
  covering the root generate-text integration path.
- 2026-05-23: `packages/ai` `prepareTools` parity split the prior grouped Rust
  coverage into 7 named counterparts for upstream `prompt/prepare-tools.test.ts`:
  `prepare_tools_should_return_undefined_when_tools_are_not_provided`,
  `prepare_tools_should_return_all_tools`,
  `prepare_tools_should_handle_provider_defined_tools`,
  `prepare_tools_should_pass_through_provider_options`,
  `prepare_tools_should_pass_through_strict_mode_settings`,
  `prepare_tools_should_pass_through_input_examples`, and
  `prepare_tools_should_resolve_function_descriptions_from_tools_context_and_sandbox`.
  The Rust helper is implemented in `crates/ai-sdk-provider-utils` and
  re-exported by the root facade, while preserving provider-facing function and
  provider-tool shapes for root `packages/ai` callers.
- 2026-05-23: `packages/ai` `convertToLanguageModelV4FilePart` parity added
  the public Rust `convert_to_language_model_v4_file_part` helper and 18 named
  counterparts for upstream `prompt/file-part-data.test.ts`. The tests cover
  legacy bare bytes, byte-slice, base64, URL string, URL instance, data URL, and
  provider-reference shorthand; tagged data, URL, reference, and text shapes;
  data-URL rejection for explicitly tagged inline data; and equality between
  tagged and bare shorthand forms. Rust maps JavaScript `Uint8Array` and
  `ArrayBuffer` to `Vec<u8>` and `&[u8]` byte inputs at the typed boundary.
- 2026-05-23: `packages/ai` `standardizePrompt` parity split grouped prompt
  checks into named Rust counterparts for the portable upstream
  `prompt/standardize-prompt.test.ts` cases:
  `standardize_prompt_should_throw_invalid_prompt_error_when_messages_contain_a_system_message_by_default`,
  `standardize_prompt_should_throw_invalid_prompt_error_when_prompt_messages_contain_a_system_message_by_default`,
  `standardize_prompt_should_allow_system_messages_in_messages_when_allow_system_in_messages_is_true`,
  `standardize_prompt_should_allow_system_messages_in_prompt_messages_when_allow_system_in_messages_is_true`,
  `standardize_prompt_should_throw_invalid_prompt_error_when_messages_array_is_empty`,
  `standardize_prompt_should_support_system_model_message_instructions`,
  `standardize_prompt_should_support_array_of_system_model_message_instructions`,
  `standardize_prompt_should_fall_back_to_system_when_instructions_is_not_defined`,
  and
  `standardize_prompt_should_prefer_instructions_over_system`.
  The upstream malformed allowed-system-message parts case is represented by
  `standardize_prompt_should_reject_allowed_system_message_parts_at_type_boundary`;
  Rust's typed `LanguageModelSystemMessage` content is a string, so the
  JavaScript dynamic invalid-shape case is rejected during deserialization
  before `standardize_prompt` can receive it.
- 2026-05-23: `packages/ai` stop-condition parity split the prior grouped
  Rust checks into named counterparts for the portable upstream
  `generate-text/stop-condition.test.ts` cases:
  `is_step_count_should_return_true_when_the_step_count_matches_exactly`,
  `is_step_count_should_return_false_when_the_step_count_does_not_match_exactly`,
  `is_loop_finished_should_always_return_false`,
  `has_tool_call_should_return_true_when_the_last_step_contains_the_specified_tool_call`,
  `has_tool_call_should_return_false_when_the_specified_tool_call_only_appears_in_earlier_steps`,
  `has_tool_call_should_return_true_when_the_last_step_contains_any_tool_call_from_the_provided_tool_names`,
  `has_tool_call_should_return_false_when_the_last_step_does_not_contain_any_tool_call_from_the_provided_tool_names`,
  `has_tool_call_should_return_false_when_there_are_no_steps`,
  `is_stop_condition_met_should_return_true_when_any_stop_condition_returns_true`,
  and
  `is_stop_condition_met_should_return_false_when_all_stop_conditions_return_false`.
  Upstream async/rejecting predicate cases are JavaScript Promise/function
  boundary behavior; Rust currently exposes built-in stop conditions as typed
  data predicates, so there is no caller-supplied async callback or rejected
  Promise boundary to port.
- 2026-05-23: `packages/ai` `pruneMessages` utility parity split the prior
  broader Rust checks into named counterparts for every upstream
  `generate-text/prune-messages.test.ts` case:
  `prune_messages_should_prune_all_reasoning_parts`,
  `prune_messages_should_prune_the_trailing_message`,
  `prune_messages_should_prune_all_tool_calls_results_errors_and_approvals`,
  `prune_messages_should_prune_tool_calls_before_last_message`,
  `prune_messages_should_prune_tool_calls_and_results_from_multi_turn_conversation_when_last_message_has_no_tool_calls`,
  `prune_messages_should_prune_all_tool_calls_results_errors_and_approvals_before_last_two_messages`,
  and
  `prune_messages_should_prune_all_tool_calls_results_errors_and_approvals_for_two_tool_settings`.
  The Rust tests cover reasoning removal modes, all/before-last/before-last-N
  tool pruning, trailing reference preservation, multi-turn conversations whose
  final message has no tool calls, approval request/response pruning, and
  sequential tool-specific pruning rules.
- 2026-05-23: `packages/ai` tool approval prompt utility parity added a
  `validate_tool_context` helper around the existing tool-execution context
  validation path and split the `collectToolApprovals` coverage into named
  upstream counterparts. Rust now has named tests for every upstream
  `generate-text/validate-tool-context.test.ts` case:
  `validate_tool_context_returns_the_tool_context_as_is_when_no_context_schema_is_defined`,
  `validate_tool_context_returns_the_validated_tool_context_when_the_context_schema_matches`,
  and
  `validate_tool_context_throws_type_validation_error_when_the_context_schema_validation_fails`.
  Rust also has named counterparts for every upstream
  `generate-text/collect-tool-approvals.test.ts` case:
  `collect_tool_approvals_should_not_return_any_tool_approvals_when_the_last_message_is_not_a_tool_message`,
  `collect_tool_approvals_should_ignore_approval_request_without_response`,
  `collect_tool_approvals_should_return_approved_approval_with_approved_response`,
  `collect_tool_approvals_should_return_processed_approval_with_approved_response_and_tool_result`,
  and
  `collect_tool_approvals_should_return_denied_approval_with_denied_response`.
  The existing Rust-only invalid-reference error guard remains additive.
- 2026-05-23: `packages/ai` `filterActiveTools` parity split the prior
  grouped Rust coverage into named one-to-one counterparts for every upstream
  `generate-text/filter-active-tools.test.ts` cases:
  `filter_active_tools_should_return_undefined_when_tools_are_not_provided`,
  `filter_active_tools_should_return_all_tools_when_active_tools_is_not_provided`,
  `filter_active_tools_should_return_no_tools_when_active_tools_is_empty`, and
  `filter_active_tools_should_filter_tools_based_on_active_tools`. The filtered
  case includes provider-defined tool preservation with the upstream tool id
  and args shape.
- 2026-05-23: `packages/ai` tool model-output parity added public
  `create_tool_model_output` and `ToolModelOutputErrorMode` in
  `src/generate_text.rs`, with named Rust counterparts for all 21 upstream
  `prompt/create-tool-model-output.test.ts` cases:
  `create_tool_model_output_should_return_error_type_with_string_value_when_is_error_is_true_and_output_is_string`,
  `create_tool_model_output_should_return_error_type_with_json_stringified_value_when_is_error_is_true_and_output_is_not_string`,
  `create_tool_model_output_should_return_error_type_with_json_stringified_value_for_complex_objects`,
  `create_tool_model_output_should_use_tool_to_model_output_when_available`,
  `create_tool_model_output_should_use_tool_to_model_output_with_complex_output`,
  `create_tool_model_output_should_use_tool_to_model_output_returning_content_type`,
  `create_tool_model_output_should_return_text_type_for_string_output`,
  `create_tool_model_output_should_return_text_type_for_string_output_even_with_tool_that_has_no_to_model_output`,
  `create_tool_model_output_should_return_text_type_for_empty_string`,
  `create_tool_model_output_should_return_json_type_for_object_output`,
  `create_tool_model_output_should_return_json_type_for_array_output`,
  `create_tool_model_output_should_return_json_type_for_number_output`,
  `create_tool_model_output_should_return_json_type_for_boolean_output`,
  `create_tool_model_output_should_return_json_type_for_null_output`,
  `create_tool_model_output_should_return_json_type_for_complex_nested_object`,
  `create_tool_model_output_should_prioritize_is_error_over_tool_to_model_output`,
  `create_tool_model_output_should_handle_undefined_output_in_error_text_case`,
  `create_tool_model_output_should_use_null_for_undefined_output_in_error_json_case`,
  `create_tool_model_output_should_use_null_for_undefined_output_in_non_error_case`,
  `create_tool_model_output_should_pass_tool_call_id_to_tool_to_model_output`,
  and `create_tool_model_output_should_pass_input_to_tool_to_model_output`.
  Rust represents JavaScript `undefined` output as `None`, mapping it to
  `unknown error` for text errors and JSON `null` for JSON/non-error outputs,
  matching the upstream `toJSONValue` boundary.
- 2026-05-23: `packages/ai` tool-choice preparation parity added
  `prepare_tool_choice` in `src/prompt.rs`, with named Rust counterparts for
  every upstream `prompt/prepare-tool-choice.test.ts` case: missing tool choice
  defaults to `auto`, string-style `none`, `auto`, and `required` choices keep
  their upstream tagged shapes, and object tool choice preserves `toolName`.
- 2026-05-23: `packages/ai` model resolution parity added
  `src/resolve_model.rs` with `ModelSource` and `ResolvedModel`, plus named
  Rust counterparts for the portable current-provider pieces of upstream
  `model/resolve-model.test.ts`: direct current-version model identity,
  Gateway fallback resolution for language, embedding, image, video, and
  reranking model ids, explicit default-provider resolution for those model
  types, optional transcription/speech resolver coverage for the upstream
  exported functions, and typed missing-model errors for optional video and
  reranking provider support. Legacy v2/v3 adaptation, unsupported-version
  throw checks, and mutable `globalThis.AI_SDK_DEFAULT_PROVIDER` mutation are
  JavaScript object/runtime boundaries and remain documented as non-portable.
- 2026-05-23: `packages/ai` language model call option preparation parity
  added `LanguageModelCallSettings` and `prepare_language_model_call_options`
  in `src/prompt.rs`, with named Rust counterparts for all upstream
  `prepareLanguageModelCallOptions` cases: valid settings, optional undefined
  values, `maxOutputTokens >= 1`, reasoning value passthrough including
  `provider-default` and absence, limited returned values, and Rust
  serde/type-boundary tests for JavaScript dynamic non-number and non-integer
  inputs.
- 2026-05-23: `packages/ai` request timeout helper parity split the
  upstream `prepare-language-model-call-options.test.ts` request-options
  helpers into named Rust counterparts in `src/prompt.rs`: all
  `getToolTimeoutMs`, `getTotalTimeoutMs`, `getStepTimeoutMs`, and
  `getChunkTimeoutMs` undefined, numeric, missing-field, and detailed-object
  cases are now mapped one-to-one. Rust keeps an additive typed per-tool
  override assertion for `{toolName}Ms`.
- 2026-05-23: legacy provider compatibility adapter inventory documented as
  JavaScript-only package compatibility. The upstream `packages/ai/src/model`
  `as-*-v3.test.ts` and `as-*-v4.test.ts` corpus contains 147 cases that
  adapt JavaScript provider object versions v2/v3 into v4 while preserving
  prototype methods, promise-valued capability fields, Web `ReadableStream`
  behavior, and compatibility-warning calls. Rust exposes only the current
  provider-v4 traits and typed model surfaces, so there is no legacy Rust
  v2/v3 object identity or JavaScript prototype boundary to port.
- 2026-05-23: `packages/ai` download utility parity added
  `CreateDownloadOptions`, `DownloadUrlOptions`, `DownloadFunction`,
  `DownloadTransportRequest`, `create_download`, `download`, and
  `download_with_transport` in `src/util.rs`, with named Rust counterparts for
  every portable upstream `util/download/download.test.ts` case: private IPv4
  and localhost SSRF rejection, redirected private/localhost final URL
  rejection, safe redirects, successful bytes/media-type downloads with
  prepared user-agent headers, inline data URLs, non-OK and transport errors,
  default size-limit rejection, and abort-signal propagation to the injected
  transport boundary. Rust keeps JavaScript global `fetch` and exact DOM
  `AbortSignal` object identity as runtime-specific boundaries.
- 2026-05-23: `packages/ai` stitchable stream utility parity added
  `StitchableStream`, `StitchableStreamRead`, and
  `create_stitchable_stream` in `src/util.rs`, with named Rust counterparts
  for every portable upstream `util/create-stitchable-stream.test.ts` case:
  immediate close, one/two/three inner streams, empty inner streams,
  read-before-add behavior, pending reads resolving in order, inner stream
  errors, outer cancellation, add-after-close errors, termination
  cancellation, and add-after-terminate errors. Rust models JavaScript pending
  read promises with an explicit `Pending` read state and keeps Web
  `ReadableStreamDefaultReader` as a JavaScript runtime primitive.
- 2026-05-23: `packages/ai` async iterable stream utility parity added
  `AsyncIterableStream`, `AsyncIterableStreamSource`, and
  `create_async_iterable_stream` in `src/util.rs`, with named Rust
  counterparts for every portable upstream `util/async-iterable-stream.test.ts`
  case: non-empty and empty async iteration, readable-stream collection,
  early-exit cancellation, thrown-error cancellation, natural completion
  without cancellation, no second iteration after break, source error
  propagation, cancellation during active iteration, already-cancelled empty
  iteration, and `return()` after completion. Rust keeps Web
  `ReadableStream`/`TransformStream` objects as JavaScript runtime primitives
  and exposes the portable source/iterator cleanup contract directly.
- 2026-05-23: `packages/ai` server response writer utility parity added
  `WriteToServerResponseOptions`, `ServerResponseWriter`, and
  `write_to_server_response` in `src/util.rs`, with named Rust counterparts
  for every portable upstream `util/write-to-server-response.test.ts` case:
  byte chunk writes, backpressure drain waiting, header/status writing without
  status text, header/status writing with status text, and default status 200
  when status is omitted. Rust keeps Node's concrete `ServerResponse` and
  EventEmitter runtime objects behind a response-writer trait while preserving
  the portable write/backpressure/end contract.
- 2026-05-23: `packages/ai` simulated readable stream utility parity
  added `SimulateReadableStreamOptions`, `SimulatedReadableStream`, and
  `simulate_readable_stream` in `src/util.rs`, with named Rust counterparts
  for every portable upstream `util/simulate-readable-stream.test.ts` case:
  provided chunks, delay sequence injection, empty inputs, generic value
  preservation, both delays set to `null`, null initial delay with chunk delay,
  and initial delay with null chunk delay. Rust exposes a dependency-light
  pull/collect facade instead of a Web `ReadableStream`, preserving the
  portable chunk and delay behavior while documenting the runtime primitive as
  JavaScript-specific.
- 2026-05-23: `packages/ai` prepare retries utility parity added
  `PrepareRetriesOptions`, `PreparedRetries`, and `prepare_retries` in
  `src/util.rs`, with the named Rust counterpart
  `prepare_retries_should_set_default_values_correctly_when_no_input_is_provided`
  for the portable upstream `util/prepare-retries.test.ts` case. Rust verifies
  absent `maxRetries` resolves to the upstream default of 2 and prepares retry
  executor options with the same resolved value. Explicit retry counts are
  typed as `usize`, so upstream JavaScript negative/fractional runtime
  validation is enforced by Rust's type boundary.
- 2026-05-23: `packages/ai` serial job executor utility parity added
  `SerialJobExecutor`, `SerialJobHandle`, `SerialJobResult`, and
  `SerialJobError` in `src/util.rs`, with named Rust counterparts for every
  portable upstream `util/serial-job-executor.test.ts` case. The 6
  `serial_job_executor_should_*` tests cover single job success, multiple jobs
  in serial submission order, job error propagation, one-at-a-time execution,
  mixed success/failure continuation, and queued run calls preserving
  submission order even when later jobs are released first. Rust uses a single
  worker thread and blocking job handles instead of JavaScript promises and
  `DelayedPromise`, while preserving the same serialized execution contract.
- 2026-05-23: `packages/ai` callback utility parity added
  `Callback`, `merge_callbacks`, and `notify` in `src/util.rs`, with named
  Rust counterparts for every portable upstream `util/merge-callbacks.test.ts`
  and `util/notify.test.ts` case. The 3 `merge_callbacks_should_*` tests cover
  starting callbacks together, waiting for all callbacks to settle, continuing
  after callback errors, ignored rejected callbacks, and skipped undefined
  callbacks. The 12 `notify_should_*` tests cover single callback invocation,
  array callback invocation, undefined/omitted callbacks, awaiting async
  callbacks, running async callbacks together, ignoring single/array/async
  callback errors, preserving typed and nested events, and repeated calls with
  the same callback. Rust represents JavaScript rejected promises as
  `CallbackResult::Err` and also settles callback panics to keep the same
  non-breaking generation-flow contract.
- 2026-05-23: `packages/ai` abort utility parity added
  `merge_abort_signals` and `set_abort_timeout` in `src/util.rs`, backed by
  `LanguageModelAbortSignal` follower propagation, with named Rust
  counterparts for every portable upstream
  `util/merge-abort-signals.test.ts` and `util/set-abort-timeout.test.ts`
  case. The 16 `merge_abort_signals_*` tests cover initial state, first and
  second signal aborts, reason preservation including strings, already-aborted
  signals, first already-aborted reason precedence, no valid sources, absent
  source filtering, numeric timeout sources, single-signal identity
  preservation, simultaneous first-reason precedence, and many-signal
  propagation. The 7 `set_abort_timeout_*` tests cover pre-timeout state,
  timeout abort, timeout reason name/message, cancellation, absent controller,
  and absent timeout. Rust represents the timeout reason as JSON with
  `name: "TimeoutError"` and the upstream-shaped message instead of
  JavaScript `DOMException` identity, and uses a cancellable background timer
  handle instead of JavaScript fake timers.
- 2026-05-23: `packages/ai` `generateText` and `streamText` warning logger
  parity added named Rust counterparts
  `generate_text_calls_log_warnings_with_warnings_from_a_single_step`,
  `generate_text_calls_log_warnings_once_for_each_step_with_warnings_from_that_step`,
  `generate_text_calls_log_warnings_with_empty_array_when_no_warnings_are_present`,
  `stream_text_calls_log_warnings_with_warnings_from_a_single_step`,
  `stream_text_calls_log_warnings_once_for_each_step_with_warnings_from_that_step`,
  and `stream_text_calls_log_warnings_with_empty_array_when_no_warnings_are_present`
  for the upstream single-step, per-step multi-step, and empty-warning
  `logWarnings` spy cases. `generate_text` and `stream_text` now invoke the
  shared warning logger with provider/model scope for each completed step.
- 2026-05-24: `packages/ai` `streamText` telemetry integration array parity
  added
  `stream_text_supports_multiple_per_call_telemetry_integrations_as_array`,
  mapping upstream `telemetry integrations > should support multiple per-call
  integrations as an array` by proving both per-call integrations receive
  `onStart` in configured order.
- 2026-05-24: `packages/ai` streamed tool execution context-validation parity
  added
  `stream_text_validates_tool_context_before_approval_callback_and_execution`,
  mapping upstream `executeToolsFromStream`'s context-schema failure before
  approval callbacks by proving invalid `toolsContext` suppresses approval
  callbacks and local tool execution before emitting the tool error.
- 2026-05-24: `packages/ai` `addToolInputExamplesMiddleware` edge-case parity
  expanded with named Rust counterparts for the remaining portable upstream
  cases: missing descriptions, default JSON stringify formatting, function
  tools without examples, empty example arrays, provider tools, mixed tool
  lists, empty tool arrays, and absent tools. The existing default removal,
  custom prefix/formatter, and `remove: false` tests remain the counterpart
  coverage for the upstream option cases.
- 2026-05-24: `packages/ai` `defaultSettingsMiddleware` one-to-one case
  mapping expanded with named Rust counterparts for default application, user
  precedence, provider-options merge, complex/nested provider-options merge,
  zero temperature preservation, max-output-token, stop-sequence, topP,
  header merge, empty header, and empty/absent provider-options cases. The
  upstream untyped `temperature: null as any` case is JavaScript-runtime-only
  at Rust's typed `Option<f64>` boundary and remains documented instead of
  modeled as a valid Rust call option.
- 2026-05-24: `packages/ai` `defaultEmbeddingSettingsMiddleware` one-to-one
  case mapping expanded with named Rust counterparts for the complete portable
  upstream file: header merging, empty default headers, empty param headers,
  absent headers, empty default provider options, empty param provider options,
  and absent provider options. The prior grouped header/provider-option tests
  remain additive coverage.
- 2026-05-24: `packages/ai` `wrapEmbeddingModel` one-to-one case mapping
  expanded with named Rust counterparts for the portable upstream identity,
  provider, max-embeddings, parallel-call, transformParams, wrapEmbed, multiple
  transform, and multiple wrap sequencing cases. Rust represents upstream
  middleware arrays by explicit nested `wrap_embedding_model` composition; the
  upstream "should not mutate the middleware array argument" case is
  JavaScript-array identity behavior and is documented as non-portable at the
  Rust typed-wrapper boundary.
- 2026-05-24: `packages/ai` `wrapImageModel` one-to-one case mapping expanded
  with named Rust counterparts for the portable upstream identity, provider,
  max-images, transformParams, wrapGenerate, stateful max-images, multiple
  transform, and multiple wrap sequencing cases. Rust represents upstream
  middleware arrays by explicit nested `wrap_image_model` composition; the
  upstream "should not mutate the middleware array argument" case is
  JavaScript-array identity behavior and is documented as non-portable at the
  Rust typed-wrapper boundary.
- 2026-05-24: `packages/ai` `wrapLanguageModel` one-to-one case mapping
  expanded with named Rust counterparts for the portable upstream identity,
  provider, supportedUrls, generate/stream transformParams, generate/stream
  wrapping, stateful supportedUrls, multiple transform, and multiple wrap
  sequencing cases. Rust represents upstream middleware arrays by explicit
  nested `wrap_language_model` composition; the upstream "should not mutate the
  middleware array argument" case is JavaScript-array identity behavior and is
  documented as non-portable at the Rust typed-wrapper boundary.
- 2026-05-24: `packages/ai` `extractJsonMiddleware` one-to-one case mapping
  expanded with named Rust counterparts for the current portable upstream
  generate and stream cases, including JSON fences with and without language
  tags, custom transforms, non-text preservation, split fences, non-fence
  backticks, multiple text block IDs, missing text-start deltas, short prefix
  buffers, large and character-by-character content, extra whitespace around
  fences, empty fenced content, and fast non-backtick streaming.
- 2026-05-24: `packages/ai` `extractReasoningMiddleware` one-to-one case
  mapping expanded with named Rust counterparts for the current portable
  upstream generate and stream cases, including single and multiple think tags,
  no-text reasoning output, start-with-reasoning true/false behavior,
  non-reasoning passthrough, split stream tags, and empty think tags.
- 2026-05-23: `packages/ai` `streamText` UI-message response helper
  parity added named Rust counterparts
  `stream_text_result_to_ui_message_stream_masks_error_messages_by_default`,
  `stream_text_result_to_ui_message_stream_supports_custom_error_messages`,
  `stream_text_result_pipe_ui_message_stream_to_response_writes_data_stream_parts`,
  `stream_text_result_pipe_ui_message_stream_to_response_applies_custom_headers`,
  `stream_text_result_pipe_ui_message_stream_to_response_masks_error_messages_by_default`,
  `stream_text_result_pipe_ui_message_stream_to_response_supports_custom_error_messages`,
  `stream_text_result_pipe_ui_message_stream_to_response_omits_finish_when_send_finish_false`,
  `stream_text_result_pipe_ui_message_stream_to_response_writes_reasoning_content`,
  `stream_text_result_pipe_ui_message_stream_to_response_writes_source_content`,
  and `stream_text_result_pipe_ui_message_stream_to_response_writes_file_content`
  for the upstream `result.toUIMessageStream` and
  `result.pipeUIMessageStreamToResponse` error masking, SSE response, option,
  reasoning, source, and file cases. Default UI-message error mapping now
  matches upstream's `An error occurred.` mask unless a custom `onError`
  mapper is supplied.
- 2026-05-23: `packages/ai` `streamText` multiple result consumption
  parity added the named Rust counterpart
  `stream_text_result_supports_text_ui_message_and_full_stream_from_single_result`
  for the upstream case that reads text stream, full stream, and UI-message
  stream from the same result object. Rust proves the equivalent owned-result
  multi-view contract because result streams are materialized rather than
  one-shot JavaScript iterables.
- 2026-05-23: `packages/ai` `streamText` error handling parity added
  named Rust counterparts
  `stream_text_invokes_finish_callback_when_error_chunk_occurs_mid_stream`
  and `stream_text_invokes_error_callback_when_error_occurs_in_second_step`
  for the portable upstream cases where provider error chunks invoke `onError`,
  preserve finish callbacks, and report second-step continuation errors.
- 2026-05-23: `packages/ai` `streamText` `result.fullStream`
  tool-input delta/result parity added named Rust counterparts
  `stream_text_result_full_stream_sends_tool_call_deltas`,
  `stream_text_result_full_stream_passes_provider_metadata_on_tool_input_start`,
  `stream_text_result_full_stream_sends_tool_results`, and
  `stream_text_result_full_stream_sends_delayed_asynchronous_tool_results`.
  Known static tool-input-start parts now carry the upstream `dynamic: false`
  marker, provider metadata is preserved, and asynchronous local tool results
  are emitted before finish-step.
- 2026-05-23: `packages/ai` `streamText` `result.fullStream`
  tool-input refinement parity added the named Rust counterpart
  `stream_text_result_full_stream_refines_tool_input_before_execution_parts_and_callbacks`
  for the upstream case where refined tool input must reach the emitted
  tool-call part, local tool-result part, language-model-call-end callback, and
  tool-execution-start callback.
- 2026-05-23: `packages/ai` `streamText` `result.fullStream`
  tool-call parity added the named Rust counterpart
  `stream_text_result_full_stream_sends_tool_calls` for the upstream snapshot
  case that emits a parsed tool call with the matched high-level tool title,
  provider metadata, required tool choice, and finish metadata.
- 2026-05-23: `packages/ai` `streamText` `result.fullStream`
  fallback response metadata parity added the named Rust counterpart
  `stream_text_result_full_stream_uses_fallback_response_metadata_when_response_metadata_missing`
  for the upstream case where a provider stream omits `response-metadata` and
  `finish-step.response`, `result.response`, and `steps[0].response` still use
  generated fallback id, timestamp, and model id.
- 2026-05-23: `packages/ai` `streamText` `result.fullStream`
  file/reasoning-file parity added named Rust counterparts for upstream
  `generate-text/stream-text.test.ts` cases that stream generated files,
  generated files with provider metadata, and reasoning-files interleaved with
  reasoning and text deltas.
- 2026-05-23: `packages/ai` `streamText` `result.fullStream`
  source/custom parity added named Rust counterparts for upstream
  `generate-text/stream-text.test.ts` cases that stream URL sources around
  text deltas and pass through custom provider parts with provider metadata.
- 2026-05-23: `packages/ai` `streamText` `result.fullStream`
  text/reasoning parity added named Rust counterparts for upstream
  `generate-text/stream-text.test.ts` cases that stream text deltas through
  `fullStream` with response metadata and stream reasoning start/delta/end
  parts, including provider metadata on reasoning/text boundaries.
- 2026-05-23: `packages/ai` `streamText` `result.textStream`
  filtering parity added named Rust counterparts for upstream
  `generate-text/stream-text.test.ts` cases that filter empty text deltas and
  exclude reasoning deltas from `textStream`, while preserving final text,
  reasoning text, and emitted non-empty `TextDelta` parts.
- 2026-05-23: `packages/ai` `cosineSimilarity` utility parity aligned
  named Rust counterparts for every upstream `util/cosine-similarity.test.ts`
  case: positive cosine similarity, negative cosine similarity, mismatched
  vector lengths, zero-vector handling in both argument positions, and very
  small magnitude vectors. Rust also keeps an additional empty-vector guard.
- 2026-05-23: `packages/ai` `getPotentialStartIndex` utility parity split
  named Rust counterparts for every upstream
  `util/get-potential-start-index.test.ts` case: empty searched text, searched
  text absent from text, full match at index zero, and one-, two-, and
  three-character trailing overlap candidates.
- 2026-05-23: `packages/ai` `splitArray` utility parity split named Rust
  counterparts for every upstream `util/split-array.test.ts` case: chunking by
  size, empty input, chunk sizes greater than or equal to the array length,
  chunk size one, zero and negative chunk-size errors, and the upstream
  pre-floored non-integer call-site case.
- 2026-05-22: `packages/ai` token utility parity added named Rust counterparts
  for upstream `generate-text/calculate-tokens-per-second.test.ts` and
  `generate-text/sum-token-counts.test.ts`: average token-rate calculation,
  unknown token count, zero response time, zero response time with unknown
  tokens, unknown duration, summing known token counts, treating one unknown
  token count as zero, and preserving unknown when both counts are unknown.
  The upstream `Number.POSITIVE_INFINITY`/`Number.NaN` token-count case is
  JavaScript-number-specific because Rust token counts are integer usage
  values.
- 2026-05-22: `packages/ai` `prepareHeaders` utility parity added
  `prepare_headers` in `src/util.rs` with named Rust counterparts for every
  portable upstream `util/prepare-headers.test.ts` case: setting a default
  `Content-Type`, preserving an existing content type case-insensitively,
  missing input headers, existing header maps, and response-header-like maps
  with multiple existing values. Rust normalizes header names to lower case for
  deterministic map behavior.
- 2026-05-22: `packages/ai` `isDeepEqualData` utility parity split the
  existing grouped Rust JSON equality checks into named Rust counterparts for
  every portable upstream `util/is-deep-equal-data.test.ts` case: primitive
  equality, different JSON types, null/object comparison, equal objects,
  different values, different key counts, nested object equality and
  inequality, array equality/inequality, repeated null/object comparison, and
  array-vs-object distinction. The upstream prototype, Date object, and
  function comparison cases are JavaScript-runtime-specific and non-portable at
  Rust's `JsonValue` boundary.
- 2026-05-22: `packages/ai` `mergeObjects` utility parity split named Rust
  counterparts for every portable upstream `util/merge-objects.test.ts` case:
  flat object merging without source mutation, deep nested object merging,
  array replacement, null value handling, complex nested replacement/merge
  behavior, empty objects, undefined input equivalents via `None`, top-level
  dangerous key filtering, combined `__proto__`/`constructor`/`prototype`
  filtering, and nested dangerous key filtering. JavaScript Date, RegExp, and
  object-property `undefined` identity semantics are non-portable at Rust's
  `JsonValue` boundary.
- 2026-05-22: OpenAI Responses current upstream corpus audit closed the
  `packages/openai/src/responses` subcorpus. The refreshed upstream inventory
  has 322 explicit `it`/`test` cases across four Responses test files, plus
  four `it.each` reasoning/provider-option matrices; the package-owned
  `ai-sdk-open-responses` crate lists 523 tests and the detailed verified rows
  map every portable current upstream Responses case to named Rust coverage.
  Continue `packages/openai` from non-Responses surfaces unless upstream changes
  this subcorpus.
- 2026-05-22: Gateway package parity audit closed the current upstream
  `packages/gateway` row. The audit counted 372 current upstream
  `it`/`test` cases across `packages/gateway/src/**/*.test.ts` and confirmed
  the package-owned Rust `ai-sdk-gateway` crate now lists 380 tests, with the
  JavaScript-only request-context, callable-constructor, Date-object identity,
  thrown-error identity, cross-realm marker, and stack-trace assertions already
  documented as non-portable. The Gateway row is now `verified`; revisit it
  only if upstream changes or a regression appears.
- 2026-05-22: MCP transport config hosted-auth parity added
  `McpTransportConfig`, `create_mcp_transport`, and
  `McpClientConfig::from_transport_config` now mirror upstream
  `createMcpTransport` for HTTP and SSE configs. The package-owned tests
  `mcp_transport_config_http_builds_authenticated_transport` and
  `mcp_transport_config_sse_builds_authenticated_transport` prove headers and
  OAuth providers propagate through the config/factory path, while
  `mcp_http_transport_rejects_redirects_by_default` covers upstream's default
  redirect rejection.
- 2026-05-22: MCP hosted OAuth HTTP example parity added
  `cargo run -p ai-sdk-mcp --example hosted_oauth_http` now starts a local
  OAuth-protected Streamable HTTP MCP server, performs protected-resource and
  authorization-server discovery, dynamic client registration, PKCE redirect
  and callback exchange, token exchange, auth-provider transport config
  creation, protected tool listing/calling, and authenticated session cleanup
  without external credentials.
- 2026-05-22: Workflow `prepareStep` parity added deterministic Rust
  counterparts for upstream `workflow-agent.test.ts` prepare-step cases in
  `crates/ai-sdk-workflow`: iterator message override, system-after-messages
  override ordering, dynamic model plus generation-setting updates,
  active-tool and tool-choice updates, runtime/tool context updates, and
  agent-level callback forwarding/runtime context propagation.
- 2026-05-22: WorkflowAgent finish callback parity added named Rust
  counterparts for upstream client-side stop and normal tool-completion cases:
  `workflow_agent_upstream_should_call_on_finish_when_stopping_for_client_side_tools`
  and
  `workflow_agent_upstream_should_have_empty_tool_calls_when_all_tools_complete_normally`.
  Rust now exposes constructor and per-stream finish callbacks and verifies
  the callback receives steps/messages while unresolved client-side tool calls
  remain visible.
- 2026-05-22: Workflow stream-text iterator provider-metadata parity added
  named Rust counterparts for upstream multiple-parallel-tool-call and mixed
  metadata/no-metadata continuation cases:
  `stream_text_iterator_upstream_should_preserve_provider_metadata_for_multiple_parallel_tool_calls`
  and
  `stream_text_iterator_upstream_should_handle_mixed_tool_calls_with_and_without_provider_metadata`.
- 2026-05-24: Workflow stream-text iterator OpenAI item-id sanitization parity
  split the remaining upstream provider-metadata cases into named Rust
  counterparts:
  `stream_text_iterator_upstream_should_not_add_provider_options_when_provider_metadata_is_undefined`,
  `stream_text_iterator_upstream_should_strip_openai_item_id_from_provider_metadata_to_avoid_reasoning_item_errors`,
  `stream_text_iterator_upstream_should_preserve_other_openai_metadata_while_stripping_item_id`,
  and
  `stream_text_iterator_upstream_should_preserve_gemini_metadata_while_stripping_openai_item_id_in_mixed_provider_metadata`.
- 2026-05-24: WorkflowAgent option-forwarding parity added named Rust
  counterparts for upstream `workflow-agent.test.ts` generation-settings,
  `toolChoice`, and `activeTools` cases:
  `workflow_agent_upstream_should_pass_generation_settings_from_constructor_to_stream_text_iterator`,
  `workflow_agent_upstream_should_allow_stream_options_to_override_constructor_generation_settings`,
  `workflow_agent_upstream_should_pass_tool_choice_from_constructor_to_stream_text_iterator`,
  `workflow_agent_upstream_should_allow_stream_options_to_override_constructor_tool_choice`,
  and `workflow_agent_upstream_should_filter_tools_when_active_tools_is_specified`.
- 2026-05-24: WorkflowAgent constructor default active-tools parity added
  `workflow_agent_upstream_should_use_constructor_active_tools_when_not_specified_in_stream`,
  mirroring upstream `workflow-agent.test.ts` `should use constructor
  activeTools when not specified in stream()`.
- 2026-05-24: WorkflowAgent tool-execution callback event parity added
  `step_number` to Rust start/end events and named Rust counterparts
  `workflow_agent_upstream_should_pass_step_number_to_tool_execution_start_and_use_success_union_on_end`
  and
  `workflow_agent_upstream_should_pass_success_false_in_tool_execution_end_when_tool_errors`,
  mirroring upstream `workflow-agent.test.ts` callback event `stepNumber` and
  `success: false` error-union cases.
- 2026-05-22: WorkflowAgent ToolLoop compatibility finish-callback parity
  fixed constructor/stream callback merging so constructor callbacks run before
  per-stream callbacks, with named Rust counterparts for upstream
  `onFinish` constructor, method, and combined-order cases.
- 2026-05-22: WorkflowAgent ToolLoop compatibility finish event parity added
  `workflow_agent_compat_should_pass_finish_event_information`, covering
  upstream `onFinish` final text, finish reason, step count, and aggregate
  token usage payload fields.
- 2026-05-22: WorkflowAgent ToolLoop compatibility step-finish parity added
  constructor, stream-method, constructor-then-method ordering, and
  step-result payload callbacks via `WorkflowAgentOnStepFinishCallback`, with
  named Rust counterparts for upstream `onStepFinish` callback cases.
- 2026-05-22: WorkflowAgent ToolLoop compatibility tool-execution callback
  parity added constructor, stream-method, constructor-then-method ordering,
  and start/end event payload callbacks via
  `WorkflowAgentOnToolExecutionStartCallback` and
  `WorkflowAgentOnToolExecutionEndCallback`, with named Rust counterparts for
  upstream `onToolExecutionStart` and `onToolExecutionEnd` stream cases.
- 2026-05-22: WorkflowAgent ToolLoop compatibility start-callback parity
  added constructor, stream-method, constructor-then-method ordering, and
  deterministic event payloads for upstream `experimental_onStart` and
  `experimental_onStepStart` stream cases via `WorkflowAgentOnStartCallback`
  and `WorkflowAgentOnStepStartCallback`.
- 2026-05-22: OpenAI-compatible provider option key normalization added named
  Rust counterparts for all 17 portable upstream
  `packages/openai-compatible/src/utils/to-camel-case.test.ts` cases, covering
  camelCase conversion, raw/camel key resolution, deprecated raw-key warnings,
  and provider-option undefined/no-options branches. The shared resolver and
  warning helpers are now used by chat, completion, embedding, and image
  provider-option paths while preserving raw-then-camel merge precedence.
- 2026-05-21: `streamObject` type-level counterpart parity added typed Rust
  accessors and named tests for portable upstream `stream-object.test-d.ts`
  assertions: `stream_object_type_counterpart_finish_reason_property_has_finish_reason_type`,
  `stream_object_type_counterpart_supports_schema_types`,
  `stream_object_type_counterpart_supports_no_schema_output_mode`,
  `stream_object_type_counterpart_supports_enum_types`, and
  `stream_object_type_counterpart_supports_array_output_mode`. The upstream
  unsupported `timeout` option is already impossible through Rust's
  `StreamObjectOptions` API, which has no timeout field; the named Rust
  counterpart
  `stream_object_type_counterpart_does_not_accept_timeout_option` now makes
  that type-level boundary explicit.
- 2026-05-21: High-level `experimental_telemetry` alias parity added named
  Rust counterparts for upstream deprecated telemetry alias tests across
  `generateText`, `streamText`, `generateObject`, `streamObject`, `embed`,
  `embedMany`, and `rerank`: `generate_text_accepts_experimental_telemetry_alias`,
  `stream_text_accepts_experimental_telemetry_alias`,
  `generate_object_accepts_experimental_telemetry_alias`,
  `stream_object_accepts_experimental_telemetry_alias`,
  `embed_accepts_experimental_telemetry_alias`,
  `embed_many_accepts_experimental_telemetry_alias`, and
  `rerank_accepts_experimental_telemetry_alias`. Each test proves the
  deprecated alias configures telemetry while start callback payloads do not
  expose telemetry option fields.
- 2026-05-21: Gateway language model final `gateway-language-model.test.ts`
  split added named Rust counterparts for portable raw chunk filtering,
  response-metadata timestamp parsing, provider-options passthrough, transport
  failure mapping, and error cause-chain cases. The exact upstream JavaScript
  `Date` object identity case is non-portable because Rust receives serialized
  SSE/JSON values and parses them into `OffsetDateTime`; the exact thrown
  `GatewayAuthenticationError` instance identity case is non-portable because
  Rust transports return typed `Result` errors instead of rejecting with
  JavaScript class instances. Gateway error metadata and stream error parts now
  include `causeMessage` when the underlying fetch/API error has one.
- 2026-05-21: Gateway language model `doStream` parity split added named Rust
  counterparts for 15 portable upstream `gateway-language-model.test.ts`
  streaming cases: text delta streaming, streaming header markers,
  abort-signal body omission, abort-signal forwarding and absent-signal fetch
  behavior, streaming observability request-id headers, rate-limit,
  authentication, invalid-request, and malformed-error conversion, prompt
  preservation without image parts, inline byte image base64 encoding with
  default and specified media types, URL image preservation, and mixed
  text/file content encoding. The parser now accepts upstream Gateway stream
  chunks using `textDelta`, string `finishReason`, and
  `prompt_tokens`/`completion_tokens`, normalizing them into Rust's typed
  provider-v4 stream parts.
- 2026-05-21: Gateway language model `doGenerate` parity split added named
  Rust counterparts for the first 20 portable upstream
  `gateway-language-model.test.ts` cases: `should set basic properties`,
  `should pass headers correctly`, `should extract text response`,
  `should extract usage information`, `should remove abortSignal from the
  request body`, `should pass abortSignal to fetch when provided`, `should not
  pass abortSignal to fetch when not provided`, `should include o11y headers in
  the request`, API-call, malformed, rate-limit, invalid-request, and
  multi-error conversion cases, non-image prompt preservation, inline byte image
  base64 encoding with default and specified media types, URL image
  preservation, mixed content encoding, actual malformed response body
  preservation, and raw non-JSON response body preservation. Rust records
  Gateway errors in typed result metadata instead of throwing JavaScript
  exceptions, and uses explicit `with_vercel_request_id` observability headers
  for the portable o11y request-id counterpart.
- 2026-05-21: Gateway fetch metadata parity split added named Rust
  counterparts for every portable upstream `gateway-fetch-metadata.test.ts`
  case: available-model endpoint construction, pricing/cache-pricing mapping,
  optional pricing and descriptions, modelType preservation and filtering,
  metadata request headers, metadata API/malformed-response errors, existing
  Gateway error preservation, metadata custom transport and empty response,
  credits endpoint/header mapping, credits API errors, malformed string-valued
  credits, credits custom transport, credits malformed error bodies, credits
  existing Gateway error preservation, cause-chain retention, and empty credits.
  The slice also preserves HTTP error cause messages when converting
  `ApiCallError` into `GatewayError`.
- 2026-05-21: Gateway image model parity split added named Rust counterparts
  for every portable upstream `gateway-image-model.test.ts` case: constructor
  properties, max-image no-splitting, custom provider id, request headers,
  full and minimal request bodies, image array results, provider metadata
  present/empty/absent variants, warning variants, response metadata, full and
  partial usage, merged custom headers, observability headers, abort-signal
  forwarding, API/auth error metadata, providerOptions body passthrough,
  alternate model-id headers, complex multi-provider metadata, and file/mask
  base64/URL/providerOptions encoding. Rust represents JavaScript thrown image
  errors through typed `ImageModelResult` Gateway error metadata.
- 2026-05-21: Gateway video model parity split added named Rust counterparts
  for every portable upstream `gateway-video-model.test.ts` case: constructor
  properties, max-video no-splitting, custom provider id, request headers, full
  and minimal request bodies, base64 and URL video results, provider metadata
  present/empty/absent variants, warning variants, response metadata, merged
  custom headers, observability headers, abort-signal forwarding, HTTP API/auth
  error metadata, SSE error events, empty/heartbeat SSE handling,
  providerOptions body passthrough, alternate model-id headers, complex
  multi-provider metadata, and image-to-video file/URL/providerOptions
  encoding. Rust represents JavaScript thrown video errors through typed
  `VideoModelResult` Gateway error metadata.
- 2026-05-21: Gateway reranking model parity split added named Rust
  counterparts for every portable upstream `gateway-reranking-model.test.ts`
  case: request headers, observability headers, ranking extraction,
  documents/query/topN request body shape, providerOptions passthrough, topN
  omission, response headers, providerMetadata extraction, invalid-request and
  internal-server error classification, and `/reranking-model` endpoint
  construction.
- 2026-05-21: Gateway embedding model parity split added named Rust
  counterparts for every portable upstream `gateway-embedding-model.test.ts`
  case: request headers, observability headers, embeddings and usage,
  `values` request body shape, providerOptions passthrough and omission,
  Gateway invalid-request and internal-server error classification, raw
  response-body providerMetadata preservation, and top-level providerMetadata
  extraction.
- 2026-05-21: Gateway `vercel-environment.test.ts` inventory recorded all
  five upstream `getVercelRequestId` cases as JavaScript-only request-context
  plumbing. The upstream helper reads `globalThis[Symbol.for('@vercel/request-context')]`,
  which has no Rust equivalent; portable Rust request-id behavior remains
  covered by explicit `GatewayProviderSettings::with_vercel_request_id` and
  environment fallback tests.
- 2026-05-21: Gateway spend report parity split added named Rust
  counterparts for every portable upstream `gateway-spend-report.test.ts`
  case: required endpoint/query construction, all optional query parameters,
  omitted optional params, empty tags, snake_case response mapping to camelCase
  serialized API shape, credential-type response mapping, grouped model rows,
  empty results, omitted optional metric fields, request headers,
  authentication errors, rate-limit errors, internal server errors, malformed
  JSON error bodies, and custom fetch behavior via `GatewayTransport`.
- 2026-05-21: Gateway generation info parity split added named Rust
  counterparts for every portable upstream `gateway-generation-info.test.ts`
  case: endpoint/method/query construction, snake_case response mapping to
  camelCase serialized API shape, data-envelope unwrapping, no snake_case
  output fields, request headers, authentication errors, internal server
  errors, malformed JSON error bodies, custom fetch behavior via
  `GatewayTransport`, generation id URL encoding, and BYOK generation data.
- 2026-05-21: Gateway metadata/model-construction parity split added named
  Rust counterparts for upstream `gateway-provider.test.ts` metadata error
  conversion, no-double-wrap Gateway error classification, model-specification
  construction, arbitrary model id construction, and non-existent model id
  construction. The JavaScript object-identity assertion is represented by the
  Rust error enum staying typed as `GatewayAuthenticationError` instead of a
  generic response wrapper.
- 2026-05-21: Gateway account API parity split added named Rust counterparts
  for upstream `gateway-provider.test.ts` `getCredits` success,
  authentication error, custom base URL, OIDC auth-header, provider-interface
  availability, and spend-report success, parameter forwarding, custom base URL,
  custom fetch behavior, provider-interface availability, and default-export
  availability.
  The custom fetch case is represented by the Rust provider's injected
  `GatewayTransport` boundary.
- 2026-05-21: Gateway authentication parity split added named Rust
  counterparts for upstream `gateway-provider.test.ts` portable
  `getGatewayAuthToken` credential matrix, empty and whitespace environment
  edge cases, explicit/environment API-key precedence, OIDC fallback, and
  createGateway auth-header construction. Upstream mocked
  `getVercelOidcToken` rejection cases are documented as JavaScript
  OIDC-provider-mock-specific; Rust reads the configured token source directly.
- 2026-05-21: Gateway real-world auth scenario parity split added named Rust
  counterparts for upstream `gateway-provider.test.ts` `should work in Vercel
  deployment with OIDC`, `should work in local development with API key`, and
  `should work with explicit API key override`. The Rust tests are
  `gateway_provider_real_world_vercel_deployment_uses_oidc_authentication`,
  `gateway_provider_real_world_local_development_uses_api_key_authentication`,
  and
  `gateway_provider_real_world_explicit_api_key_override_wins_over_environment`.
- 2026-05-21: Gateway `createGateway` metadata/default-provider parity split
  added named Rust counterparts for upstream `gateway-provider.test.ts`
  available-model metadata fetch, configured and default metadata cache
  refresh behavior, observability headers present and absent, default provider
  export shape, default base URL, empty options, default image/video model
  factories, custom base URL override, and API-key precedence over OIDC.
- 2026-05-21: Gateway `createGateway` model-factory parity split added named
  Rust counterparts for upstream `gateway-provider.test.ts` embedding, image,
  video, reranking, image/video header and transport reuse, observability
  header reuse, and reranking alias cases. The upstream `new` keyword guard is
  JavaScript-callable-constructor-specific and is intentionally non-portable in
  Rust.
- 2026-05-21: Gateway `createGateway` OIDC fallback parity split added a
  named Rust counterpart for upstream `gateway-provider.test.ts` `should use
  OIDC token when no API key is provided`, proving the created language model
  keeps custom base URL/header configuration while resolving OIDC bearer auth,
  Gateway auth-method, protocol, and user-agent headers through injected env.
- 2026-05-21: Gateway `createGateway` language-model configuration parity
  split added a named Rust counterpart for upstream `gateway-provider.test.ts`
  `should create provider with correct configuration`, proving custom base URL,
  API key authorization, custom headers, Gateway protocol/auth/model-id
  headers, user-agent prefixing, and generated text response parsing.
- 2026-05-21: Gateway provider metadata cache parity split added a named
  Rust counterpart for upstream `gateway-provider.test.ts` metadata caching
  with a configured refresh interval, proving immediate cache reuse and
  refresh after expiry. Existing cache-disabled and default-interval tests
  remain additive support.
- 2026-05-21: Gateway provider account-method parity split added named
  Rust counterparts for portable upstream `gateway-provider.test.ts`
  `getCredits`/`getSpendReport` account cases covering credit request auth,
  protocol, auth-method, custom, and user-agent headers plus credit and spend
  endpoint transport error propagation. Existing broader account tests remain
  additive only.
- 2026-05-21: Gateway error type and auth-method parity split added
  one-to-one Rust counterparts for the remaining portable upstream
  `gateway-error-types.test.ts` and `parse-auth-method.test.ts` cases:
  default/custom error values, enum-variant instance checks, contextual auth
  matrix including API-key priority when both credentials were present,
  model-id capture, retryability matrix, response/validation details, exported
  auth-method header name, valid auth methods with extra headers, invalid
  values, missing/nullish headers, and whitespace rejection. JavaScript
  `Error` inheritance, cross-realm symbol markers, and stack-trace shape remain
  JavaScript-runtime-specific and non-portable.
- 2026-05-21: Gateway error edge-case parity split added one-to-one Rust
  counterparts for portable upstream `create-gateway-error.test.ts`,
  `extract-api-call-response.test.ts`, and `as-gateway-error.test.ts` cases:
  empty and null messages, null error types, malformed/null/string/array
  responses, model-not-found param edges, ignored extra fields, cause-message
  preservation, generation id propagation, contextual auth errors, explicit
  parsed data precedence including `null` and `{}`, raw/HTML/malformed/empty
  body fallback, scalar and array response bodies, all Undici timeout codes,
  and non-timeout transport error normalization. JavaScript-only thrown-error
  identity, arbitrary non-Error/null/undefined inputs, and APICallError cause
  object identity remain non-portable under Rust's typed error boundary.
- 2026-05-21: Provider-utils schema parity added Rust Standard Schema support
  plus one-to-one tests for upstream `schema.test.ts` portable `asSchema` and
  Standard Schema cases: default object schema, input JSON Schema conversion,
  draft-07 target propagation, nested object/array closure, valid/invalid
  validation, transform validation, non-Zod vendor detection, and the
  `schema.test-d.ts` `InferSchema<StandardSchema<T>>` type-level counterpart.
  Zod v4 snapshot cases remain documented as JavaScript adapter/runtime
  behavior.
- 2026-05-21: Provider-utils executable-tool parity added one-to-one Rust
  tests for every portable upstream `types/execute-tool.test.ts` and
  `types/executable-tool.test.ts` runtime case, including single final output,
  streamed preliminary outputs repeated as final output, executable detection
  for present/missing executors and missing tools, and the narrowed executable
  tool path into `execute_tool`.
- 2026-05-21: Provider-utils delay parity added one-to-one Rust tests for every
  portable upstream `delay.test.ts` case, including delayed resolution,
  null/undefined immediate resolution, zero and negative delays, abort-before
  and abort-during-delay rejection, abort error name/message parity, no-signal
  completion, large pending delays, multiple simultaneous delays, and
  listener/timer cleanup semantics represented by Rust abort completion and
  post-completion abort no-op behavior.
- 2026-05-21: Provider-utils streaming tool-call tracker parity added
  one-to-one Rust tests for every portable upstream
  `streaming-tool-call-tracker.test.ts` case: incremental and single-chunk
  tool-call assembly, concurrent calls, finished-call skipping, null/absent
  arguments, index fallback, missing id/name validation, all type-validation
  modes, flush finalization/idempotence, provider metadata building/omission,
  and custom generate-id fallback behavior.
- 2026-05-21: Provider-utils GET API helper parity added one-to-one
  Rust tests for every portable upstream `get-from-api.test.ts` case:
  successful GET request preparation with provider-utils user-agent suffix and
  JSON validation, failed status responses as `APICallError`, normalized
  network failures, abort-signal handling before transport execution, removal
  of undefined header entries, and response-handler errors. The upstream
  default-global-fetch case is documented as JavaScript-only because Rust
  requires an injected transport.
- 2026-05-21: Provider-utils blob download parity added one-to-one
  Rust tests for every portable upstream `download-blob.test.ts` case:
  successful blob media type and bytes, non-OK status errors, network error
  cause messages, propagated `DownloadError`s without wrapping, default
  size-limit rejection, SSRF rejections for private IPv4, localhost,
  non-HTTP URLs, redirected private/localhost URLs, safe redirect allowance,
  and `DownloadError` status, cause-message, custom-message, upstream name, and
  Rust type identity. The upstream abort-signal fetch case is documented as
  JavaScript-only because Rust uses an injected transport rather than the Web
  Fetch `AbortSignal` API.
- 2026-05-21: Provider-utils parse JSON parity added one-to-one
  Rust tests for every portable upstream `parse-json.test.ts` case:
  plain and schema-validated `parseJSON`, invalid JSON and validation errors,
  `safeParseJSON` raw-value preservation for success, transforms, parse
  failures and validation failures, nested object/array/discriminated
  union/nullable/union schema cases, and valid/invalid `isParsableJson` checks.
- 2026-05-21: Provider-utils JSON serializability parity added
  one-to-one Rust tests for every portable upstream
  `is-json-serializable.test.ts` case: null/undefined, JSON primitives,
  unsupported JavaScript primitives, arrays, arrays containing non-serializable
  values, plain objects, nested non-serializable object values, and non-plain
  object instances. Rust represents JavaScript-only unsupported runtime values
  through `JsonSerializableValue` so the parity ledger remains explicit.
- 2026-05-21: Provider-utils async iterator stream parity added
  one-to-one Rust tests for every portable upstream
  `convert-async-iterator-to-readable-stream.test.ts` case:
  `iterator.return()` is called on cancel and triggers the generator-style
  cleanup hook, reads stop after cancel, iterators without a return hook cancel
  cleanly, and errors from the return hook are ignored. The exact browser
  `ReadableStream.getReader()` object model remains JavaScript-runtime-specific.
- 2026-05-21: Provider-utils fetch error parity split added one-to-one
  Rust tests for every portable upstream `handle-fetch-error.test.ts` case:
  abort error passthrough, Node `fetch failed`, browser `Failed to fetch`,
  Bun-style `ConnectionRefused`, `ConnectionClosed`, `FailedToOpenSocket`,
  and `ECONNRESET` retryable API-call conversion, plus unknown-error
  passthrough. Existing Rust tests for no-cause TypeError and extra network
  codes remain additive only.
- 2026-05-21: Provider-utils response handler parity split added one-to-one
  Rust tests for every portable upstream `response-handler.test.ts` case:
  JSON value/raw-value extraction, binary byte handling, empty binary body
  `ApiCallError`, and status-code error construction with status text, body,
  URL, status code, and request body values. Existing Rust handler option,
  header, invalid-JSON, validation-error, event-source, and JSON-error tests
  remain additive only.
- 2026-05-21: Provider-utils response size-limit parity split added
  one-to-one Rust tests for every portable upstream
  `read-response-with-size-limit.test.ts` case: successful bounded reads,
  early `Content-Length` rejection, streamed-byte overflow rejection, lying
  content-length rejection, null and zero-length body handling, custom
  `maxBytes` acceptance at the exact limit, and `maxBytes + 1` rejection.
  Existing grouped/default-limit Rust tests remain additive only.
- 2026-05-21: Provider-utils secure JSON parse parity split added
  one-to-one Rust tests for every portable upstream
  `secure-json-parse.test.ts` case: object/null/number/string parsing,
  constructor string/null allowance, object-valued constructor rejection,
  `__proto__` rejection, and unicode-escaped dangerous-key rejection. The
  implementation now rejects any object-valued `constructor` property to match
  upstream; exact JavaScript `SyntaxError` identity is documented as
  JS-runtime-specific.
- 2026-05-21: Provider-utils validateTypes test parity split added
  one-to-one Rust tests for every portable upstream `validate-types.test.ts`
  case: successful `validateTypes`, failed `validateTypes` with
  `TypeValidationError`, successful `safeValidateTypes` with raw input
  preservation, and failed `safeValidateTypes` returning the error object plus
  raw input. Existing Rust context and transformation tests remain additive
  coverage only.
- 2026-05-21: Provider-utils serialized model-options test parity split
  added one-to-one Rust tests for all portable upstream
  `serialize-model-options.test.ts` cases: serializable config retention,
  header resolution boundary, non-serializable function/object filtering,
  primitive array retention, and class-instance filtering. The two upstream
  Promise-returning header cases are documented as JavaScript-only because the
  Rust API accepts already-resolved typed JSON/`None` values and cannot express
  JavaScript functions or promises at this boundary.
- 2026-05-21: Provider-utils download URL validation test parity split
  added one-to-one Rust tests for all portable upstream
  `validate-download-url.test.ts` cases: allowed HTTP/HTTPS/data URLs, public
  IPs and ports, blocked protocols, malformed URLs, localhost-style hostnames,
  private/link-local IPv4 ranges, private/link-local IPv6 ranges, and
  IPv4-mapped IPv6 addresses.
- 2026-05-21: Provider-utils URL support test parity split added
  one-to-one Rust tests for all portable upstream `is-url-supported.test.ts`
  cases: exact media type matching, wildcard media types, combined
  specific/wildcard fallback, empty URL edge cases, case-insensitive matching,
  subtype wildcards, top-level-only media types, and empty URL pattern arrays.
- 2026-05-21: Provider-utils runtime user-agent test parity split added
  one-to-one Rust tests for upstream
  `get-runtime-environment-user-agent.test.ts`: browser, navigator/test,
  Vercel Edge Runtime, and Node.js runtime user-agent cases. Additional Rust
  coverage keeps unknown-runtime, navigator lowercasing, and probe-precedence
  regressions explicit.
- 2026-05-19: Chat transport HTTP request contract parity added
  `http_chat_transport_builds_default_send_messages_request`,
  `http_chat_transport_prepare_send_options_match_upstream_callback_input`,
  and reconnect/prepared override tests in `src/chat_transport.rs`, covering
  upstream `ChatTransport`/`HttpChatTransport` send and reconnect request
  construction without browser fetch/WebStream bindings.
- 2026-05-19: UI-message last-assistant completion predicate parity added the
  initial aggregate checks in `src/ui_message_stream.rs`, covering
  last-step-only tool completion, dynamic tools, provider-executed tool
  exclusion for ordinary tool-completion checks, and approval-response
  terminal-state rules. These aggregate checks were split into one named Rust
  counterpart per upstream case on 2026-05-23.
- 2026-05-19: `streamText.toUIMessageStream` portable non-text chunk parity
  added `stream_text_result_maps_portable_non_text_parts_to_ui_message_stream`
  in `src/stream_text.rs`, covering source gating, file and reasoning-file data
  URLs, custom chunks, tool input/result/approval chunks, provider-executed
  flags, dynamic markers, preliminary outputs, titles, and provider metadata.
- 2026-05-20: `streamText.toUIMessageStream` custom error masking parity
  added `stream_text_result_ui_message_stream_options_mask_errors_with_on_error`
  in `src/stream_text.rs`, covering custom `onError` handling for stream
  errors, invalid tool inputs, local tool errors, and provider-executed tool
  error passthrough.
- 2026-05-20: `streamText.toUIMessageStream` persistence id parity added
  `stream_text_result_ui_message_stream_options_use_persistence_message_ids`
  in `src/stream_text.rs`, covering original-message continuation id reuse,
  generated response ids for new assistant messages, and generated ids for
  non-persistence streams when `generateMessageId` is supplied.
- 2026-05-21: `streamText` retry parity added
  `stream_text_preserves_system_messages_when_retrying_after_retryable_error`
  and strengthened `stream_text_retries_retryable_pre_stream_errors` in
  `src/stream_text.rs`, covering upstream `streamText` retry-after behavior for
  retryable pre-stream 500/429 failures, successful response streaming after
  retry, and preservation of standardized system/user prompts across retry
  attempts.
- 2026-05-24: `streamText` `result.consumeStream` parity added
  `StreamTextResult::consume_stream` and
  `StreamTextResult::consume_stream_with_on_error`, with named Rust
  counterparts
  `stream_text_result_consume_stream_ignores_abort_error_during_stream_consumption`,
  `stream_text_result_consume_stream_ignores_response_aborted_error_during_stream_consumption`,
  `stream_text_result_consume_stream_ignores_any_errors_during_stream_consumption`,
  and
  `stream_text_result_consume_stream_calls_on_error_callback_with_the_error`.
  Rust consumes the materialized stream result and reports provider error JSON
  to the callback instead of exposing Web `ReadableStream` thrown `Error`
  object identity.
- 2026-05-21: `packages/ai` retry utility parity added named Rust counterparts
  for every portable upstream `util/retry-with-exponential-backoff.test.ts`
  case in `src/retry.rs`, including rate-limit `retry-after-ms`, `retry-after`
  seconds, too-long/invalid/negative header fallback, Anthropic and OpenAI 429
  mocked provider responses, multiple retry progression, header precedence,
  HTTP date parsing, Gateway internal-server/rate-limit retry, Gateway
  authentication no-retry, and Gateway retry delay from an API-call cause.
  Rust records injected sleep durations instead of using JavaScript fake
  timers, and the retry attempt model now represents Gateway retryability plus
  cause response headers.
- 2026-05-21: `packages/ai` partial JSON utility test parity added 46
  `fix_json_upstream_*` Rust tests in `src/util.rs`, mapping every portable
  upstream `util/fix-json.test.ts` case one-to-one for empty input, literals,
  number and exponent prefixes, string escape repair, array/object repair,
  nested structures, and regression fixtures. Existing grouped Rust fix-json
  tests remain additive coverage only. The existing `parse_partial_json_*` tests
  already cover every portable upstream `util/parse-partial-json.test.ts` case.
- 2026-05-22: `streamObject` object-stream delta parity added
  `stream_object_object_stream_sends_object_deltas` in `src/stream_object.rs`,
  mapping upstream `output = "object" > result.objectStream > should send
  object deltas` by proving the Rust partial object stream emits the same
  `{}`, partial text-field, and final object sequence while sending a JSON
  response format with no schema name or description.
- 2026-05-22: `generateObject` result and request-option parity split added
  `generate_object_result_contains_request_information`,
  `generate_object_result_contains_response_information`,
  `generate_object_result_contains_provider_metadata`,
  `generate_object_passes_headers_to_model`, and
  `generate_object_passes_provider_options_to_model` in
  `src/generate_object.rs`, mapping upstream `result.request`,
  `result.response`, `result.providerMetadata`, `options.headers`, and
  `options.providerOptions` cases one-to-one while preserving the Rust AI
  user-agent suffix behavior.
- 2026-05-23: `generateObject` callback panic parity added
  `generate_object_callback_panics_do_not_break_generation` in
  `src/generate_object.rs`, mapping upstream
  `callbacks > error handling in callbacks > should not break the generation
  when a callback throws` for synchronous callback failures.
- 2026-05-23: `generateObject` warning logger parity added
  `generate_object_calls_log_warnings_with_the_correct_warnings` and
  `generate_object_calls_log_warnings_with_empty_array_when_no_warnings_are_present`
  in `src/generate_object.rs`, plus scoped warning logger invocation from
  `generate_object`. These map upstream `should call logWarnings with the
  correct warnings` and `should call logWarnings with empty array when no
  warnings are present` by proving the logger receives provider warnings,
  empty-warning calls, provider id, and model id.
- 2026-05-23: `generateText` and `streamText` warning logger parity added
  `generate_text_calls_log_warnings_with_warnings_from_a_single_step`,
  `generate_text_calls_log_warnings_once_for_each_step_with_warnings_from_that_step`,
  `generate_text_calls_log_warnings_with_empty_array_when_no_warnings_are_present`,
  `stream_text_calls_log_warnings_with_warnings_from_a_single_step`,
  `stream_text_calls_log_warnings_once_for_each_step_with_warnings_from_that_step`,
  and `stream_text_calls_log_warnings_with_empty_array_when_no_warnings_are_present`
  in `src/generate_text.rs` and `src/stream_text.rs`, plus scoped warning
  logger invocation from each API. These map upstream `logWarnings` single-step,
  per-step multi-step, and empty-warning spy cases by proving the logger
  receives each step's warnings, empty-warning calls, provider id, and model id.
- 2026-05-24: `streamText` telemetry integration array parity added
  `stream_text_supports_multiple_per_call_telemetry_integrations_as_array` in
  `src/stream_text.rs`, mapping upstream
  `telemetry integrations > should support multiple per-call integrations as an
  array` by proving both integrations receive `onStart` in configured order.
- 2026-05-22: `streamObject` object-stream schema metadata parity added
  `stream_object_object_stream_uses_schema_name_and_description` in
  `src/stream_object.rs`, mapping upstream `output = "object" >
  result.objectStream > should use name and description` by proving the Rust
  call preserves the schema name, schema description, user prompt, and object
  delta stream.
- 2026-05-23: `streamObject` full-stream finish metadata parity added
  `stream_object_result_full_stream_sends_finish_provider_metadata_and_timestamp`
  in `src/stream_object.rs`, mapping the current upstream
  `result.fullStream` snapshot's finish chunk provider metadata and response
  timestamp fields.
- 2026-05-23: `streamObject` callback panic parity added
  `stream_object_callback_panics_do_not_break_stream` in
  `src/stream_object.rs` plus callback invocation guards in
  `src/generate_object.rs`, mapping upstream
  `callbacks > error handling in callbacks > should not break the stream when
  a callback throws` for synchronous callback failures.
- 2026-05-23: `streamObject` warning logger parity added
  `stream_object_calls_log_warnings_with_the_correct_warnings` and
  `stream_object_calls_log_warnings_with_empty_array_when_no_warnings_are_present`
  in `src/stream_object.rs`, plus scoped warning logger invocation from
  `stream_object`. These map upstream `warnings > should call logWarnings with
  the correct warnings` and `warnings > should call logWarnings with empty
  array when no warnings are present` by proving the logger receives the
  provider warnings, empty-warning calls, provider id, and model id.
- 2026-05-21: `streamObject` full-stream result parity added
  `stream_object_result_full_stream_matches_upstream_object_chunks` in
  `src/stream_object.rs`, mapping upstream `result.fullStream` object-output
  ordering with object deltas, text deltas, and final finish metadata.
- 2026-05-21: `streamObject` text response parity added
  `stream_object_result_text_stream_sends_text_stream`,
  `stream_object_result_to_text_stream_response_creates_response_with_text_stream`,
  and
  `stream_object_result_pipe_text_stream_to_response_writes_default_headers_chunks_and_end`
  in `src/stream_object.rs`, mapping upstream `result.textStream`,
  `result.toTextStreamResponse`, and `result.pipeTextStreamToResponse` object
  output cases with default `200` status, `text/plain; charset=utf-8`, chunk
  preservation, and response finalization.
- 2026-05-21: `streamObject` request-option parity added
  `stream_object_passes_headers_to_model` and
  `stream_object_passes_provider_options_to_model` in `src/stream_object.rs`,
  mapping upstream `options.headers` and `options.providerOptions` cases by
  proving user headers and nested provider options reach the model call while
  preserving the Rust AI user-agent suffix behavior.
- 2026-05-21: `streamObject` result metadata parity added
  `stream_object_result_usage_resolves_with_token_usage`,
  `stream_object_result_provider_metadata_resolves_with_provider_metadata`,
  `stream_object_result_response_resolves_with_response_information`, and
  `stream_object_result_request_contains_request_information` in
  `src/stream_object.rs`, mapping upstream `result.usage`,
  `result.providerMetadata`, `result.response`, and `result.request` cases with
  provider usage details, finish provider metadata, response id/model/timestamp
  plus headers, and provider request body propagation.
- 2026-05-21: `streamObject` object and finish-result parity added
  `stream_object_result_object_resolves_with_typed_object`,
  `stream_object_result_object_errors_when_streamed_object_does_not_match_schema`,
  `stream_object_result_object_schema_error_is_observable_without_unhandled_rejection`,
  and `stream_object_result_finish_reason_resolves_with_finish_reason` in
  `src/stream_object.rs`, mapping upstream `result.object` success,
  schema-validation rejection, no-unhandled-rejection schema-failure, and
  `result.finishReason` cases. The two JS promise-rejection checks map to
  Rust's explicit `object: None` plus retained error state.
- 2026-05-21: `streamObject` `onFinish` parity added
  `stream_object_on_finish_is_called_when_valid_object_is_generated` and
  `stream_object_on_finish_is_called_when_object_does_not_match_schema` in
  `src/stream_object.rs`, mapping upstream `options.onFinish` success and
  schema-mismatch cases with callback object/error, finish reason, usage,
  response id/model/timestamp, and finish provider metadata.
- 2026-05-21: `streamObject` custom-schema and error-handling parity added
  `stream_object_custom_schema_sends_object_deltas`,
  `stream_object_error_handling_reports_no_object_when_schema_validation_fails`,
  `stream_object_error_handling_reports_no_object_when_parsing_fails`, and
  `stream_object_error_handling_reports_no_object_when_no_text_is_generated`
  in `src/stream_object.rs`, mapping upstream custom `jsonSchema` object
  deltas/response format plus NoObjectGenerated-style schema-validation,
  parse-failure, and empty-text failures. The JS rejection assertions map to
  Rust's explicit `object: None`, retained error state, response metadata,
  usage, and finish reason.
- 2026-05-22: `streamObject` provider-error stream parity added
  `stream_object_partial_object_stream_suppresses_provider_errors` and
  `stream_object_object_stream_invokes_on_error_callback_with_error` in
  `src/stream_object.rs`, mapping the upstream `partialObjectStream`
  suppression and `onError` callback cases where `doStream` rejects before
  yielding object deltas. Rust models the upstream thrown `doStream` error as a
  provider `Error` stream part because the Rust provider trait returns a typed
  stream result instead of throwing a promise.
- 2026-05-21: `streamObject` array output parity added
  `stream_object_array_three_elements_streams_complete_objects_in_partial_object_stream`,
  `stream_object_array_three_elements_streams_complete_objects_in_text_stream`,
  `stream_object_array_three_elements_has_correct_object_result`,
  `stream_object_array_three_elements_calls_on_finish_with_full_array`,
  `stream_object_array_three_elements_streams_elements_individually`,
  `stream_object_array_single_chunk_streams_complete_objects_in_partial_object_stream`,
  `stream_object_array_single_chunk_streams_complete_objects_in_text_stream`,
  `stream_object_array_single_chunk_has_correct_object_result`,
  `stream_object_array_single_chunk_calls_on_finish_with_full_array`, and
  `stream_object_array_single_chunk_streams_elements_individually` in
  `src/stream_object.rs`, mapping upstream `output = "array"` cases for three
  streamed elements and two elements emitted in one chunk across
  `partialObjectStream`, `textStream`, `result.object`, `onFinish`, and
  `elementStream`.
- 2026-05-21: `streamObject` enum output parity added
  `stream_object_enum_output_streams_value_and_sends_response_format` and
  `stream_object_enum_output_handles_non_ambiguous_values` in
  `src/stream_object.rs`, completing the portable upstream `output = "enum"`
  stream tests alongside the existing incorrect-value and ambiguous-prefix
  cases. The new response-format assertion verifies the upstream JSON schema
  wrapper with `result` enum values, and the new non-ambiguous test maps the
  exact upstream `foobar`/`barfoo` prefix case.
- 2026-05-21: High-level URL-file message parity added
  `generate_object_messages_with_url_file_calls_model_supported_urls`,
  `generate_text_messages_with_url_file_calls_model_supported_urls`,
  `stream_text_messages_with_url_file_calls_model_supported_urls`, and
  `stream_object_messages_with_url_file_calls_model_supported_urls`, mapping
  upstream `options.messages` coverage for image URL prompts against models
  whose `supportedUrls` hooks read model state. Rust has no JavaScript `this`,
  so the equivalent proof is that each high-level API invokes the model trait
  method for URL file prompts and the generation still resolves to the expected
  text or object.
- 2026-05-21: `streamObject` warnings and callback parity added
  `stream_object_warnings_resolve_empty_when_no_warnings_are_present`,
  `stream_object_warnings_resolve_model_warnings`,
  `stream_object_warnings_are_available_to_step_finish_and_finish_callbacks`,
  `stream_object_on_start_runs_before_model_call`,
  `stream_object_on_start_sends_text_prompt_information`,
  `stream_object_on_step_start_runs_before_model_call`,
  `stream_object_on_step_start_provides_step_number_and_model_info`,
  `stream_object_on_step_finish_runs_after_model_call`,
  `stream_object_on_step_finish_provides_raw_object_text_and_usage`, and
  `stream_object_callbacks_fire_in_upstream_order_with_model_call` in
  `src/stream_object.rs`, mapping the upstream warning-resolution cases and
  the portable callback order/event payload cases. The `logWarnings` spy cases
  and callback-throws case now have direct 2026-05-23 Rust counterparts.
- 2026-05-22: `streamObject` callback correlation parity added
  `stream_object_callbacks_correlate_all_events_with_same_call_id` in
  `src/stream_object.rs`, mapping the upstream `callback ordering` case that
  requires `onStart`, `onStepStart`, `onStepFinish`, and `onFinish` to share a
  single generated `callId`.
- 2026-05-21: `streamObject` repair-text parity completed the upstream
  `options.experimental_repairText` block with
  `stream_object_repair_text_repairs_json_parse_error`,
  `stream_object_repair_text_repairs_type_validation_error`,
  `stream_object_repair_text_handles_repair_returning_none`,
  `stream_object_repair_text_repairs_json_wrapped_with_markdown_code_blocks`,
  and `stream_object_repair_text_reports_no_object_when_parsing_still_fails`
  in `src/stream_object.rs`, mapping every portable original upstream repair
  case for JSON parse repair, schema-validation repair, null repair result,
  markdown code-fence cleanup, and failed repaired text.
- 2026-05-20: `streamText` smooth stream chunking parity added
  `smooth_stream_combines_partial_words`,
  `smooth_stream_supports_line_and_pattern_chunking`,
  `smooth_stream_supports_detector_chunking_and_validation`,
  `smooth_stream_preserves_provider_metadata_on_flushed_reasoning_delta`, and
  `stream_text_smooth_stream_transforms_chunks_before_callbacks` in
  `src/stream_text.rs`, covering upstream `smoothStream` word, line, pattern,
  and detector chunking across text and reasoning deltas before `onChunk`.
- 2026-05-21: `streamText` smooth stream delay parity added
  `smooth_stream_marks_detected_chunks_for_default_delay`,
  `smooth_stream_supports_custom_and_null_delay_options`, and
  `stream_text_smooth_stream_waits_after_detected_chunks` in
  `src/stream_text.rs`, covering upstream `smoothStream` default `10ms`, custom
  numeric, and `null` `delayInMs` scheduling after detected smoothed chunks
  while keeping final buffer flushes immediate.
- 2026-05-23: `streamText` smooth stream edge-case parity added
  `smooth_stream_should_split_larger_text_chunks`,
  `smooth_stream_should_keep_longer_whitespace_sequences_together`,
  `smooth_stream_should_flush_text_buffer_before_tool_call_starts`,
  `smooth_stream_should_flush_text_buffer_before_streaming_tool_input_starts`,
  `smooth_stream_should_not_return_chunks_with_just_spaces`,
  `smooth_stream_should_split_text_by_lines_when_using_line_chunking_mode`,
  `smooth_stream_should_handle_text_without_line_endings_in_line_chunking_mode`,
  `smooth_stream_should_support_custom_chunking_regexps_character_level`,
  `smooth_stream_should_change_the_id_when_the_text_part_id_changes`,
  `smooth_stream_should_split_larger_reasoning_chunks`,
  `smooth_stream_should_flush_reasoning_buffer_before_tool_call`,
  `smooth_stream_should_use_line_chunking_for_reasoning`,
  `smooth_stream_should_flush_text_buffer_when_switching_to_reasoning`,
  `smooth_stream_should_flush_reasoning_buffer_when_switching_to_text`,
  `smooth_stream_should_handle_multiple_switches_between_text_and_reasoning`,
  and
  `smooth_stream_preserves_provider_metadata_on_reasoning_start_for_redacted_thinking`
  in `src/stream_text.rs`, completing the portable upstream edge matrix around
  longer text/reasoning chunks, whitespace-only buffering, line-only final
  flushes, regex character chunking, text-id changes, tool-call flushes,
  streamed tool-input flushes, text/reasoning switching, and redacted-thinking
  provider metadata. Upstream `Intl.Segmenter` chunking remains documented as a
  JavaScript runtime boundary for Rust's dependency-light native smoother.
- 2026-05-20: `streamText` arbitrary transform parity added
  `stream_text_transform_updates_text_response_and_callbacks`,
  `stream_text_transform_applies_multiple_transforms_in_order`, and
  `stream_text_transform_updates_tool_calls_and_tool_results` in
  `src/stream_text.rs`, covering Rust-native transforms over collected
  `TextStreamPart`s before replay, ordered transform composition, callback
  visibility, response/step state recomputation, response-message
  reconstruction, and provider-emitted tool call/result transformation.
  `stream_text_transform_updates_finish_metadata_and_usage` and
  `stream_text_transform_can_stop_stream_with_finish_parts` additionally cover
  transformed finish metadata/usage and stop-style truncation with supplied
  finish parts. `stream_text_transform_updates_local_tool_results_after_execution`
  covers local tool-result transformation after execution before callbacks,
  results, and continuation messages. True streaming scheduler timing remains
  an explicit follow-up.
- 2026-05-20: UI-message stream finish callback parity added
  `handle_ui_message_stream_finish_injects_id_and_calls_on_finish` in
  `src/ui_message_stream.rs` plus
  `stream_text_result_ui_message_stream_options_on_finish_receives_persisted_messages`
  and the stream-option branch of `stream_text_result_creates_ui_message_stream_response`
  in `src/stream_text.rs`, covering upstream-style finish callback events,
  response-message id injection, continuation detection, persisted message
  reconstruction, finish reasons, and UI-message stream response helpers that
  honor `toUIMessageStream` options.
- 2026-05-20: UI-message stream creation and step-finish persistence parity
  added `create_ui_message_stream_invokes_step_and_finish_callbacks` and
  `create_ui_message_stream_adds_error_chunk_when_execute_returns_error` plus
  `create_ui_message_stream_adds_error_chunk_when_merged_stream_errors` in
  `src/ui_message_stream.rs`, covering a Rust-native writer/write/merge flow,
  generated response-id injection, `onStepFinish` snapshots across multi-step
  streams, final `onFinish` persisted-message reconstruction, finish reason
  propagation, and custom `onError` masking for fallible execute callbacks and
  merged-stream failures.
- 2026-05-19: Gateway image-model request/response metadata parity added
  `gateway_image_model_maps_upstream_request_response_and_metadata` in
  `crates/ai-sdk-gateway`, covering upstream-style max image splitting behavior,
  request headers/body for image parameters and provider options, returned
  images, response headers/model metadata, warnings, usage, and complex
  provider metadata.
- 2026-05-19: Direct chat transport parity added
  `direct_chat_transport_streams_text_response_from_agent`,
  `direct_chat_transport_passes_abort_signal_to_agent`,
  `direct_chat_transport_passes_prepared_agent_options`,
  `direct_chat_transport_applies_ui_message_stream_options`,
  `direct_chat_transport_converts_ui_messages_to_model_messages_in_order`,
  `direct_chat_transport_rejects_invalid_ui_message_part_shape`, and
  `direct_chat_transport_reconnect_returns_none` in `src/chat_transport.rs`,
  covering upstream `DirectChatTransport`'s portable in-process agent bridge,
  UI-message text conversion, native Rust abort-signal forwarding to the agent
  and model call, Rust agent option forwarding, UI-message stream options,
  validation errors, and reconnect-null behavior.
- 2026-05-19: Assistant tool-history UI-to-model conversion parity added
  `convert_ui_messages_maps_static_tool_output_available_to_assistant_and_tool_messages`,
  `convert_ui_messages_maps_tool_output_error_raw_input_to_error_text`,
  `convert_ui_messages_maps_dynamic_tool_output_available_tool_name`,
  `convert_ui_messages_preserves_step_start_blocks_as_assistant_tool_pairs`,
  `convert_ui_messages_places_provider_executed_tool_result_in_assistant`, and
  `convert_ui_messages_maps_denied_approval_response_to_execution_denied_result`
  in `src/chat_transport.rs`, covering upstream `convertToModelMessages`
  assistant tool-call/tool-result history for static and dynamic tools,
  output-error raw input, step boundaries, provider-executed results, and denied
  approval responses.
- 2026-05-19: Open Responses unsupported assistant prompt part parity added
  `open_responses_provider_ignores_unsupported_assistant_file_parts` in
  `crates/ai-sdk-open-responses/src/open_responses.rs`, covering upstream
  assistant prompt conversion behavior where file and reasoning-file parts are
  ignored while text and tool-call parts still serialize.
- 2026-05-19: Open Responses unsupported standard call option parity added
  `open_responses_provider_warns_for_unsupported_standard_call_options` in
  `crates/ai-sdk-open-responses/src/open_responses.rs`, covering upstream
  `topK`, `seed`, `presencePenalty`, `frequencyPenalty`, and `stopSequences`
  warning behavior in upstream order without leaking those unsupported options
  into the Responses request body.
- 2026-05-20: OpenAI/Gateway Responses no-schema JSON response format parity
  added `open_responses_provider_maps_no_schema_json_format_by_route` in
  `crates/ai-sdk-open-responses/src/open_responses.rs` plus
  `vercel_ai_gateway_openai_responses_maps_no_schema_json_response_format`,
  and ignored live test
  `live_vercel_ai_gateway_openai_responses_no_schema_json_response_format`,
  covering upstream `json_object` request shaping for schema-free JSON response
  format on OpenAI, Azure, and Vercel AI Gateway Responses wrapper routes while
  preserving the generic Open Responses package route's no-schema `json_schema`
  shape. The live Gateway JSON response-format test passed on 2026-05-20.
- 2026-05-20: Chat transport response parser parity added
  `DefaultChatTransport` and `TextStreamChatTransport` wrappers in
  `src/chat_transport.rs`, with
  `default_chat_transport_parses_ui_message_event_stream`,
  `default_chat_transport_reports_invalid_ui_message_event`, and
  `text_stream_chat_transport_maps_text_to_ui_message_stream` covering upstream
  `DefaultChatTransport` UI-message JSON event parsing and
  `TextStreamChatTransport` plain text to UI-message chunk conversion without
  binding Rust to browser `ReadableStream` or fetch.
- 2026-05-20: UI-message file/data prompt conversion parity added
  `convert_ui_messages_skips_unconverted_data_parts` and
  `convert_ui_messages_maps_file_provider_reference_and_metadata_parts` in
  `src/chat_transport.rs`, covering upstream `convertToModelMessages`
  behavior for skipped `data-*` parts when no data converter is supplied,
  file URL/provider-reference mapping, and custom/reasoning provider metadata.
- 2026-05-21: UI-message approval response conversion parity added
  `convert_ui_messages_maps_approved_static_tool_approval_response`,
  `convert_ui_messages_maps_approved_dynamic_tool_approval_response`,
  `convert_ui_messages_preserves_automatic_approval_metadata_for_tool_result`,
  and `convert_ui_messages_marks_provider_executed_denied_approval_response`
  in `src/chat_transport.rs`, plus `providerExecuted` preservation on
  `LanguageModelToolApprovalResponsePart`. These map upstream
  `convert-to-model-messages.test.ts` cases for approved static approvals,
  approved dynamic approvals, automatic approval metadata on approved tool
  results, and provider-executed denied approval responses one-to-one.
- 2026-05-21: UI-message approval follow-up/output-denied parity completed
  the remaining upstream approval-response conversion matrix with
  `convert_ui_messages_maps_denied_static_tool_approval_with_follow_up_text`,
  `convert_ui_messages_maps_denied_dynamic_tool_approval_with_follow_up_text`,
  `convert_ui_messages_maps_static_tool_output_denied`,
  `convert_ui_messages_maps_dynamic_tool_output_denied`,
  `convert_ui_messages_maps_approved_tool_result_with_follow_up_text`, and
  `convert_ui_messages_maps_approved_tool_error_with_follow_up_text` in
  `src/chat_transport.rs`, mapping upstream static/dynamic denied approvals,
  output-denied results, and approved result/error follow-up text cases
  one-to-one.
- 2026-05-21: Provider-executed UI-message tool result parity added exact Rust
  counterparts for upstream `convert-to-model-messages.test.ts` provider-run
  tool output cases: `convert_ui_messages_maps_provider_executed_tool_output_available`,
  `convert_ui_messages_maps_provider_executed_tool_output_error`,
  `convert_ui_messages_propagates_provider_metadata_to_provider_executed_tool_result`,
  and
  `convert_ui_messages_prefers_result_provider_metadata_for_provider_executed_tool_result`.
  These prove provider-executed results stay in assistant content, output
  errors become `error-json`, call provider metadata propagates to the result,
  and result provider metadata takes precedence when present.
- 2026-05-21: UI-message provider metadata conversion parity added named Rust
  counterparts for upstream `convert-to-model-messages.test.ts` system/user/
  assistant provider metadata cases:
  `convert_ui_messages_maps_system_provider_metadata`,
  `convert_ui_messages_merges_system_provider_metadata_from_text_parts`,
  `convert_ui_messages_maps_system_anthropic_cache_control_metadata`,
  `convert_ui_messages_maps_user_text_provider_metadata`,
  `convert_ui_messages_maps_user_file_provider_metadata`,
  `convert_ui_messages_maps_assistant_text_provider_metadata`, and
  `convert_ui_messages_maps_assistant_file_provider_metadata`.
  These cover text/file provider metadata propagation, merged system metadata,
  and Anthropic cache-control metadata without relying on broad grouped tests.
- 2026-05-21: UI-message file conversion parity added named Rust counterparts
  for upstream `convert-to-model-messages.test.ts` file URL, filename, and
  provider-reference cases: `convert_ui_messages_maps_user_file_url_part`,
  `convert_ui_messages_includes_user_file_filename`,
  `convert_ui_messages_maps_user_file_provider_reference`,
  `convert_ui_messages_omits_user_file_filename_when_absent`,
  `convert_ui_messages_maps_assistant_file_url_part`,
  `convert_ui_messages_includes_assistant_file_filename`, and
  `convert_ui_messages_maps_assistant_file_provider_reference`.
  These prove URL-backed files, optional filenames, and provider references
  map one-to-one for user and assistant model messages.
- 2026-05-21: Basic UI-message model conversion parity added named Rust
  counterparts for upstream `convert-to-model-messages.test.ts` simple
  system/user/assistant text, custom assistant, and assistant reasoning cases:
  `convert_ui_messages_maps_simple_system_message`,
  `convert_ui_messages_maps_simple_user_message`,
  `convert_ui_messages_maps_custom_assistant_part`,
  `convert_ui_messages_maps_simple_assistant_text_message`, and
  `convert_ui_messages_maps_assistant_reasoning_parts`.
  These make the baseline text, custom, and reasoning inventory visible as
  individual Rust tests instead of relying only on broader transport tests.
- 2026-05-22: UI-message assistant tool output and conversation splitting
  parity added exact Rust counterparts for upstream
  `convert-to-model-messages.test.ts` cases:
  `convert_ui_messages_maps_tool_output_available_with_provider_metadata`,
  `convert_ui_messages_maps_tool_output_error_input_to_error_text`,
  `convert_ui_messages_maps_tool_invocation_multi_part_response`,
  `convert_ui_messages_maps_empty_tool_invocation_conversation`,
  `convert_ui_messages_maps_multiple_messages_conversation`, and
  `convert_ui_messages_maps_multiple_tool_invocations_with_steps`.
  Together with existing raw-input output-error coverage, these map the next
  upstream assistant tool-output and step-splitting snapshot cases one-to-one.
- 2026-05-22: UI-message mixed text/tool conversation parity added
  `convert_ui_messages_maps_tool_invocations_mixed_with_text` and
  `convert_ui_messages_maps_multiple_tool_invocations_with_trailing_user_message`,
  covering the next two upstream `convert-to-model-messages.test.ts`
  snapshot cases exactly: text around tool calls across `step-start` blocks
  and the same split assistant/tool history followed by a user message.
- 2026-05-22: UI-message incomplete tool filtering parity added
  `ConvertUiMessagesToModelMessagesOptions::ignore_incomplete_tool_calls`
  plus `convert_ui_messages_can_ignore_incomplete_tool_calls`, matching
  upstream `ignoreIncompleteToolCalls: true` for static `input-streaming`,
  static `input-available`, and dynamic `input-available` tool parts.
- 2026-05-22: UI-message dynamic tool parity added
  `convert_ui_messages_maps_dynamic_tool_with_trailing_user_message` and
  `convert_ui_messages_maps_provider_executed_dynamic_tool_with_trailing_user_message`,
  and moved the provider-executed denied approval case onto the same
  `ignore_incomplete_tool_calls` option path as upstream.
- 2026-05-22: UI-message data part conversion completed the portable upstream
  `convert-to-model-messages.test.ts` data-part matrix with named Rust
  counterparts for every user and assistant case: data URL to text,
  no-converter skip, selective conversion, file conversion, multiple data-type
  conversion, no-data-message passthrough, and part-order preservation. The
  Rust hook is `convert_ui_messages_to_model_messages_with_data_part_converter`
  and returns `ConvertedUiMessageDataPart` text or file parts.
- 2026-05-20: Completion transport parity added `CompletionTransport` in
  `src/completion_transport.rs`, with
  `completion_transport_builds_default_request`,
  `completion_transport_builds_prepared_request_with_overrides`,
  `completion_transport_processes_text_stream`,
  `completion_transport_processes_data_event_stream`,
  `completion_transport_reports_data_event_error_chunks`, and
  `completion_transport_reports_invalid_data_event_chunks` covering upstream
  `callCompletionApi` and `processTextStream` portable behavior without binding
  Rust to browser fetch, AbortController, or framework hook state.
- 2026-05-20: Object transport parity added `ObjectTransport` in
  `src/object_transport.rs`, with
  `object_transport_builds_post_request_with_input_body`,
  `object_transport_processes_distinct_partial_json_updates`,
  `object_transport_skips_duplicate_partial_objects`,
  `object_transport_ignores_empty_chunks_until_json_can_be_repaired`, and
  `object_transport_parses_final_json_for_validation_boundary` covering
  upstream `experimental_useObject` portable request and partial-object stream
  behavior without binding Rust to browser fetch, AbortController, Web
  `ReadableStream`, or framework hook state.
- 2026-05-20: Live Gateway object OTel proof added
  `live_vercel_ai_gateway_openai_compatible_generate_object_with_otel` and
  `live_vercel_ai_gateway_openai_compatible_stream_object_with_otel`, extending
  `scripts/check-otel-loopback.sh --live-gateway` so the live proof now covers
  real Gateway OpenAI-compatible `generate_text`, `stream_text`,
  `generate_object`, and `stream_object` calls, root telemetry integration,
  local OTLP receiver export, span-name assertions, and configured function-id
  attributes without printing credentials.
- 2026-05-20: Gateway OpenAI Responses object proof added
  `vercel_ai_gateway_openai_responses_streams_object`,
  `live_vercel_ai_gateway_openai_responses_generate_object`, and
  `live_vercel_ai_gateway_openai_responses_stream_object`, covering structured
  object generation and streamed-object parsing through the real Gateway
  `/v1/responses` route. Both live object tests passed against `.env.local`
  Gateway credentials on 2026-05-20 without printing credentials.
- 2026-05-20: Gateway OpenAI Responses streamed tool-loop coverage added
  `vercel_ai_gateway_openai_responses_runs_stream_text_tool_loop_end_to_end`,
  covering streamed function-call argument deltas, local Rust tool execution,
  item-reference continuation, `function_call_output` continuation, and final
  streamed text through the Gateway `/v1/responses` route. Live Responses
  tool-loop proof is still missing because the real Gateway continuation
  currently rejects the second request with `input.1.output: Invalid input`.
- 2026-05-20: Gateway OpenAI Responses live tool-loop validation added ignored
  `live_vercel_ai_gateway_openai_responses_generate_text_tool_loop` and
  `live_vercel_ai_gateway_openai_responses_stream_text_tool_loop`, using
  `vercelAiGateway.store=false` so the continuation replays the full
  `function_call` plus `function_call_output` shape that the real Gateway
  accepts. Direct isolation showed current Gateway rejects the stored
  `item_reference` continuation shape with `input.1.output: Invalid input`,
  while full function-call replay succeeds.
- 2026-05-20: Gateway OpenAI Responses live OTel proof added ignored
  `live_vercel_ai_gateway_openai_responses_generate_text_with_otel` and
  `live_vercel_ai_gateway_openai_responses_stream_text_with_otel`, extending
  `scripts/check-otel-loopback.sh --live-gateway` so the local OTLP receiver
  proof now covers real Gateway OpenAI Responses generate and stream calls,
  root telemetry integration, exported OTLP wire payloads, operation span
  names, and configured function-id attributes without printing credentials.
- 2026-05-20: Test-server package marked verified
  `crates/ai-sdk-test-server` covers the portable upstream `createTestServer`
  contracts, request inspection, controlled-stream helper, array-stream helper,
  and loopback HTTP proof. Upstream MSW interception and `with-vitest` lifecycle
  hooks are documented as JavaScript-runtime bindings instead of Rust parity
  debt. `cargo test -p ai-sdk-test-server` passed on 2026-05-20.
- 2026-05-20: Legacy OpenTelemetry recorder and local OTLP proof added
  `crates/ai-sdk-otel` now includes a package-owned `LegacyOpenTelemetry`
  recorder for legacy `ai.*` text/tool/object/embedding/reranking spans plus a
  root `create_legacy_open_telemetry_integration` dispatcher adapter. The
  deterministic crate tests cover legacy span attributes and lifecycle cleanup,
  the root adapter test exports dispatcher-produced legacy spans through the
  local OTLP receiver, and `scripts/check-otel-loopback.sh` now runs both root
  dispatcher OTLP export checks. `scripts/check-otel-loopback.sh --live-gateway`
  passed on 2026-05-20, including the new legacy dispatcher export proof and all
  six ignored live Gateway telemetry tests.
- 2026-05-20: Authenticated MCP Streamable HTTP tool proof added
  `mcp_client_runs_authenticated_http_tools_with_output_schema_and_provider_metadata`
  in `crates/ai-sdk-mcp` now exercises a real loopback Streamable HTTP client
  against a local `TcpListener`, sends bearer auth on GET/POST/DELETE requests,
  negotiates session cleanup, lists tools, validates schema-typed
  `structuredContent` output, executes an untyped tool, and asserts MCP provider
  metadata (`clientName`, `toolName`, and `title`) from the package-owned tool
  bridge. The runnable `cargo run -p ai-sdk-mcp --example http_auth_typed_tools`
  example covers the same portable upstream HTTP/auth/output-schema/provider
  metadata categories without requiring hosted credentials.
- 2026-05-22: MCP HTTP transport OAuth provider parity added
  `McpHttpTransport::with_auth_provider` now mirrors upstream
  `HttpMCPTransport` bearer-token injection for stored OAuth tokens and retries
  one 401 response after running the package-owned `auth` flow. The named Rust
  tests `mcp_http_transport_refreshes_oauth_tokens_for_unauthorized_inbound_sse`
  and `mcp_http_transport_refreshes_oauth_tokens_and_retries_unauthorized_post`
  cover protected-resource metadata discovery from `WWW-Authenticate`,
  refresh-token authorization, retried GET/POST requests with the fresh token,
  and authenticated session DELETE cleanup against deterministic loopback HTTP.
  Interactive hosted MCP OAuth and protected live service validation remain in
  progress because they require external user/service credentials.
- 2026-05-22: MCP SSE transport OAuth provider parity added
  `SseMcpTransport::with_auth_provider` now mirrors upstream `SseMCPTransport`
  token-header and one-shot 401 retry behavior for both initial SSE connection
  and endpoint POST requests. The named Rust tests
  `mcp_sse_transport_refreshes_oauth_tokens_for_unauthorized_connect` and
  `mcp_sse_transport_refreshes_oauth_tokens_and_retries_unauthorized_post`
  cover `WWW-Authenticate` protected-resource discovery, refresh-token auth,
  retried connection/POST requests with the fresh bearer token, and parsed
  endpoint POST JSON-RPC responses through deterministic loopback HTTP. Hosted
  interactive MCP OAuth and protected live service proof remain in progress.
- 2026-05-22: MCP transport config hosted-auth parity added
  `McpTransportConfig`, `create_mcp_transport`, and
  `McpClientConfig::from_transport_config` now mirror upstream
  `createMcpTransport` for Streamable HTTP and standalone SSE configs. The
  named Rust tests `mcp_transport_config_http_builds_authenticated_transport`
  and `mcp_transport_config_sse_builds_authenticated_transport` prove headers
  plus OAuth providers propagate through the factory into hosted-auth-shaped
  loopback transports, and `mcp_http_transport_rejects_redirects_by_default`
  covers upstream's default redirect rejection.
- 2026-05-22: MCP hosted OAuth HTTP example parity added
  `cargo run -p ai-sdk-mcp --example hosted_oauth_http` now mirrors the
  portable upstream `examples/mcp/src/mcp-with-auth` client/server flow without
  external credentials: a local protected-resource and authorization server
  performs dynamic client registration, PKCE redirect/callback exchange, token
  exchange, `McpTransportConfig::http(...).with_auth_provider(...)` client
  creation, protected tool listing/calling, and authenticated session cleanup.
  Protected live service proof remains credential-gated.
- 2026-05-20: MCP stdio typed-tool example added
  `cargo run -p ai-sdk-mcp --example stdio_typed_tools` now self-spawns a local
  MCP stdio server using `StdioMcpTransport`, initializes a package-owned client,
  lists cached tool definitions, validates schema-typed `structuredContent`,
  executes an untyped tool, and prints MCP provider metadata without network or
  hosted credentials.
- 2026-05-20: MCP tools validated through Vercel AI Gateway
  `vercel_ai_gateway_openai_compatible_runs_generate_text_with_mcp_tools` now
  proves MCP-created tool definitions can be passed into root `generate_text`,
  serialized through the Vercel AI Gateway OpenAI-compatible tool-call request,
  executed by the package-owned MCP client bridge, and returned to the model as
  tool-role continuation content. The ignored live test
  `live_vercel_ai_gateway_openai_compatible_generate_text_mcp_tool_loop` ran
  against a real Vercel AI Gateway model with `.env.local` credentials and
  asserted the MCP tool call/result path without printing secrets.
- 2026-05-20: MCP SSE typed-tool example added
  `mcp_sse_transport_parses_post_sse_message_responses` extends the bounded
  synchronous `SseMcpTransport` proof to parse POST response `event: message`
  payloads, and `cargo run -p ai-sdk-mcp --example sse_typed_tools` now starts a
  local SSE MCP server, initializes a package-owned client, validates
  schema-typed `structuredContent`, executes an untyped tool, and prints MCP
  provider metadata without hosted credentials.
- 2026-05-21: MCP tool metadata example parity added
  `OPENAI_OUTPUT_TEMPLATE_META_KEY` support in `crates/ai-sdk-mcp` so upstream
  `examples/mcp/src/tool-meta` style `_meta["openai/outputTemplate"]` values
  normalize into MCP app provider metadata and app resource URI discovery. The
  runnable `cargo run -p ai-sdk-mcp --example tool_meta` example lists local MCP
  tools, prints app provider metadata, reads the referenced `ui://` widget
  resource, and verifies the portable tool metadata flow without hosted
  credentials.
- 2026-05-21: MCP image-content example parity added
  `mcp_dynamic_tool_model_output_converts_image_content` and
  `cargo run -p ai-sdk-mcp --example image_content` cover upstream
  `examples/mcp/src/image-content`: a self-spawned stdio MCP server returns
  `type: "image"` content, the package-owned MCP client executes `get-image`,
  and `toModelOutput` parity converts it to AI SDK `file` model output with the
  original `image/png` media type.
- 2026-05-21: MCP server metadata example parity added
  `mcp_client_exposes_initialized_server_info_and_instructions` and
  `cargo run -p ai-sdk-mcp --example server_metadata` cover upstream
  `examples/mcp/src/server-info` and `examples/mcp/src/server-instructions`:
  initialized `serverInfo` and `instructions` are stored on the package-owned
  Rust MCP client, exposed after initialization, and exercised with a local
  `ping` tool without hosted credentials.
- 2026-05-21: MCP multi-step elicitation example parity added
  `cargo run -p ai-sdk-mcp --example elicitation_multi_step` covers upstream
  `examples/mcp/src/elicitation-multi-step`: a deterministic local MCP server
  emits two `elicitation/create` requests during a `create_event` tool call,
  the Rust client accepts both with schema-shaped content, and the example
  asserts both JSON-RPC response payloads plus the final tool output without
  hosted model or user-input dependencies.
- 2026-05-20: `streamText` Rust-native abort handling added
  `stream_text_aborts_before_model_call_and_invokes_on_abort` and
  `stream_text_aborts_after_chunk_callback_and_suppresses_finish` cover a
  package-owned abort controller/signal path, `onAbort` events, abort chunk
  emission before provider calls, abort emission after chunk callbacks, and
  suppression of finish parts for aborted streams.
- 2026-05-20: `streamObject` Rust-native abort handling added
  `stream_object_aborts_before_model_call_and_suppresses_finish` and
  `stream_object_aborts_after_model_call_and_suppresses_finish` cover a
  package-owned abort controller/signal path, abort-shaped error emission
  before provider calls and between returned stream parts, and suppression of
  step-finish/finish callbacks for aborted object streams.
- 2026-05-20: Language-model abort signals now propagate to provider call options
  `LanguageModelCallOptions` now carries a non-serialized abort signal from the
  matching `ai-sdk-provider` crate, while `streamText` and `streamObject`
  forward their Rust abort controller/signal to the provider-facing call
  options. `call_options_carries_abort_signal_without_serializing_it`,
  `stream_text_aborts_after_chunk_callback_and_suppresses_finish`, and
  `stream_object_aborts_after_model_call_and_suppresses_finish` cover this
  path.
- 2026-05-20: Non-language provider call options now carry abort signals
  Embedding, image, speech, transcription, reranking, and video provider-v4
  call option structs now expose a non-serialized `abort_signal` field and
  `with_abort_signal` builder. The provider crate exports package-wide
  `ProviderAbortController`/`ProviderAbortSignal` aliases, and each affected
  model module has abort-signal serialization coverage.
- 2026-05-20: Provider HTTP requests now carry language-model abort signals
  `post_json_to_api` and `ProviderApiRequest` carry a non-serialized abort
  signal, short-circuit pre-aborted requests before transport execution, and
  return upstream-shaped `AbortError` failures when a pending transport is
  aborted. Gateway, OpenAI-compatible, and Open Responses language-model
  adapters now forward `LanguageModelCallOptions.abort_signal` into those
  provider-utils request helpers. `post_json_to_api_aborts_before_transport_call`,
  `post_json_to_api_aborts_pending_transport_when_signal_fires`,
  `gateway_model_passes_typed_gateway_provider_options_for_generate`,
  `gateway_model_passes_typed_gateway_provider_options_for_stream`,
  `openai_compatible_chat_passes_tools_tool_choice_and_provider_options`, and
  `open_responses_provider_warns_for_unsupported_standard_call_options` cover
  this path.
- 2026-05-20: Non-language high-level abort signals now reach provider requests
  `embed`, `embedMany`, `generateImage`, `generateSpeech`, `generateVideo`,
  `transcribe`, and `rerank` now forward Rust abort signals into provider call
  options. Gateway embedding/image/reranking/video and OpenAI-compatible
  embedding/image generation/image edit models pass those signals into
  `ProviderApiRequest`; `PostFormDataToApiOptions` and `PostToApiOptions` now
  retain non-serialized abort signals like `PostJsonToApiOptions`.
- 2026-05-20: URL download callbacks now receive high-level abort signals
  `GenerateVideoDownloadOptions` and `TranscribeDownloadOptions` now mirror
  upstream custom download callback options by carrying the URL and optional
  abort signal into URL-backed video/audio downloads.
- 2026-05-20: OpenAI Responses basic generated-result parity added
  `open_responses_provider_generates_basic_text_response`,
  `open_responses_provider_extracts_basic_text_usage`, and
  `open_responses_provider_extracts_basic_text_response_id_metadata` now map
  the upstream basic text response result tests one-to-one for generated text,
  item id metadata, detailed usage, raw usage, and response id metadata.
- 2026-05-20: OpenAI Responses basic request-body parity added
  `open_responses_provider_sends_model_id_settings_and_input`,
  `open_responses_provider_keeps_temperature_and_top_p_for_gpt_5_1_reasoning_none`,
  `open_responses_provider_removes_unsupported_settings_for_o1`, and 38
  `open_responses_provider_removes_unsupported_settings_for_reasoning_model_*`
  generated tests now map the adjacent upstream request-body block one-to-one.
  Rust request serialization now omits `type: "message"` on role-based
  Responses input messages, matching upstream `convertToOpenAIResponsesInput`.
- 2026-05-20: OpenAI Responses model capability parity added
  `open_responses_provider_adds_encrypted_reasoning_include_for_reasoning_store_false`; `open_responses_provider_omits_encrypted_reasoning_include_for_non_reasoning_store_false`; `open_responses_provider_omits_encrypted_reasoning_include_for_store_true`; `open_responses_provider_allows_force_reasoning_for_unrecognized_model_ids`; `open_responses_provider_sends_xhigh_reasoning_effort_for_codex_max_model`; `open_responses_provider_warns_for_reasoning_effort_on_non_reasoning_models`; `open_responses_provider_applies_openai_model_capability_rules` and
  `open_responses_provider_validates_openai_service_tier_model_capabilities`
  now cover OpenAI/Gateway-specific reasoning-model request shaping:
  temperature/topP stripping except GPT-5.1+ `reasoningEffort: none`,
  non-reasoning-model rejection for provider `reasoningEffort` and
  `reasoningSummary` including upstream's full `nonReasoningModelIds` matrix, dedicated `store: false`/`store: true` encrypted reasoning include request tests, unsupported
  OpenAI Responses presence/frequency penalties, dedicated `forceReasoning`/Codex Max `xhigh` tests, and
  flex/priority `serviceTier` validation.
- 2026-05-24: OpenAI language-model capability matrix parity added
  `openai_language_model_capabilities_is_reasoning_model_matches_upstream_matrix`
  and
  `openai_language_model_capabilities_supports_non_reasoning_parameters_matches_upstream_matrix`,
  mapping upstream `openai-language-model-capabilities.test.ts` table rows
  directly for reasoning-model detection and GPT-5.1+ non-reasoning-parameter
  compatibility.
- 2026-05-24: OpenAI hosted tool type parity added
  `openai_web_search_tool_matches_upstream_tool_type_contract` and
  `openai_local_shell_tool_matches_upstream_tool_type_contract`, mapping
  upstream `tool/web-search.test-d.ts` and `tool/local-shell.test-d.ts`
  `Tool<...>` assertions to Rust provider-tool helper constructors with
  explicit provider ids, names, provider arguments, and serialized tool shapes.
- 2026-05-24: OpenAI chat provider-option extension parity added
  `openai_chat_should_send_max_completion_tokens_extension_setting`,
  `openai_chat_should_send_prediction_extension_setting`,
  `openai_chat_should_send_store_extension_setting`,
  `openai_chat_should_send_metadata_extension_values`,
  `openai_chat_should_send_prompt_cache_key_extension_value`,
  `openai_chat_should_send_prompt_cache_retention_extension_value`,
  `openai_chat_should_send_safety_identifier_extension_value`,
  `openai_chat_should_send_service_tier_flex_processing_setting`, and
  `openai_chat_should_send_service_tier_priority_processing_setting`, plus
  `openai_chat_should_pass_logit_bias_parallel_tool_calls_and_user_settings`,
  `openai_chat_should_send_numeric_logprobs_as_logprobs_and_top_logprobs`,
  `openai_chat_should_send_boolean_logprobs_true_with_zero_top_logprobs`, and
  `openai_chat_should_omit_boolean_logprobs_false`, plus
  `openai_chat_should_not_set_reasoning_effort_when_reasoning_is_provider_default`,
  `openai_chat_should_pass_top_level_reasoning_as_reasoning_effort`,
  `openai_chat_should_prefer_provider_options_reasoning_effort_over_top_level_reasoning`,
  `openai_chat_should_pass_reasoning_effort_setting_from_provider_options`,
  `openai_chat_should_pass_reasoning_effort_setting_from_settings`,
  `openai_chat_should_pass_reasoning_effort_xhigh_setting`, and
  `openai_chat_should_pass_text_verbosity_setting_from_provider_options`, mapping
  upstream `openai-chat-language-model.test.ts` chat extension request-body
  cases for `maxCompletionTokens`, `prediction`, `store`, `metadata`,
  prompt-cache settings, `safetyIdentifier`, basic `serviceTier`
  flex/priority serialization, `logitBias` to `logit_bias`,
  `parallelToolCalls` to `parallel_tool_calls`, `user`, and
  `logprobs`/`top_logprobs` option shaping, plus `reasoning:
  provider-default`, top-level `reasoning`, provider-option
  `reasoningEffort` low/high/xhigh precedence, and `textVerbosity` to
  `verbosity` through the OpenAI provider facade.
- 2026-05-24: OpenAI chat tool request-body parity added
  `openai_chat_should_pass_tools_and_tool_choice`, mapping upstream
  `openai-chat-language-model.test.ts` `should pass tools and toolChoice` to
  the OpenAI provider facade by asserting function-tool JSON Schema request
  shaping and specific `tool_choice` serialization.
- 2026-05-24: OpenAI chat basic generate parity added
  `openai_chat_should_extract_text_response`,
  `openai_chat_should_extract_usage`,
  `openai_chat_should_send_request_body`,
  `openai_chat_should_send_additional_response_information`,
  `openai_chat_should_expose_the_raw_response_headers`,
  `openai_chat_should_pass_the_model_and_the_messages`, and
  `openai_chat_should_pass_headers`, mapping the upstream
  `openai-chat-language-model.test.ts` basic non-streaming chat cases through
  the OpenAI provider facade for generated text content, usage accounting, raw
  request body capture, response id/timestamp/model/body metadata, response
  headers, model/message request shaping, and provider/request header merging.
- 2026-05-24: OpenAI chat finish/tool-result parity added
  `openai_chat_should_support_partial_usage`,
  `openai_chat_should_extract_finish_reason`,
  `openai_chat_should_support_unknown_finish_reason`, and
  `openai_chat_should_parse_tool_results`, mapping upstream
  `openai-chat-language-model.test.ts` partial usage, known and unknown finish
  reason, and generated tool-call parsing cases through the OpenAI provider
  facade.
- 2026-05-24: OpenAI chat annotation/citation parity added
  `openai_chat_should_parse_annotations_and_citations`, mapping upstream
  `openai-chat-language-model.test.ts` `should parse annotations/citations` by
  parsing chat `url_citation` annotations into generated URL source content
  with title preservation through the OpenAI provider facade.
- 2026-05-24: OpenAI chat strict tool-call parity added
  `openai_chat_should_set_strict_with_tool_call` and
  `openai_chat_should_set_strict_for_tool_usage`, mapping upstream
  `openai-chat-language-model.test.ts` strict tool-call request/result cases
  to the OpenAI provider facade by asserting required and specific
  `tool_choice` request bodies and parsed assistant tool-call content.
- 2026-05-24: OpenAI chat usage/provider-metadata parity added
  `openai_chat_should_return_cached_tokens_in_prompt_details_tokens` and
  `openai_chat_should_return_prediction_tokens_in_provider_metadata`, mapping
  upstream `openai-chat-language-model.test.ts` prompt `cached_tokens` usage
  and completion `accepted_prediction_tokens`/`rejected_prediction_tokens`
  provider-metadata cases through the OpenAI provider facade.
- 2026-05-24: OpenAI chat reasoning-token usage parity added
  `openai_chat_should_return_reasoning_tokens_in_provider_metadata`, mapping
  upstream `openai-chat-language-model.test.ts` `should return the reasoning
  tokens in the provider metadata` through the OpenAI provider facade by
  asserting completion `reasoning_tokens` splits output text/reasoning usage
  and preserves the raw usage payload.
- 2026-05-24: OpenAI chat response-format request-body parity added
  `openai_chat_should_not_send_response_format_when_response_format_is_text`,
  `openai_chat_should_forward_json_response_format_as_json_object_without_schema`,
  `openai_chat_should_forward_json_response_format_as_json_object_and_include_schema`,
  `openai_chat_should_use_json_schema_and_strict_with_response_format_json`,
  `openai_chat_should_set_name_and_description_with_response_format_json`, and
  `openai_chat_should_allow_undefined_schema_with_response_format_json_when_structured_outputs_are_enabled`,
  mapping upstream `openai-chat-language-model.test.ts` response-format cases
  for text omission, JSON object fallback, structured JSON Schema request body
  shaping, strict schema defaulting, and name/description passthrough through
  the OpenAI provider facade. The OpenAI facade now marks OpenAI-compatible
  chat settings as structured-output capable so the wrapper route matches
  upstream `@ai-sdk/openai` behavior instead of relying only on generic
  OpenAI-compatible defaults.
- 2026-05-24: OpenAI chat logprobs provider-metadata parity added
  `openai_chat_should_extract_logprobs_provider_metadata` and
  `openai_chat_stream_should_extract_logprobs_provider_metadata`, mapping
  upstream `openai-chat-language-model.test.ts` non-streaming
  `should extract logprobs` and streaming `should stream text deltas`
  provider-metadata assertions by storing `choice.logprobs.content` under
  `providerMetadata.openai.logprobs` for OpenAI chat generate and stream
  results.
- 2026-05-24: OpenAI chat stream request and finish parity added
  `openai_chat_stream_should_stream_text_deltas`,
  `openai_chat_stream_should_handle_error_stream_parts`,
  `openai_chat_stream_should_send_request_body`,
  `openai_chat_stream_should_expose_the_raw_response_headers`,
  `openai_chat_stream_should_pass_the_messages_and_the_model`,
  `openai_chat_stream_should_pass_headers`,
  `openai_chat_stream_should_return_cached_tokens_in_provider_metadata`,
  `openai_chat_stream_should_return_prediction_tokens_in_provider_metadata`,
  `openai_chat_stream_should_send_store_extension_setting`,
  `openai_chat_stream_should_send_metadata_extension_values`,
  `openai_chat_stream_should_send_service_tier_flex_processing_setting`, and
  `openai_chat_stream_should_send_service_tier_priority_processing_setting`,
  mapping the upstream non-Responses OpenAI chat streaming text, error, request
  body, raw headers, request headers, streamed usage/provider metadata, store,
  metadata, and service-tier cases. The OpenAI provider now enables upstream
  `stream_options.include_usage` for OpenAI chat and completion streams.
- 2026-05-24: OpenAI chat streaming annotation/tool-call edge parity added
  `openai_chat_stream_should_stream_annotations_and_citations`,
  `openai_chat_stream_should_stream_tool_deltas`,
  `openai_chat_stream_should_stream_tool_call_deltas_when_arguments_are_in_first_chunk`,
  `openai_chat_stream_should_not_duplicate_tool_calls_after_completed_empty_chunk`,
  `openai_chat_stream_should_stream_tool_call_with_missing_type_field`, and
  `openai_chat_stream_should_stream_tool_call_that_is_sent_in_one_chunk`,
  mapping the adjacent upstream `doStream` citation and function-tool delta
  cases in the OpenAI facade. The shared OpenAI-compatible chat stream parser
  now emits `Source` parts for streamed `delta.annotations` URL citations
  instead of only supporting non-streaming message annotations.
- 2026-05-24: OpenAI chat model-specific request rule parity added
  `openai_chat_reasoning_model_should_clear_unsupported_standard_settings`,
  `openai_chat_reasoning_model_should_convert_max_output_tokens_to_max_completion_tokens`,
  `openai_chat_should_allow_temperature_when_reasoning_none_on_gpt_5_1`,
  `openai_chat_should_still_clear_temperature_when_reasoning_none_on_o4_mini`,
  `openai_chat_should_allow_forcing_reasoning_behavior_for_unrecognized_model_ids`,
  `openai_chat_should_remove_temperature_setting_for_search_preview_models`, and
  `openai_chat_should_warn_and_remove_unsupported_service_tier_settings`, plus
  `openai_chat_reasoning_model_should_clear_unsupported_logit_bias_and_logprobs_settings`, mapping
  upstream `openai-chat-language-model.test.ts` reasoning-model, search-preview,
  forced-reasoning, logprobs/logit-bias/top-logprobs pruning warnings, and
  service-tier request pruning/warning cases for the non-Responses OpenAI chat
  facade.
- 2026-05-24: OpenAI chat system-message mode parity added
  `openai_chat_should_default_system_message_mode_to_developer_when_forcing_reasoning`,
  `openai_chat_should_use_developer_messages_for_o1`,
  `openai_chat_should_allow_overriding_system_message_mode_via_provider_options`,
  `openai_chat_should_use_default_system_message_mode_when_not_overridden`, and
  `openai_chat_should_remove_system_messages_when_requested`, mapping upstream
  `convert-to-openai-chat-messages.test.ts` and `openai-chat-language-model.test.ts`
  system/developer/remove message-mode behavior for the non-Responses OpenAI
  chat facade while preserving generic OpenAI-compatible system-message output.
- 2026-05-20: OpenAI Responses hosted tool include parity added
  `open_responses_provider_adds_hosted_tool_include_options` now covers
  upstream automatic `include` additions for hosted web-search action sources
  and code-interpreter outputs while preserving caller-specified include
  entries.
- 2026-05-20: OpenAI Responses system-message mode parity added
  `open_responses_provider_sends_instructions_from_system_message`,
  `open_responses_provider_joins_multiple_system_messages_with_newlines`,
  `open_responses_provider_converts_openai_message_chain_with_system_input_items`,
  and `open_responses_provider_maps_openai_system_message_modes` now split the
  generic Open Responses route from the OpenAI/Gateway wrapper route: generic
  providers keep system messages in top-level `instructions`, while
  OpenAI/Gateway Responses send `system`/`developer` input items and honor
  `systemMessageMode: remove` with the upstream warning.
- 2026-05-20: OpenAI Responses provider option edge coverage added
  `open_responses_provider_maps_openai_passthrough_option_edges` now directly
  covers upstream passthrough request options for `instructions`, multi-value
  `include`, `user`, `conversation`, `metadata`, `store`, `truncation`, and
  numeric `logprobs`, including the automatic logprobs include merge.
- 2026-05-20: Azure Responses provider-option fallback parity added
  `open_responses_provider_falls_back_to_openai_options_for_azure_requests` and
  `open_responses_provider_prefers_azure_options_over_openai_fallback` now cover
  upstream Azure Responses fallback to `providerOptions.openai` when
  `providerOptions.azure` is absent, while keeping provider metadata under the
  `azure` key and ensuring Azure-specific options win when present.
- 2026-05-20: Azure Responses provider-metadata key parity added
  `open_responses_provider_uses_azure_metadata_key_for_function_call_content`
  and
  `open_responses_provider_streams_azure_metadata_key_for_reasoning_and_finish`
  now cover upstream Azure metadata-key selection for generated tool-call
  content, streamed reasoning, and stream finish metadata without also emitting
  `openai` metadata.
- 2026-05-22: Azure Responses top-level provider-metadata key parity added
  `open_responses_provider_uses_azure_metadata_key_for_text_result`
  now covers the upstream `azure.responses` text-result case directly: the
  generated result uses the `azure` metadata key for `responseId` and text item
  metadata, and does not emit parallel `openai` metadata.
- 2026-05-20: OpenAI Responses web-search schema resilience added
  `open_responses_provider_maps_web_search_api_sources`,
  `open_responses_provider_maps_web_search_missing_action`,
  `open_responses_provider_streams_web_search_action_query`, and
  `open_responses_provider_streams_web_search_missing_action` now cover the
  separate upstream Responses web-search API-typed source, missing-action,
  streaming action-query, and streaming missing-action tests one-to-one.
- 2026-05-20: OpenAI Responses error data edge parity added
  `open_responses_provider_maps_openai_numeric_error_code` and
  `open_responses_provider_streams_openai_error_event_without_synthetic_message`
  now cover upstream OpenAI error payloads with numeric `code` fields and SSE
  `type: "error"` events without synthesizing an extra top-level message.
- 2026-05-20: OpenAI Responses failed stream incomplete-details parity added
  `open_responses_provider_streams_failed_response_incomplete_details_finish_reason`
  now maps upstream `should expose raw finish reason from response.failed
  incomplete details`: a streamed `error` part is preserved, and the later
  `response.failed` incomplete reason `max_output_tokens` becomes the raw finish
  reason with unified `length`.
- 2026-05-20: OpenAI Responses streaming context-management parity added
  `open_responses_provider_streams_context_management_options` now covers
  upstream streaming Responses request bodies that forward
  `contextManagement` compaction options as `context_management` without
  leaking the camelCase provider option key.
- 2026-05-20: OpenAI Responses conversation conflict warning parity added
  `open_responses_provider_warns_for_conversation_with_previous_response_id`
  now gives the upstream `conversation` plus `previousResponseId` warning its
  own Rust test, instead of relying only on broader prompt-history coverage.
- 2026-05-20: OpenAI Responses store/include parity split added
  `open_responses_provider_adds_encrypted_reasoning_include_for_reasoning_store_false`,
  `open_responses_provider_omits_encrypted_reasoning_include_for_non_reasoning_store_false`,
  and `open_responses_provider_omits_encrypted_reasoning_include_for_store_true`
  now map the upstream `store` option tests one-to-one instead of relying only
  on the broader model-capability matrix.
- 2026-05-20: OpenAI Responses reasoning option test split added
  `open_responses_provider_allows_force_reasoning_for_unrecognized_model_ids`
  and `open_responses_provider_sends_xhigh_reasoning_effort_for_codex_max_model`
  now map the upstream `forceReasoning` and Codex Max `xhigh` request-body
  tests one-to-one instead of relying only on the broader model-capability
  matrix.
- 2026-05-20: OpenAI Responses non-reasoning model matrix test added
  `open_responses_provider_warns_for_reasoning_effort_on_non_reasoning_models`
  now maps upstream's `it.each(nonReasoningModelIds)` rejection test
  one-to-one across every non-reasoning Responses model id, including
  `gpt-5-chat-latest`.
- 2026-05-21: OpenAI Responses reasoning model table rows split
  Added one Rust test function per upstream
  `it.each(openaiResponsesReasoningModelIds)` row and per
  `it.each(nonReasoningModelIds)` row, so the provider-option model matrices
  are no longer represented only by broad loop coverage. The new ledger row
  lists all 38 reasoning-row tests and all 22 non-reasoning-row tests by exact
  Rust test name.
- 2026-05-21: OpenAI Responses generate error throw-equivalent added
  `open_responses_provider_maps_generate_error_fixture_like_upstream_throw_case`
  now maps upstream `doGenerate > errors > should throw an error` to the Rust
  provider trait's error-result shape while asserting the same OpenAI error
  message is retained in metadata and parsed response body.
- 2026-05-20: OpenAI Responses top-level reasoning parity split added
  `open_responses_provider_omits_provider_default_top_level_reasoning_for_openai`,
  `open_responses_provider_maps_top_level_reasoning_none_for_openai`,
  `open_responses_provider_maps_top_level_reasoning_minimal_for_openai`,
  `open_responses_provider_maps_top_level_reasoning_low_for_openai`,
  `open_responses_provider_maps_top_level_reasoning_medium_for_openai`,
  `open_responses_provider_maps_top_level_reasoning_high_for_openai`,
  `open_responses_provider_maps_top_level_reasoning_xhigh_for_openai`,
  `open_responses_provider_prefers_provider_reasoning_effort_over_top_level_for_openai`,
  `open_responses_provider_strips_temperature_and_top_p_for_top_level_reasoning_model`,
  and `open_responses_provider_keeps_sampling_parameters_for_top_level_reasoning_none`
  now map upstream OpenAI Responses top-level reasoning tests one-to-one while
  preserving generic `@ai-sdk/open-responses` minimal-to-low coercion.
- 2026-05-20: OpenAI Responses client tool-search stream id parity added
  `open_responses_provider_streams_client_tool_search_uses_final_call_id` now
  maps upstream `doStream > tool search tool > should use the final tool search
  call_id when the streamed provisional id changes`: client-executed streamed
  `tool_search_call` parts emit tool-input start/end, tool call, and tool
  result parts with the final `call_id`, preserve item metadata and returned
  tool definitions, and assert the provisional id does not leak.
- 2026-05-20: OpenAI Responses client tool-search fixture parity added
  `open_responses_provider_streams_client_tool_search_fixture_as_non_provider_executed_parts`,
  `open_responses_provider_streams_client_tool_search_omits_provider_executed_flag`,
  `open_responses_provider_streams_client_tool_search_uses_final_call_id`,
  and
  `open_responses_provider_streams_function_call_after_client_tool_search_output`
  now map upstream's fixture-backed client `tool_search` stream tests
  one-to-one for
  non-provider-executed start/end/call parts, absent `providerExecuted` flags,
  final done-chunk `call_id` selection, and the follow-up streamed
  `get_weather` function call after client tool-search output.
- 2026-05-20: OpenAI Responses file-search stream fixture parity added
  `open_responses_provider_streams_file_search_without_results_include` and
  `open_responses_provider_streams_file_search_with_results_include` now map
  upstream `openai-file-search-tool.1` and `openai-file-search-tool.2`
  streaming fixtures, including null results when no results include is sent,
  included result field mapping when `file_search_call.results` is requested,
  provider-executed tool-call/result stream parts, and request include
  forwarding.
- 2026-05-22: OpenAI Responses file-search fixture parity hardened the generated
  and streamed file-search tests to consume byte-matched local copies of
  upstream `openai-file-search-tool.1` and `openai-file-search-tool.2`
  JSON/chunks fixtures, deriving ids, text, usage, query arrays, and result
  payloads from the fixtures instead of shortened reconstructed bodies.
- 2026-05-22: OpenAI Responses image-generation streaming fixture parity
  hardened `open_responses_provider_streams_image_generation_fixture_results`
  to consume a byte-matched local copy of upstream
  `openai-image-generation-tool.1.chunks.txt`, deriving the image item id,
  preliminary/final image payloads, and empty assistant message id from the
  fixture instead of reconstructed SSE literals.
- 2026-05-22: OpenAI Responses local-shell streaming fixture parity hardened
  `open_responses_provider_streams_local_shell_fixture_call` to consume a
  byte-matched local copy of upstream `openai-local-shell-tool.1.chunks.txt`,
  deriving response metadata, reasoning id, local-shell item/call ids, action,
  usage, and service-tier assertions from the fixture instead of reconstructed
  SSE literals.
- 2026-05-22: OpenAI Responses custom-tool fixture parity hardened the
  generated custom-tool tests and `open_responses_provider_streams_custom_tool_fixture`
  to consume byte-matched local copies of upstream
  `openai-custom-tool.1.json` and `openai-custom-tool.1.chunks.txt`, deriving
  response metadata, custom-tool ids, input deltas, final input, and usage from
  the fixtures instead of reconstructed JSON/SSE literals.
- 2026-05-22: OpenAI Responses client tool-search streaming fixture parity
  hardened the client tool-search stream tests to consume byte-matched local
  copies of upstream `openai-client-tool-search.1.chunks.txt` and
  `openai-client-tool-search.2.chunks.txt`, deriving provisional/final
  tool-search call ids, item ids, arguments, and follow-up function-call input
  from the fixtures instead of reconstructed SSE literals.
- 2026-05-22: OpenAI Responses client tool-search step-2 JSON fixture parity
  added `open_responses_provider_generates_function_call_after_client_tool_search_output_fixture`
  against a byte-matched local copy of upstream
  `openai-client-tool-search.2.json`, deriving the follow-up function-call id,
  item id, arguments, response metadata, and usage from the fixture.
- 2026-05-22: OpenAI Responses apply-patch fixture parity hardened the
  generated create-file tests plus create/delete streaming tests to consume
  byte-matched local copies of upstream `openai-apply-patch-tool.1.json`,
  `openai-apply-patch-tool.1.chunks.txt`, and
  `openai-apply-patch-tool-delete.1.chunks.txt`, deriving ids, operations,
  streamed diff deltas, metadata, and usage from the fixtures instead of
  reconstructed JSON/SSE literals.
- 2026-05-22: OpenAI Responses code-interpreter streaming fixture parity
  hardened `open_responses_provider_streams_code_interpreter_results_with_annotations`
  to consume a byte-matched local copy of upstream
  `openai-code-interpreter-tool.1.chunks.txt`, deriving response metadata,
  all code-interpreter call ids/code/container/output payloads, final message
  text, citation metadata, and usage from the fixture instead of reconstructed
  SSE literals.
- 2026-05-22: OpenAI Responses MCP streaming fixture parity hardened
  `open_responses_provider_streams_mcp_tool_fixture` to consume a byte-matched
  local copy of upstream `openai-mcp-tool.1.chunks.txt`, deriving response
  metadata, reasoning ids, MCP call ids/names/arguments/output payloads, final
  message text, and usage from the fixture instead of reconstructed SSE
  literals.
- 2026-05-22: OpenAI Responses shell streaming fixture parity hardened
  `open_responses_provider_streams_shell_fixture_multiresponse` to consume a
  byte-matched local copy of upstream `openai-shell-tool.1.chunks.txt`,
  deriving both streamed response metadata records, shell call id/action,
  final message text, and usage from the fixture instead of reconstructed SSE
  literals.
- 2026-05-22: OpenAI Responses shell container streaming fixture parity
  hardened `open_responses_provider_streams_shell_container_fixture` to consume
  a byte-matched local copy of upstream `openai-shell-container.1.chunks.txt`,
  deriving response metadata, provider-executed shell call/action, shell output
  payload, streamed assistant text, and usage from the fixture instead of
  reconstructed SSE literals.
- 2026-05-22: OpenAI Responses shell container multiturn streaming fixture
  parity hardened `open_responses_provider_streams_shell_container_multiturn_fixture`
  to consume a byte-matched local copy of upstream
  `openai-shell-container-multiturn.1.chunks.txt`, deriving response metadata,
  streamed assistant text, service tier, and usage from the fixture instead of
  reconstructed SSE literals.
- 2026-05-22: OpenAI Responses shell local multiturn streaming fixture parity
  hardened `open_responses_provider_streams_shell_local_multiturn_fixture` to
  consume a byte-matched local copy of upstream
  `openai-shell-local-multiturn.1.chunks.txt`, deriving response metadata,
  streamed assistant text, service tier, and usage from the fixture instead of
  reconstructed SSE literals.
- 2026-05-22: OpenAI Responses shell skills fixture parity hardened
  `open_responses_provider_generates_shell_environment_fixture_content` and
  `open_responses_provider_streams_shell_environment_fixture` to consume
  byte-matched local copies of upstream `openai-shell-skills.1.json` and
  `openai-shell-skills.1.chunks.txt` instead of reconstructed response bodies.
- 2026-05-20: OpenAI Responses code-interpreter annotation stream fixture
  parity added
  `open_responses_provider_streams_code_interpreter_results_with_annotations`
  now maps upstream `openai-code-interpreter-tool.1` by proving streamed code
  input completion, code-interpreter output mapping, `container_file_citation`
  document source emission, and final text annotation metadata preservation.
- 2026-05-20: OpenAI Responses image-generation stream fixture parity added
  `open_responses_provider_streams_image_generation_fixture_results` now maps
  upstream `openai-image-generation-tool.1` by proving provider-executed
  image-generation tool calls, preliminary partial-image tool results, final
  image result mapping, and empty assistant message text-start/text-end
  metadata.
- 2026-05-20: OpenAI Responses local-shell generated fixture parity added
  `open_responses_provider_generates_local_shell_fixture_call` now maps
  upstream non-streaming `openai-local-shell-tool.1` by proving local-shell
  request-tool mapping, empty reasoning metadata, local-shell action JSON
  mapping to the configured `shell` tool name, absence of a synthetic tool
  result, OpenAI item metadata, and cached/reasoning usage.
- 2026-05-20: OpenAI Responses local-shell stream fixture parity added
  `open_responses_provider_streams_local_shell_fixture_call` now maps upstream
  `openai-local-shell-tool.1` by proving local-shell request-tool mapping,
  reasoning metadata start/end preservation, streamed `local_shell_call`
  action JSON mapping to the configured `shell` tool name, absence of a
  synthetic tool result, and final response metadata/usage/service-tier
  preservation.
- 2026-05-20: OpenAI Responses shell stream fixture parity added
  `open_responses_provider_streams_shell_fixture_multiresponse` now maps
  upstream `openai-shell-tool.1` by proving shell request-tool mapping,
  streamed shell-call command action mapping, multiple response metadata
  emissions for the tool-call and follow-up assistant Responses objects,
  follow-up text streaming, absence of a synthetic local shell result, and
  final response usage/service-tier preservation.
- 2026-05-20: OpenAI Responses shell generated fixture parity added
  `open_responses_provider_generates_shell_fixture_request_body` and
  `open_responses_provider_generates_shell_fixture_call` now map upstream
  `openai-shell-tool.1` by proving shell request-tool mapping, non-streaming
  shell-call command action mapping, OpenAI item metadata preservation,
  absence of a synthetic local shell result, and cached/reasoning usage.
- 2026-05-20: OpenAI Responses shell container generated fixture parity added
  `open_responses_provider_generates_shell_container_fixture_request_body` and
  `open_responses_provider_generates_shell_container_fixture_content` now map
  upstream `openai-shell-container.1` by proving `containerAuto` request
  mapping, provider-executed shell call/result mapping with `exitCode`, final
  assistant text preservation, and cached/reasoning usage.
- 2026-05-20: OpenAI Responses shell container stream fixture parity added
  `open_responses_provider_streams_shell_container_fixture` now maps upstream
  `openai-shell-container.1` by proving `containerAuto` request mapping,
  provider-executed shell call streaming, shell output result mapping with
  `exitCode`, follow-up assistant text streaming, and final response
  metadata/usage/service-tier preservation.
- 2026-05-20: OpenAI Responses shell container multiturn generated fixture
  parity added
  `open_responses_provider_generates_shell_container_multiturn_fixture_request_body`
  and `open_responses_provider_generates_shell_container_multiturn_fixture_content`
  now map upstream `openai-shell-container-multiturn.1` by proving stored
  shell-call item references, shell-output history replay, `containerAuto`
  request mapping, final assistant text preservation, and cached/reasoning
  usage.
- 2026-05-20: OpenAI Responses shell container multiturn stream fixture parity
  added `open_responses_provider_streams_shell_container_multiturn_fixture`
  now maps upstream `openai-shell-container-multiturn.1` by proving stored
  shell-call item references, shell-output history replay, `containerAuto`
  request mapping, follow-up assistant text streaming, and final response
  metadata/usage/service-tier preservation.
- 2026-05-20: OpenAI Responses shell local multiturn generated fixture parity
  added
  `open_responses_provider_generates_shell_local_multiturn_fixture_request_body`
  and `open_responses_provider_generates_shell_local_multiturn_fixture_content`
  now map upstream `openai-shell-local-multiturn.1` by proving stored
  shell-call item references, tool-role shell-output history replay, local
  `{ "type": "shell" }` request mapping, final assistant text preservation,
  and cached/reasoning usage.
- 2026-05-20: OpenAI Responses shell local multiturn stream fixture parity
  added `open_responses_provider_streams_shell_local_multiturn_fixture` now
  maps upstream `openai-shell-local-multiturn.1` by proving stored shell-call
  item references, tool-role shell-output history replay, local `{ "type":
  "shell" }` request mapping, follow-up assistant text streaming, and final
  response metadata/usage/service-tier preservation.
- 2026-05-20: OpenAI Responses shell environment fixture parity added
  `open_responses_provider_generates_shell_environment_fixture_request_body`,
  `open_responses_provider_generates_shell_environment_fixture_content`, and
  `open_responses_provider_streams_shell_environment_fixture` now map upstream
  `openai-shell-skills.1` by proving the separate non-streaming request-body
  and content tests, `containerAuto` request mapping, provider-executed shell
  call/result mapping, OpenAI item metadata preservation for non-streaming and
  streaming shell calls, STOP-instruction assistant text preservation, and
  cached/reasoning usage propagation.
- 2026-05-20: OpenAI Responses MCP generated fixture parity added
  `open_responses_provider_generates_mcp_tool_fixture_request_body` and
  `open_responses_provider_generates_mcp_tool_fixture_content` now map
  upstream `openai-mcp-tool.1` by proving MCP request tool shaping, ignored
  `mcp_list_tools` output items, provider-executed dynamic MCP call/result
  content, tool-result item metadata, reasoning metadata preservation, final
  assistant text preservation, and cached/reasoning usage.
- 2026-05-20: OpenAI Responses MCP stream fixture parity added
  `open_responses_provider_streams_mcp_tool_fixture` now maps upstream
  `openai-mcp-tool.1` by proving MCP request tool shaping, ignored
  `mcp_list_tools` output items, provider-executed dynamic MCP call/result
  stream parts with item metadata, interleaved reasoning metadata, final
  assistant text streaming, and cached/reasoning usage propagation.
- 2026-05-20: OpenAI Responses MCP approval non-streaming fixture parity added
  `open_responses_provider_generates_mcp_approval_request_fixture_turn_1`,
  `open_responses_provider_generates_mcp_approval_denial_fixture_turn_2`,
  `open_responses_provider_generates_mcp_approval_retry_fixture_turn_3`, and
  `open_responses_provider_generates_mcp_approval_result_fixture_turn_4` now
  map upstream `openai-mcp-tool-approval.1` through
  `openai-mcp-tool-approval.4` by proving required-approval MCP request
  shaping, generated dynamic approval tool calls, approval response
  continuation input for denial/approval, retry approval ids, approved MCP
  call/result mapping, final assistant text, stop finish reason, and response
  metadata/usage preservation.
- 2026-05-20: OpenAI Responses MCP approval stream fixture parity added
  `open_responses_provider_streams_mcp_approval_request_fixture_turn_1`,
  `open_responses_provider_streams_mcp_approval_denial_fixture_turn_2`,
  `open_responses_provider_streams_mcp_approval_retry_fixture_turn_3`, and
  `open_responses_provider_streams_mcp_approval_result_fixture_turn_4` now map
  upstream `openai-mcp-tool-approval.1` through
  `openai-mcp-tool-approval.4` by proving required-approval MCP request
  shaping, generated dynamic approval tool calls, approval response
  continuation input for denial/approval, retry approval ids, approved MCP
  call/result mapping, final assistant text streaming, and response
  metadata/usage preservation.
- 2026-05-22: OpenAI Responses MCP approval fixtures hardened
  `open_responses_provider_generates_mcp_approval_request_fixture_turn_1`
  through `open_responses_provider_generates_mcp_approval_result_fixture_turn_4`
  and `open_responses_provider_streams_mcp_approval_request_fixture_turn_1`
  through `open_responses_provider_streams_mcp_approval_result_fixture_turn_4`
  now consume byte-matched local copies of upstream
  `openai-mcp-tool-approval.1` through `openai-mcp-tool-approval.4` JSON and
  chunk fixtures instead of reconstructed response bodies.
- 2026-05-20: OpenAI Responses file-search non-streaming fixture parity added
  `open_responses_provider_generates_file_search_without_results_include_fixture_request_body`,
  `open_responses_provider_generates_file_search_without_results_include_fixture_content`,
  `open_responses_provider_generates_file_search_with_results_include_fixture_request_body`,
  and
  `open_responses_provider_generates_file_search_with_results_include_fixture_content`
  now map upstream `openai-file-search-tool.1` and
  `openai-file-search-tool.2` by proving the separate request-body and content
  tests for both include modes, request tool shaping for vector store ids, max
  results, filters, and ranking options, include forwarding for
  `file_search_call.results`, provider-executed file-search call/result
  mapping, `file_id` to `fileId` conversion, file-citation source metadata,
  cached/reasoning usage, response metadata, and stop finish reason.
- 2026-05-20: OpenAI Responses apply-patch fixture parity added
  `open_responses_provider_generates_apply_patch_create_file_fixture_request_body`,
  `open_responses_provider_generates_apply_patch_create_file_fixture_content`,
  `open_responses_provider_streams_apply_patch_create_file_fixture`, and
  `open_responses_provider_streams_apply_patch_delete_file_fixture` now map
  upstream `openai-apply-patch-tool.1` JSON/SSE fixtures plus
  `openai-apply-patch-tool-delete.1` by proving the separate non-streaming
  request-body and content tests, request tool shaping, create-file and
  delete-file tool input reconstruction, OpenAI item metadata, response
  metadata, service tier, and usage preservation.
- 2026-05-20: OpenAI Responses custom provider-tool fixture parity added
  `open_responses_provider_generates_custom_tool_fixture_request_body`,
  `open_responses_provider_generates_custom_tool_fixture_content`,
  `open_responses_provider_generates_custom_tool_fixture_tool_calls_finish_reason`,
  and
  `open_responses_provider_streams_custom_tool_fixture` now map upstream
  `openai-custom-tool.1` JSON/SSE fixtures by proving custom grammar-tool
  request shaping, separate non-streaming content and finish-reason parity,
  generated and streamed SQL tool-call input, OpenAI item metadata, response
  metadata, `tool-calls` finish reason, and usage preservation.
- 2026-05-20: OpenAI Responses web-search generated fixture parity added
  `open_responses_provider_generates_web_search_fixture` now maps upstream
  non-streaming `openai-web-search-tool.1` by proving hosted web-search request
  shaping, provider-executed search/open-page/find-in-page call/result parts,
  camelCase action result names, URL citation source emission, final text
  annotation metadata, and cached/reasoning usage preservation.
- 2026-05-20: OpenAI Responses web-search streaming fixture parity added
  `open_responses_provider_streams_upstream_web_search_tool_fixture` now maps
  upstream `openai-web-search-tool.1` by proving hosted web-search request
  shaping, provider-executed search/open-page/find-in-page call/result parts,
  camelCase action result names, empty reasoning start/end parts, URL citation
  source emission, final text reconstruction, response metadata, service tier,
  and cached/reasoning usage preservation.
- 2026-05-20: OpenAI Responses hosted tool-search fixture parity added
  `open_responses_provider_generates_hosted_tool_search_fixture` and
  `open_responses_provider_streams_hosted_tool_search_fixture` now map upstream
  `openai-tool-search.1` JSON/SSE fixtures by proving provider-executed hosted
  tool-search calls, call-result id aliasing when `call_id` is null, returned
  deferred function-tool definitions, OpenAI item metadata, `tool-calls`
  finish reason, and usage preservation.
- 2026-05-22: OpenAI Responses hosted tool-search fixture parity hardened those
  two tests to consume byte-matched local copies of upstream
  `openai-tool-search.1.json` and `openai-tool-search.1.chunks.txt` instead of
  hand-built response payloads, preserving fixture-only fields and streamed
  function-call deltas from the original upstream test data.
- 2026-05-20: OpenAI Responses phase metadata fixture parity added
  `open_responses_provider_generates_phase_fixture_metadata` and
  `open_responses_provider_streams_phase_fixture_metadata` now use byte-matched
  copies of upstream `openai-phase.1` JSON/JSONL fixtures to prove generated
  text content and streamed `text-start`/`text-end` parts preserve OpenAI
  `itemId` plus `phase` metadata for both `commentary` and `final_answer`
  assistant messages, along with response metadata and usage.
- 2026-05-20: OpenAI Responses encrypted reasoning fixture parity added
  `open_responses_provider_generates_reasoning_encrypted_content_fixture` and
  `open_responses_provider_streams_reasoning_encrypted_content_fixture` now use
  byte-matched copies of upstream `openai-reasoning-encrypted-content.1`
  JSON/JSONL fixtures to prove non-streaming encrypted reasoning metadata,
  streamed reasoning start/delta/end metadata, multi-response calculator tool
  calls, final text, usage, and response metadata. The request-body check also
  keeps upstream Responses behavior by ensuring chat-only `maxCompletionTokens`
  does not leak into `/responses` bodies.
- 2026-05-20: OpenAI Responses inline reasoning generate parity added
  `open_responses_provider_generates_reasoning_with_summary_parts`,
  `open_responses_provider_generates_reasoning_with_empty_summary`,
  `open_responses_provider_generates_encrypted_reasoning_with_summary_parts`,
  `open_responses_provider_generates_encrypted_reasoning_with_empty_summary`,
  and `open_responses_provider_generates_multiple_reasoning_blocks` now map the
  upstream non-streaming inline reasoning tests one-to-one, including summary
  arrays, empty summaries, encrypted-content metadata, multiple interleaved
  reasoning/message blocks, request-body reasoning/include shaping, response
  metadata, and usage.
- 2026-05-20: OpenAI Responses inline reasoning stream parity added
  `open_responses_provider_streams_reasoning_with_summary_parts`,
  `open_responses_provider_streams_reasoning_with_empty_summary`,
  `open_responses_provider_streams_encrypted_reasoning_with_summary_parts`,
  `open_responses_provider_streams_encrypted_reasoning_with_empty_summary`,
  and `open_responses_provider_streams_multiple_reasoning_blocks` now map the
  upstream inline reasoning stream tests one-to-one, including summary-part
  boundaries, empty summaries, encrypted-content start/end metadata, multiple
  reasoning blocks, request-body reasoning/include shaping, final text, finish
  metadata, and usage.
- 2026-05-20: OpenAI Responses compaction fixture parity added
  `open_responses_provider_generates_compaction_fixture` and
  `open_responses_provider_streams_compaction_fixture` now use byte-matched
  copies of upstream `openai-compaction.1` JSON/JSONL fixtures to prove
  generated and streamed text, request-body `context_management` forwarding,
  `openai.compaction` custom content with encrypted-content metadata, response
  metadata, service tier, and cached/reasoning usage.
- 2026-05-20: OpenAI Responses streaming citation annotation parity added
  `open_responses_provider_streams_mixed_url_and_file_citations`,
  `open_responses_provider_streams_file_citations_without_optional_fields`,
  `open_responses_provider_streams_container_file_citation`, and
  `open_responses_provider_streams_file_path_citation` now map the upstream
  `mixed citation types` streaming tests by proving URL citation,
  file-citation, container-file-citation, and file-path source emission,
  raw OpenAI annotation metadata on `text-end`, `responseId: null` finish
  metadata when no `response.created` event is streamed, file-path
  `application/octet-stream` media typing, and cached/reasoning usage.
- 2026-05-20: OpenAI Responses generated citation annotation parity added
  `open_responses_provider_generates_mixed_url_and_file_citations`,
  `open_responses_provider_generates_file_citation_only`,
  `open_responses_provider_generates_file_citations_without_optional_fields`,
  `open_responses_provider_generates_container_file_citation`, and
  `open_responses_provider_generates_file_path_citation` now map the upstream
  non-streaming mixed citation tests by proving text provider metadata,
  URL/document source emission, source provider metadata casing, file-path
  `application/octet-stream` media typing, and usage preservation.
- 2026-05-20: OpenAI Responses generated computer-use tool-call parity added
  `open_responses_provider_generates_computer_use_tool_calls` now maps the
  upstream non-streaming `should handle computer use tool calls` test by proving
  provider-executed `computer_use` tool-call/result ordering, assistant text
  metadata, and usage.
- 2026-05-20: OpenAI Responses logprobs provider-metadata parity added
  `open_responses_provider_generates_logprobs_provider_metadata` and
  `open_responses_provider_streams_logprobs_provider_metadata` now map the
  upstream non-streaming and streaming logprobs tests by proving grouped
  logprob payloads in provider metadata, response id, service tier, stop finish
  reason, and usage.
- 2026-05-20: OpenAI Responses OpenAI-key and text-delta parity added
  `open_responses_provider_uses_openai_metadata_key_for_text_result` and
  `open_responses_provider_streams_text_deltas_and_openai_finish_metadata` now
  map upstream provider-metadata key tests for non-Azure providers plus the
  simple `should stream text deltas` fixture, including the provisional
  streamed item id, final text-end item id, created-response finish metadata,
  one `response.created` metadata event, and cached/reasoning usage.
- 2026-05-20: OpenAI Responses generated function tool-call parity added
  `open_responses_provider_generates_upstream_function_tool_calls`,
  `open_responses_provider_sets_tool_calls_finish_reason_for_function_calls`,
  `open_responses_provider_preserves_namespace_on_function_call_output`, and
  `open_responses_provider_omits_namespace_on_function_call_when_absent` now
  map the upstream non-streaming function-call tests one-to-one, including
  multiple tool calls, `tool-calls` finish reason, OpenAI item metadata, and
  namespace metadata only when upstream sends it.
- 2026-05-20: OpenAI Responses allowed-tools test parity split
  `open_responses_provider_maps_allowed_tools_to_tool_choice`,
  `open_responses_provider_maps_allowed_tools_required_mode`, and
  `open_responses_provider_allowed_tools_overrides_request_tool_choice` now map
  the three upstream `allowedTools` request tests one-to-one, including the
  separate request-level `toolChoice` override case.
- 2026-05-20: OpenAI Responses generated hosted-tool fixture parity added
  `open_responses_provider_generates_code_interpreter_fixture_results` and
  `open_responses_provider_generates_image_generation_fixture_results` now map
  upstream non-streaming `code_interpreter` and `image_generation` fixture tests
  one-to-one. The Rust fixtures prove request shaping, hosted tool-call/result
  mapping, container-file citation source metadata, image result payloads, empty
  assistant text metadata, and cached/reasoning usage.
- 2026-05-20: OpenAI Responses hosted-tool request-body parity added
  `open_responses_provider_sends_code_interpreter_request_body_with_include_and_tool`
  and `open_responses_provider_sends_image_generation_request_body_with_tool`
  now map the upstream non-streaming request-body tests for
  `openai.code_interpreter` and `openai.image_generation` one-to-one, including
  automatic hosted-tool includes and image generation option casing.
- 2026-05-20: OpenAI Responses local-shell and web-search request-body parity
  added `open_responses_provider_sends_local_shell_request_body_with_tool` and
  `open_responses_provider_sends_web_search_request_body_with_include_and_tool`,
  mapping the upstream request-body tests for `openai.local_shell` and
  `openai.web_search` one-to-one.
- 2026-05-20: OpenAI Responses local-shell/web-search prepare-tool test split
  added `open_responses_provider_prepares_local_shell_tool` plus seven
  `openai.web_search` Rust tests for omitted options, external web access
  true/false, full option mapping, filters without external access, hosted
  tool-choice, and mixed function plus web-search tools, mapping upstream
  `prepareResponsesTools` cases one-to-one while keeping the existing
  `web_search_preview` combined regression test.
- 2026-05-20: OpenAI Responses code-interpreter/image-generation prepare-tool
  test split added auto-container, string-container, file-id-container, empty
  file-id, omitted file-id, hosted tool-choice, mixed function plus
  code-interpreter, image-generation options, and image-generation tool-choice
  Rust tests, mapping upstream `prepareResponsesTools` cases one-to-one while
  keeping the existing combined regression test.
- 2026-05-20: OpenAI Responses shell prepare-tool test split added twelve
  explicit shell request tests alongside the existing unresolved-reference
  error test, mapping upstream `prepareResponsesTools` shell cases one-to-one
  for no environment args, `containerAuto`, skill references, inline skills,
  network policies, file ids, memory limits, `containerReference`, and local
  environments. Rust assertions compare the serialized request body, so
  TypeScript-only `undefined` fields are omitted.
- 2026-05-20: OpenAI Responses custom prepare-tool test split added
  `open_responses_provider_prepares_custom_tool_with_regex_format`,
  `open_responses_provider_prepares_custom_tool_with_lark_format`,
  `open_responses_provider_prepares_multiple_tools_including_custom_tool`, and
  `open_responses_provider_resolves_custom_tool_choice_using_tool_name`, mapping
  upstream `prepareResponsesTools` custom-tool cases one-to-one while keeping
  the existing combined regression test.
- 2026-05-20: OpenAI Responses apply-patch/tool-search prepare-tool test split
  added `open_responses_provider_prepares_apply_patch_tool`,
  `open_responses_provider_resolves_apply_patch_tool_choice`,
  `open_responses_provider_prepares_multiple_tools_including_apply_patch`,
  `open_responses_provider_prepares_tool_search_tool`, and
  `open_responses_provider_prepares_tool_search_with_deferred_function_tool`,
  mapping upstream `prepareResponsesTools` apply-patch/tool-search cases
  one-to-one while keeping the existing combined regression test.
- 2026-05-20: OpenAI Responses function-tool strict-mode prepare-tool test
  split added `open_responses_provider_passes_strict_true_function_tool`,
  `open_responses_provider_passes_strict_false_function_tool`,
  `open_responses_provider_omits_undefined_strict_function_tool`, and
  `open_responses_provider_passes_mixed_strict_function_tools`, mapping
  upstream `prepareResponsesTools` strict-mode cases one-to-one while keeping
  the existing combined regression test.
- 2026-05-22: OpenAI Responses prepare-tool empty-warning parity hardened
  the shared request-body helper for the split `prepareResponsesTools` cases
  now asserts `result.warnings` is empty, matching upstream `toolWarnings: []`
  snapshots instead of only comparing the serialized request body.
- 2026-05-20: OpenAI Responses generated client tool-search parity added
  `open_responses_provider_generates_client_tool_search_fixture`,
  `open_responses_provider_omits_provider_executed_for_client_tool_search_fixture`,
  and `open_responses_provider_uses_call_id_for_client_tool_search_fixture`
  now map the three upstream non-streaming client `tool_search` fixture tests
  one-to-one, proving client execution metadata, absent `providerExecuted`,
  `call_id`-based `toolCallId`, OpenAI item metadata, `store: false`, and
  client tool-search request shaping.
- 2026-05-20: OpenAI Responses upstream streamed tool edge parity added
  `open_responses_provider_streams_upstream_incomplete_response_finish_reason`,
  `open_responses_provider_streams_upstream_tool_calls`,
  `open_responses_provider_preserves_namespace_on_streaming_function_call_output`,
  and
  `open_responses_provider_omits_namespace_on_streaming_function_call_when_absent`
  now map the upstream incomplete response, streaming tool-call, and function
  namespace tests one-to-one. The parser now uses final `call_id` values for
  `tool-input-end`, dedupes completed response tool-call payloads by item id,
  and preserves namespace metadata only when upstream sends it.
- 2026-05-20: OpenAI Responses upstream service-tier stream parity added
  `open_responses_provider_streams_upstream_service_tier`, mapping upstream
  `Should handle service tier` with `providerOptions.openai.serviceTier=flex`,
  empty reasoning start/end metadata, text start/delta/end, response metadata,
  `serviceTier: flex` finish metadata, and cached/reasoning usage.
- 2026-05-20: OpenAI Responses hasConversation prompt parity added
  `open_responses_provider_skips_assistant_text_item_ids_when_conversation_is_set`,
  `open_responses_provider_skips_assistant_tool_call_item_ids_when_conversation_is_set`,
  `open_responses_provider_includes_fresh_assistant_text_when_conversation_is_set`,
  `open_responses_provider_uses_item_references_when_conversation_is_not_set`,
  and `open_responses_provider_skips_reasoning_item_ids_when_conversation_is_set`
  now map the upstream `convertToOpenAIResponsesInput > hasConversation`
  tests one-to-one in the package-owned Open Responses crate.
- 2026-05-20: OpenAI Responses compaction prompt parity added
  `open_responses_provider_converts_compaction_to_item_reference_when_stored`,
  `open_responses_provider_converts_compaction_to_full_item_when_unstored`,
  `open_responses_provider_skips_compaction_item_ids_when_conversation_is_set`,
  `open_responses_provider_converts_compaction_alongside_fresh_text_when_unstored`,
  and
  `open_responses_provider_converts_compaction_alongside_text_to_item_references_when_stored`
  now map the upstream `convertToOpenAIResponsesInput > compaction` tests
  one-to-one in the package-owned Open Responses crate.
- 2026-05-20: OpenAI Responses custom provider-tool prompt parity added
  `open_responses_provider_converts_custom_tool_call_to_custom_tool_call_input_item`,
  `open_responses_provider_json_stringifies_non_string_custom_tool_call_input`,
  `open_responses_provider_converts_stored_custom_tool_call_to_item_reference`,
  `open_responses_provider_converts_custom_tool_text_result_to_output`,
  `open_responses_provider_converts_custom_tool_json_result_to_output`,
  `open_responses_provider_converts_execution_denied_custom_tool_result_to_output`,
  `open_responses_provider_converts_custom_tool_content_result_to_output`,
  `open_responses_provider_converts_custom_tool_file_url_content_result_to_output`,
  and
  `open_responses_provider_falls_back_to_function_call_without_custom_provider_tool_names`
  now map upstream `convertToOpenAIResponsesInput > custom tool calls`
  one-to-one, including the non-custom fallback case where string input must
  become JSON-stringified function-call arguments.
- 2026-05-20: OpenAI Responses API schema-alignment parity added
  `open_responses_schema_alignment_matches_annotation_shape_between_stream_and_response`,
  `open_responses_schema_alignment_aligns_web_search_call_actions`,
  `open_responses_schema_alignment_aligns_code_interpreter_outputs`,
  `open_responses_schema_alignment_aligns_file_search_call_results`,
  `open_responses_schema_alignment_aligns_message_phase_between_added_done_and_response`,
  and `open_responses_schema_alignment_aligns_output_text_logprobs` in
  `crates/ai-sdk-open-responses/src/open_responses.rs`, mapping every upstream
  `openai-responses-api.test.ts` schema-alignment case to Rust parser evidence
  across non-streaming Responses bodies and SSE chunks.
- 2026-05-21: OpenAI Responses isolated provider-option parity split the Rust
  `textVerbosity` and `truncation` request-body coverage into distinct test
  cases:
  `open_responses_provider_sends_text_verbosity_low_provider_option`,
  `open_responses_provider_sends_text_verbosity_medium_provider_option`,
  `open_responses_provider_sends_text_verbosity_high_provider_option`,
  `open_responses_provider_sends_truncation_auto_provider_option`, and
  `open_responses_provider_sends_truncation_disabled_provider_option`. These
  now map one-to-one to the five separate upstream
  `openai-responses-language-model.test.ts` cases instead of collapsing them
  into grouped Rust loops.
- 2026-05-21: OpenAI Responses prompt file conversion parity split the Rust
  default-PDF filename and unsupported-file coverage into
  `open_responses_provider_uses_default_filename_for_pdf_file_parts_when_not_provided`,
  `open_responses_provider_rejects_unsupported_file_types_by_default`, and
  `open_responses_provider_passes_through_unsupported_file_types_when_enabled`.
  These now map one-to-one to the three upstream
  `convert-to-openai-responses-input.test.ts` prompt file cases instead of
  collapsing them into one provider request test.
- 2026-05-21: OpenAI Responses isolated provider-option parity added distinct
  request-body tests for `parallelToolCalls`, `user`, `conversation`,
  `previousResponseId`, the upstream metadata-titled user assertion, actual
  `metadata`, `instructions`, single `include`, and multiple `include` values.
  These now map one-to-one to the upstream
  `openai-responses-language-model.test.ts` cases that were previously only
  covered by the broader provider-option matrix.
- 2026-05-21: OpenAI Responses tool-result conversion parity split the Rust
  standard and multipart tool-result coverage into one-to-one tests for JSON,
  text, execution-denied, multiple-result, text-content, image data, image URL,
  `imageDetail` data and URL forwarding, PDF data, PDF URL, and mixed-content
  outputs. These now map one-to-one to the upstream
  `convert-to-openai-responses-input.test.ts` tool-message cases instead of
  relying only on two broader Rust regression tests.
- 2026-05-21: OpenAI Responses assistant tool-call prompt conversion parity
  split the Rust coverage into distinct tests for assistant text plus a tool
  call, missing tool-call input defaulting to `{}`, stored item IDs becoming
  `item_reference` entries, and multiple tool calls in one assistant message.
  These now map one-to-one to the upstream
  `convert-to-openai-responses-input.test.ts` assistant tool-call cases while
  keeping the broader Rust argument-stringification regression as extra
  coverage.
- 2026-05-22: OpenAI Responses omitted assistant tool-call input parity
  hardened `open_responses_provider_defaults_missing_assistant_tool_call_input_to_empty_object`
  to deserialize a prompt part with no `input` field, backed by
  `assistant_tool_call_part_deserializes_missing_input_as_null` in the provider
  crate, so the Rust case now covers the upstream `input: undefined` fixture
  directly.
- 2026-05-21: OpenAI Responses reasoning prompt-history parity split the Rust
  coverage into distinct tests for single reasoning parts, empty summaries,
  empty append warnings, same-id merging, unencrypted drop behavior, separate
  reasoning IDs, stored and unstored multi-message histories, complex
  reasoning/tool interleaving, and missing-provider-option warnings. These now
  map one-to-one to the upstream `convert-to-openai-responses-input.test.ts`
  reasoning-message cases instead of relying only on broader reconstruction and
  warning regressions.
- 2026-05-21: OpenAI Responses user prompt file conversion parity split the
  Rust coverage into distinct package-owned tests for text, image URL/data/byte
  data, image file-id prefixes, image/PDF provider references, wildcard image
  detection and rejection, OpenAI/Azure `imageDetail`, PDF data/file-id/URL
  parts, provider-reference namespace selection, plain-string base64 handling,
  and empty/multiple file-id-prefix modes. These now map one-to-one to the
  upstream `convert-to-openai-responses-input.test.ts` user-message file cases
  instead of relying on grouped root/facade regressions.
- 2026-05-22: Open Responses generic top-level image media-type parity split
  upstream
  `packages/open-responses/src/responses/convert-to-open-responses-input.test.ts`
  top-level-only media type resolution into named Rust counterparts:
  `open_responses_provider_passes_full_image_png_through_unchanged_for_inline_data`,
  `open_responses_provider_detects_image_subtype_from_inline_bytes_for_top_level_image`,
  `open_responses_provider_passes_through_url_source_for_top_level_only_image`,
  and `open_responses_provider_normalizes_image_wildcard_via_detection`.
  These cover the generic Open Responses route instead of relying on the older
  grouped OpenAI-wrapper prompt-file regression.
- 2026-05-21: OpenAI Responses system and assistant text prompt parity split
  the Rust coverage into distinct package-owned tests for `system`,
  `developer`, and `remove` system-message modes plus assistant `output_text`
  conversion, `commentary` phase, `final_answer` phase, and omitted phase.
  These now map one-to-one to the upstream
  `convert-to-openai-responses-input.test.ts` system-message and assistant
  text/phase cases instead of relying only on broader Rust regressions.
- 2026-05-21: Open Responses finish-reason mapping parity split upstream
  `map-open-responses-finish-reason.test.ts` into one Rust test per portable
  original case: `undefined` and `null` with and without tool calls,
  `max_output_tokens`, `content_filter`, and unknown `completed` with and
  without tool calls. Rust keeps separate named tests for upstream
  `undefined` and `null` even though both map to `None`, and retains the
  legacy `max_tokens` alias only as additive Rust coverage.
- 2026-05-21: Open Responses generic reasoning request parity split upstream
  `open-responses-language-model.test.ts` reasoning request-option cases into
  one Rust test per original case: top-level `high`, `minimal`, `none`,
  `xhigh`, omitted reasoning, provider-option `detailed`, combined `high` plus
  `auto`, provider-option `concise`, and empty provider options. The generic
  provider option filtering assertion remains as additive Rust coverage.
- 2026-05-21: Open Responses generic tool-choice request parity split upstream
  `open-responses-language-model.test.ts` `doGenerate > tool choice` cases into
  one Rust test per original case: `auto`, `none`, `required`, and specific
  function-tool selection. The old grouped Rust regression was replaced so the
  ledger maps the original TypeScript cases by name instead of by feature.
- 2026-05-21: Open Responses generic system-message request parity split upstream
  `open-responses-language-model.test.ts` `doGenerate > system messages` cases
  into dedicated Rust tests for one system instruction and multiple system
  instructions joined with newlines. The old broader Rust regression was
  replaced with original-case names in the package-owned crate.
- 2026-05-22: Open Responses generic input system-message parity split upstream
  `packages/open-responses/src/responses/convert-to-open-responses-input.test.ts`
  system-message conversion into exact named Rust counterparts:
  `open_responses_provider_converts_single_system_message_to_instructions`,
  `open_responses_provider_joins_system_messages_with_newlines_for_input_conversion`,
  `open_responses_provider_returns_no_instructions_without_system_messages`,
  and `open_responses_provider_handles_system_message_with_user_and_assistant_messages`.
  These assert the generic provider's `instructions` field and non-system
  input message array directly.
- 2026-05-22: Open Responses generic provider-reference rejection parity added
  `open_responses_provider_rejects_file_parts_with_provider_references` for
  upstream
  `packages/open-responses/src/responses/convert-to-open-responses-input.test.ts`
  `provider reference > should throw for file parts with provider references`.
  Generic Open Responses providers now reject provider-reference file parts with
  the upstream unsupported-functionality message while OpenAI/Azure/Gateway
  wrapper routes continue resolving supported prompt references.
- 2026-05-21: Open Responses LMStudio tool-call parsing parity split upstream
  `open-responses-language-model.test.ts` `doGenerate > tool call parsing`
  cases into dedicated Rust tests for parsed tool-call content, `tool-calls`
  finish reason, and usage extraction instead of one grouped fixture
  regression.
- 2026-05-21: Open Responses LMStudio basic generation parity split upstream
  `open-responses-language-model.test.ts` `doGenerate > basic generation`
  cases into dedicated Rust tests for request body, content, and usage. The
  fixture call now uses the upstream `gemma-7b-it` model id and `Hello` prompt
  for the request-body counterpart.
- 2026-05-21: Open Responses LMStudio basic streaming parity added the upstream
  `open-responses-language-model.test.ts` `doStream > basic generation >
  should stream content` case as `open_responses_provider_streams_lmstudio_basic_content`
  with the original `lmstudio-basic.1` chunk fixture copied into the
  package-owned Open Responses crate.
- 2026-05-21: Open Responses LMStudio request-parameter/tool request parity
  split upstream `open-responses-language-model.test.ts` `doGenerate >
  request parameters` and `doGenerate > tools` into exact Rust request-body
  counterparts. The request-parameter test now uses LMStudio's upstream model
  and preserves presence/frequency penalties, while the tools test asserts the
  two-function-tool schema and strict flag shape.
- 2026-05-22: Provider-utils upstream test inventory audit made the remaining
  schema/type gaps explicit. The ledger now records the 25-file, 162-case Zod
  v3 `zod3-to-json-schema` adapter suite plus Zod v4 schema snapshots as
  JavaScript/Zod-runtime-specific, and tracks all 13 upstream provider-utils
  `*.test-d.ts` files as a 92-case type-level inventory that still needs named
  Rust compile/type/API counterparts or explicit TypeScript-only
  justifications before the package can be verified.
- 2026-05-22: Provider-utils tool function type parity added named Rust
  counterparts for all 3 upstream `types/tool-execute-function.test-d.ts` cases
  and the single `types/tool-needs-approval-function.test-d.ts` case. Rust now
  carries provider abort signals through tool execution options and sandbox
  command options without serializing them, and tests execute/approval callback
  input, output, options, and boolean result contracts.
- 2026-05-22: OpenAI-compatible prepare-tools parity split upstream
  `openai-compatible-prepare-tools.test.ts` into one Rust test per portable
  case in `crates/ai-sdk-openai-compatible`: null and empty tool lists,
  function-tool serialization, unsupported provider-defined tool warnings,
  `auto`/`required`/`none`/specific tool choice serialization, and strict
  true/false/omitted/mixed function-tool settings.
- 2026-05-22: WorkflowAgent parity started in the package-owned
  `crates/ai-sdk-workflow` crate with a deterministic agent loop over the
  existing stream-text iterator. Named Rust counterparts now cover upstream
  optional `id`, successful local tool execution, tool execution errors as
  `error-text`, provider-executed tool result/error/missing-result handling,
  client-side tools without executors stopping the loop, accumulated messages
  passed to tool executors, per-tool context delivery, and Rust-side context
  validator failures. Real model-backed execution, HTTP/SSE workflow
  transports, integration workflows, and full
  Ajv-equivalent arbitrary JSON Schema validation remain open.
- 2026-05-22: WorkflowAgent mixed tool execution parity added named Rust
  counterparts for upstream mixed provider-executed/local tool rounds, mixed
  executable/client-side tool rounds, and invalid tool-call error-path handling
  without local execution.
- 2026-05-22: OpenAI-compatible includeUsage provider-setting parity added
  named Rust counterparts for the three upstream
  `openai-compatible-provider.test.ts` includeUsage cases. The Rust tests
  now verify `true`, `false`, and unspecified provider settings both as direct
  chat, `language_model`, and completion model configuration, and through
  stream request bodies that match upstream's `stream_options.include_usage`
  emission only for `true`.
- 2026-05-22: OpenAI-compatible provider factory/configuration parity added
  named Rust counterparts for upstream `openai-compatible-provider.test.ts`
  provider construction, missing-authorization, chat/completion/embedding
  model creation, default language-model alias, queryless URL, and
  `supportsStructuredOutputs` routing cases.
- 2026-05-22: OpenAI-compatible metadata extractor parity added Rust
  `OpenAICompatibleMetadataExtractor` and `OpenAICompatibleStreamMetadataExtractor`
  callback surfaces plus named Rust counterparts for the upstream
  `metadataExtractor` provider-setting case and the two upstream chat-model
  complete/streaming metadata processing cases.
- 2026-05-22: OpenAI-compatible chat request body transformation parity added
  `OpenAICompatibleRequestBodyTransformer` plus named Rust counterparts for
  all three upstream `transformRequestBody` generate, stream, and absent-setting
  cases.
- 2026-05-22: OpenAI-compatible chat config parity added named Rust
  counterparts for the three upstream `providerOptionsName` extraction cases
  and moved provider-options name extraction to a shared helper.
- 2026-05-22: OpenAI-compatible non-stream chat `doGenerate` basic parity
  added named Rust counterparts for text/tool-call extraction, usage, response
  metadata and headers, request model/messages, settings, provider-option
  filtering, headers, unknown finish reasons, and reasoning-content precedence.
- 2026-05-22: OpenAI-compatible chat response-format and GPT-5 request option
  parity added named Rust counterparts for all upstream non-stream
  `doGenerate > response format` cases: text/no body `response_format`,
  JSON object, structured-output schema omission/warnings, JSON schema
  strict/name/description handling, `strictJsonSchema: false`,
  undefined-schema JSON, provider `reasoningEffort`, top-level `reasoning`,
  provider-over-top-level precedence, provider `textVerbosity`, and
  no-duplicate provider option request bodies.
- 2026-05-22: OpenAI-compatible chat request-body/raw-chunk parity added
  named Rust counterparts for the upstream non-stream returned request body
  snapshot and the `includeRawChunks: true` raw chunk stream ordering case.
- 2026-05-22: OpenAI-compatible streaming chat `doStream` basic parity added
  named Rust counterparts for text streaming, raw response headers,
  provider-level `includeUsage`, reasoning streaming and precedence, error
  and unparsable stream parts, request model/messages, full stream request
  body, merged headers, and provider-option filtering.
- 2026-05-22: OpenAI-compatible streaming chat fixture parity copied the
  upstream `xai-text.chunks.txt` and `xai-tool-call.chunks.txt` fixtures into
  the owning crate and added named Rust counterparts for both upstream snapshot
  cases, including full fixture line counts, reasoning delta counts, text/tool
  output, metadata, finish reason, and usage.
- 2026-05-22: OpenAI-compatible streaming chat tool-call delta parity added
  named Rust counterparts for upstream normal, late-name, missing-name error,
  thought-signature, parallel-call, first-chunk-arguments,
  duplicate-empty-chunk, one-chunk, and empty one-chunk tool-call stream cases.
- 2026-05-22: OpenAI-compatible non-stream chat usage-detail parity added named
  Rust counterparts for detailed, missing, partial, and provider-specific raw
  usage cases, and retained empty provider metadata for missing token details.
- 2026-05-22: OpenAI-compatible streaming chat usage-detail parity added named
  Rust counterparts for the upstream `doStream > usage details in streaming`
  cases: detailed finish usage with prediction metadata, missing token details,
  and partial token details.
- 2026-05-22: OpenAI-compatible chat provider-option metadata key parity added
  named Rust counterparts for upstream `openai-compatible-chat-language-model.test.ts`
  non-stream `doGenerate` camelCase/raw provider-option cases: camelCase key
  acceptance for hyphenated providers, camel-over-raw precedence, raw-key
  warnings, raw/camel/no-option provider metadata keys, and thought-signature
  metadata under the selected key.
- 2026-05-22: OpenAI-compatible non-stream chat thought-signature parity added
  named Rust counterparts for the upstream Google Gemini `extra_content`
  response cases: signed tool calls expose `thoughtSignature` provider metadata
  under the default raw provider key, parallel unsigned calls omit metadata, and
  completely unsigned tool calls have no provider metadata.
- 2026-05-22: OpenAI-compatible chat streaming provider-option metadata key
  parity added named Rust counterparts for upstream `doStream` camelCase/raw
  provider-option cases: stream request option merging, camel-over-raw
  precedence, stream-start raw-key warnings, finish metadata key selection for
  camel/raw/no options, and streamed tool-call thought signatures under the
  selected camelCase key.
- 2026-05-22: OpenAI-compatible chat prompt conversion parity added named Rust
  counterparts for every portable upstream
  `convert-to-openai-compatible-chat-messages.test.ts` case, covering user
  text/media/file conversion, error cases, tool-call and tool-result prompt
  mapping, metadata merging/precedence, Google thought-signature
  `extra_content`, and top-level-only image media-type detection.
- 2026-05-22: OpenAI-compatible embedding parity added named Rust counterparts
  for every portable upstream `openai-compatible-embedding-model.test.ts`
  case and marked the embeddings row verified.
- 2026-05-22: OpenAI-compatible completion non-stream parity added named Rust
  counterparts for upstream `openai-compatible-completion-language-model.test.ts`
  `config` and `doGenerate` cases, split completion usage conversion from chat
  usage conversion, and left `doStream` cases for the next completion slice.
- 2026-05-22: OpenAI-compatible completion streaming parity added named Rust
  counterparts for every portable upstream `doStream` case in
  `openai-compatible-completion-language-model.test.ts`, aligned streamed
  provider-error finish metadata with upstream, and marked the completion row
  verified.
- 2026-05-22: OpenAI-compatible image parity added named Rust counterparts
  for every portable upstream `openai-compatible-image-model.test.ts` case:
  constructor metadata, generation request body, provider-option key selection
  and hyphenated warning behavior, unsupported settings, headers, custom and
  default API error structures, raw base64 image data, response metadata, real
  timestamp fallback, `user` pass-through and omission, and image-edit requests
  with files, masks, byte data, multiple images, and edit response metadata.
  The image row is now verified.
- 2026-05-22: OpenAI-compatible package parity verified the top-level
  `packages/openai-compatible` row after re-enumerating all 8 current upstream
  test files and 230 portable `it` cases. The detailed rows now map every
  package test file to named Rust counterparts in the matching
  `ai-sdk-openai-compatible` crate, with additional root/Gateway integration
  and ignored live Gateway coverage counted only as additive proof.
- 2026-05-22: Provider-utils TypeScript-only conditional helper inventory
  documented all 4 `has-required-key.test-d.ts` cases and all 3
  `types/never-optional.test-d.ts` cases as non-portable TypeScript compiler
  mechanics. These no longer remain ambiguous hidden Rust test debt, but the
  broader provider-utils `.test-d.ts` inventory stays in-progress until the
  remaining portable type/API assertions are mapped.
- 2026-05-22: Provider-utils content-part type parity added package-owned
  Rust `FilePart`, `ReasoningFilePart`, `ToolResultOutput`, and
  `ToolResultContentPart` contracts plus named Rust counterparts for every
  upstream `types/content-part.test-d.ts` case: file data tagged/shorthand
  arms, reasoning-file tagged/shorthand arms, tool-result tagged-only file
  content, legacy file/image result variants, and top-level
  `ProviderReference` reserved-key rejection.
- 2026-05-22: Provider-utils generic tool inference inventory documented the
  20 upstream TypeScript-only `types/executable-tool.test-d.ts`,
  `types/execute-tool.test-d.ts`, `types/infer-tool-context.test-d.ts`,
  `types/infer-tool-input.test-d.ts`, `types/infer-tool-output.test-d.ts`,
  and `types/infer-tool-set-context.test-d.ts` cases as TypeScript compiler
  inference/narrowing behavior. The ledger now points those rows to existing
  Rust runtime counterparts for executable-tool detection, streamed/final tool
  outputs, context schema retention, and execute/approval callback options.
- 2026-05-22: Tool input lifecycle callbacks added package-owned
  provider-utils callback option contracts plus high-level `generate_text` and
  `stream_text` invocation. Rust now has deterministic counterparts for
  upstream `invoke-tool-callbacks-from-stream.test.ts`, the non-streaming
  `generate-text.test.ts` `onInputAvailable` case, the streaming
  `stream-text.test.ts` `onInputStart`/`onInputDelta`/`onInputAvailable`
  sequence, and the portable runtime callback surface behind
  `types/tool.test-d.ts` input lifecycle callback typing.
- 2026-05-22: Provider-utils `types/tool.test-d.ts` callback-option parity
  added six named Rust tests for upstream `toModelOutput` and function-form
  `needsApproval` cases. Rust now proves model-output callbacks receive tool
  call id, input, and output for input-only, execute-backed, and output-schema
  tools, and approval callbacks receive input/options/context for input-only,
  execute-backed, and context-schema tools.
- 2026-05-22: Provider-utils `types/tool.test-d.ts` tool-variant parity added
  eighteen named Rust tests for upstream DynamicTool, ProviderDefinedTool,
  ProviderExecutedTool, FunctionTool, and Tool discriminated-union cases. Rust
  now proves function/dynamic/provider variant identity, provider id/args and
  execution flags, dynamic function-style properties, provider-only property
  exclusion, deferred-result support only for provider-executed tools, and
  Rust predicate-based narrowing across all tool variants.
- 2026-05-22: Provider-utils `types/tool.test-d.ts` tool-constructor parity added
  eight named Rust tests for upstream input type, context type, and output type
  helper cases. Rust now proves helper-created tools retain input schemas and
  examples, `FlexibleSchema` validation survives normalization, context schema
  data reaches execute and lifecycle callbacks, and execute plus streamed
  execute helpers expose final/preliminary outputs. Remaining exact Zod literal
  inference and `undefined`/`any`/`AsyncGenerator` generic signatures are
  tracked as TypeScript compiler-only inventory.
- 2026-05-21: Open Responses PDF input-file parity split upstream
  `open-responses-language-model.test.ts` `doGenerate > pdf input file` into
  dedicated Rust tests for request body, content, and usage extraction. The
  streamed PDF fixture remains a separate counterpart for upstream `doStream >
  pdf input file`.
- 2026-05-21: Provider `getErrorMessage` parity split the previous grouped
  Rust regression into one test per original upstream
  `packages/provider/src/errors/get-error-message.test.ts` case. The Rust
  package now has explicit counterparts for null/undefined, string and empty
  string passthrough, named errors, custom error/toString output, and compact
  JSON serialization of object, number, boolean, and array values.
- 2026-05-21: Provider-utils nullish filtering parity split `filterNullable`
  and `removeUndefinedEntries` into exact Rust counterparts for every upstream
  `filter-nullable.test.ts` and `remove-undefined-entries.test.ts` case,
  including empty records, all-undefined records, null removal, and preservation
  of falsy present values.
- 2026-05-21: Provider-utils ID generation parity split upstream
  `generate-id.test.ts` into one Rust test per portable configured-length,
  default-length, invalid-separator, and unique-ID case. Existing grouped Rust
  ID generator tests remain additive only.
- 2026-05-21: Provider-utils delayed promise parity split upstream
  `delayed-promise.test.ts` into one Rust test per portable resolve/reject
  before-access, after-access, repeated-access, blocking, and all-pending
  futures case. Existing grouped Rust delayed-promise tests remain additive
  only.
- 2026-05-21: Provider-utils JSON instruction parity added Rust counterparts
  for upstream `inject-json-instruction.test.ts` no-prompt/no-schema handling,
  empty message-array insertion, and all non-system-message preservation while
  keeping the helper row in progress for the remaining schema edge cases.
- 2026-05-21: Provider-utils JSON instruction parity completed the portable
  upstream `inject-json-instruction.test.ts` matrix with one-to-one Rust tests
  for prompt/schema/default suffix variants, empty and complex schemas, special
  characters, long schemas, message insertion/update behavior, no-mutation
  proof, empty and no-system message arrays, no-schema message injection, and
  custom message schema lines. The explicit `null as any` optional-parameter
  fixture is documented as JavaScript-only because Rust `Option` does not
  expose a distinct explicit-null state for these typed arguments.
- 2026-05-21: Provider-utils media type extension parity split upstream
  `media-type-to-extension.test.ts` into one Rust test per `it.each` row for
  common audio types, uppercase input, and invalid media type handling.
- 2026-05-21: Provider-utils array and filename helper parity named existing
  Rust tests as explicit counterparts for every upstream `as-array.test.ts`
  and `strip-file-extension.test.ts` portable case.
- 2026-05-21: Provider-utils JSON Schema additional-properties parity split
  upstream `add-additional-properties-to-json-schema.test.ts` into one Rust
  test per portable case, with a tuple-items Rust regression retained as extra
  coverage only.
- 2026-05-21: Provider-utils media detection parity split upstream
  `detect-media-type.test.ts` into one Rust test per portable media signature,
  helper, top-level detection, no-top-level detection, and negative/error case.
  The same slice completed the missing portable `resolve-full-media-type.test.ts`
  unsupported top-level and base64 string data cases.
- 2026-05-21: Provider-utils form-data and image file data-URI parity split
  upstream `convert-to-form-data.test.ts` and
  `convert-image-model-file-to-data-uri.test.ts` into one Rust test per
  portable form field, array, nullish, typed-input, URL, base64, byte, and
  media-type variation case.
- 2026-05-21: Provider-utils header normalization parity split upstream
  `normalize-headers.test.ts` into one Rust test per portable case for missing
  input, header-pair conversion, tuple arrays, plain records with nullish
  filtering, empty headers, and uppercase key normalization.
- 2026-05-21: Provider-utils reasoning provider mapping parity split upstream
  `map-reasoning-to-provider.test.ts` into one Rust test per portable effort,
  custom-reasoning, and budget-mapping case, with grouped Rust regressions
  retained only as additional coverage.
- 2026-05-21: Provider-utils provider-reference detection parity split
  upstream `is-provider-reference.test.ts` into one Rust test per portable
  record, tagged object, array, null, string, and number case. The upstream
  `URL` instance case is documented as JavaScript-object-prototype-specific.
- 2026-05-21: Provider-utils provider-reference resolution parity split
  upstream `resolve-provider-reference.test.ts` into one Rust test per
  provider lookup, alternate-provider lookup, missing-provider error,
  empty-reference error, and single-provider reference case.
- 2026-05-21: Provider-utils resolve parity split upstream
  `resolve.test.ts` into one Rust test per portable raw value/object,
  promise/future, rejected-result, sync/async function, null/undefined,
  nested-object, header, repeated async header producer, and type-preservation
  case. Existing grouped Rust resolve tests remain additive only.
- 2026-05-21: Provider-utils tool-name mapping parity split upstream
  `create-tool-name-mapping.test.ts` into one Rust test per portable
  provider-defined mapping, function-tool passthrough, missing provider-name
  entry, missing lookup entry, empty tool list, and mixed tool-list case.
  Existing grouped Rust mapping tests remain additive only.
- 2026-05-21: Provider-utils user-agent suffix parity split upstream
  `with-user-agent-suffix.test.ts` into one Rust test per portable new
  user-agent, existing user-agent append, missing-header filtering, browser
  `Headers`-style iterable, and array-header entry case. Existing grouped Rust
  user-agent tests remain additive only.
- 2026-05-24: OpenAI image model parity added a dedicated `OpenAIImageModel`
  for `/images/generations` and `/images/edits`, with named Rust counterparts
  for all 27 portable upstream `openai-image-model.test.ts` generation/edit
  cases covering request bodies, provider-option snake casing, headers,
  warnings, max image limits, response-format defaults, response metadata,
  provider metadata, usage mapping, token-detail distribution, and multipart
  edit inputs.
- 2026-05-24: `generateObject` callback parity split the upstream
  `generate-object.test.ts` callback corpus into named Rust counterparts for
  onStart ordering and event payloads, deprecated telemetry alias payload
  isolation, onStepStart ordering/model metadata, onStepFinish raw object text,
  usage and reasoning, onFinish parsed object/provider metadata/reasoning,
  callback ordering, call-id correlation, and callback panic isolation.
- 2026-05-24: `generateObject`/`streamObject` timeout type-level parity added
  named Rust counterparts for the upstream `.test-d.ts` unsupported `timeout`
  option assertions:
  `generate_object_type_counterpart_does_not_accept_timeout_option` and
  `stream_object_type_counterpart_does_not_accept_timeout_option`. Rust proves
  this at the typed options boundary: neither API exposes a timeout field, and
  no timeout-derived abort signal is forwarded to the model call.
- 2026-05-24: `generateObject` result type-level parity added a typed
  `GenerateObjectResult<JsonValue>::object_as` accessor and named Rust
  counterparts for the remaining portable upstream `generate-object.test-d.ts`
  result assertions: `generate_object_type_counterpart_supports_enum_types`,
  `generate_object_type_counterpart_supports_schema_types`,
  `generate_object_type_counterpart_supports_no_schema_output_mode`, and
  `generate_object_type_counterpart_supports_array_output_mode`.

## Next Unported Work Queue

1. Finish ALL common/core SDK packages together with Vercel AI Gateway coverage
   before returning to unrelated standalone providers. This ordering is a hard
   gate, not a preference: every next eligible slice must come from this
   first-phase queue until it is closed. The first phase covers
   `packages/ai`, `packages/provider`, `packages/provider-utils`,
   `packages/openai-compatible`, `packages/open-responses`, `packages/gateway`,
   the Vercel AI Gateway OpenAI-compatible and Open Responses routes, and the
   portable non-provider package rows such as MCP, OTel, Workflow, telemetry,
   UI transport, chat state management, and test-server support. Vercel AI
   Gateway belongs to this first phase and must not be deferred with the other
   standalone providers.
   Standalone provider slices are blocked while any of these rows are not yet
   verified or explicitly documented as intentionally non-portable.
2. Treat the original upstream TypeScript tests as the non-negotiable floor for
   every slice. Each future iteration must start from the exact original
   package test list and ensure EVERY portable `it`/`test` case, table row,
   fixture/snapshot-equivalent case, streaming/error/provider-option case, and
   portable type-level assertion exists as Rust in the matching crate.
   The Rust crate may contain potentially more tests than the original
   TypeScript package, but never fewer mapped portable counterparts; EVERY
   original portable TypeScript test must exist in Rust before the package can
   be treated as parity-complete.
   Rust-specific tests can be added on top, but they are additive only; they
   never replace, collapse, or hide a missing upstream test. A slice with even
   one fewer portable original TypeScript test/case than upstream is incomplete,
   even if broader Rust tests appear to cover the same behavior.
   Completion notes must include or reference the named mapping from each
   original portable TypeScript test/case to a Rust counterpart in the matching
   crate. Total Rust test count is insufficient evidence on its own, because
   Rust-only tests are extra coverage and cannot compensate for a missing
   original upstream case.
   Explicit acceptance rule: EVERY original portable TypeScript test exists in
   Rust, potentially more Rust tests on top, but no less. A crate is incomplete
   until the complete original portable TypeScript test inventory exists in
   Rust, with any extra Rust tests counted only as additive coverage.
   Required inventory shape: `original portable TypeScript tests <= mapped Rust
   tests`. Rust may be a strict superset, but never a subset, sample, or
   behavior-only replacement for the upstream package's original tests.
3. Treat real-provider validation as part of parity evidence, not a later QA
   task. Each provider-backed row needs deterministic tests and an ignored
   credential-gated live test or runnable example before it can move to
   `verified`; if live credentials are unavailable, keep the row `in-progress`
   and document the missing proof.
   For OTel/telemetry rows, run `scripts/check-otel-loopback.sh` or equivalent
   local OTLP/HTTP receiver or collector proof of the emitted wire payload
   before `verified`; `packages/otel` must prove both the dependency-free
   exporter shape and the real Rust `opentelemetry` SDK/exporter path. Once root
   telemetry wiring is available, live provider tests should also assert that
   telemetry export.
4. Do not spend the next first-phase slice on the current
   `packages/openai/src/responses` corpus unless upstream changes or a
   regression appears. The 2026-05-22 audit refreshed upstream with
   `npx opensrc@latest path github:vercel/ai`, counted 322 explicit current
   `it`/`test` cases across the four OpenAI Responses files plus the four
   reasoning/provider-option `it.each` matrices, and confirmed the
   package-owned `ai-sdk-open-responses` crate's 523 tests include named Rust
   counterparts for every portable current Responses case. Continue
   `packages/openai` from remaining non-Responses surfaces such as chat,
   provider factory/capability matrices, and
   non-Responses error mappings.
   The upstream `openai-error.test.ts` non-Responses schema case now has the
   named Rust counterpart
   `openai_error_data_schema_should_parse_openrouter_resource_exhausted_error`.
   The upstream `openai-provider.test.ts` base URL precedence cases now have
   named Rust counterparts under `openai_provider_*base_url*`.
   The portable upstream `embedding/openai-embedding-model.test.ts` cases now
   have named Rust counterparts under `openai_embedding_should_*`.
   The portable upstream `files/openai-files.test.ts` upload cases now have
   named Rust counterparts under `openai_files_should_*`.
   The portable upstream `skills/openai-skills.test.ts` upload cases now have
   named Rust counterparts under `openai_skills_should_*`.
   The portable upstream `speech/openai-speech-model.test.ts` generation cases
   now have named Rust counterparts under `openai_speech_should_*`.
   The portable upstream `transcription/openai-transcription-model.test.ts`
   generation cases now have named Rust counterparts under
   `openai_transcription_should_*`.
   The portable upstream `image/openai-image-model.test.ts` generation/edit
   cases now have named Rust counterparts under `openai_image_should_*`.
5. Keep the next slices Gateway-first within the first-phase queue: close
   the whole common/core plus Vercel AI Gateway first-phase queue before
   expanding to unrelated providers. Within that first-phase set, finish open
   packages in this order unless upstream drift or a regression forces a
   narrower repair first: `packages/ai` to 100%, then
   `packages/provider-utils`, then `packages/provider`, then the remaining
   first-phase rows (`packages/open-responses`, `packages/gateway`, Vercel AI
   Gateway routes, MCP, OTel, Workflow, telemetry, UI transport, chat state
   management, and test-server support). The OpenAI-compatible package row is
   now verified against all current upstream package tests; do not spend
   another first-phase slice there unless upstream changes or a regression
   appears.
6. Continue `packages/mcp` inside `crates/ai-sdk-mcp` only where live protected
   service validation is possible with suitable credentials, or where upstream
   adds new portable MCP surfaces. HTTP transport bearer-token injection and
   401 refresh retry are covered for inbound SSE GET and JSON-RPC POST,
   standalone SSE auth-provider retry is covered for connection and endpoint
   POST, `McpTransportConfig`/`create_mcp_transport` now cover the hosted-auth
   config shape for both HTTP and SSE, and `hosted_oauth_http` covers the
   end-to-end local hosted OAuth example flow. The remaining known MCP auth gap
   is credential-gated protected live service validation.
7. Continue `packages/ai` utility/core parity from the upstream corpus.
   The upstream `util/merge-abort-signals.test.ts` and
   `util/set-abort-timeout.test.ts` cases now have named Rust counterparts in
   the `merge_abort_signals_*` and `set_abort_timeout_*` tests. Rust keeps the
   upstream behavior of nullish-source filtering, single-signal identity,
   first-reason propagation, numeric timeout sources, timer cancellation, and
   timeout no-ops for missing inputs, with the documented Rust differences of
   JSON timeout reasons and cancellable background timer handles.
   The upstream `util/merge-callbacks.test.ts` and `util/notify.test.ts` cases
   now have named Rust counterparts in `merge_callbacks_should_*` and
   `notify_should_*`, including together-started callback settlement, ignored
   callback errors, missing callbacks, async callback waiting, typed event
   preservation, and repeated callback reuse. The upstream
   `util/serial-job-executor.test.ts` cases now have named Rust counterparts in
   `serial_job_executor_should_*`, including single-job success, serial
   ordering, job errors, one-at-a-time execution, mixed failure continuation,
   and queued run calls preserving submission order. The upstream
   `util/prepare-retries.test.ts` default retry-count case now has the named
   Rust counterpart
   `prepare_retries_should_set_default_values_correctly_when_no_input_is_provided`.
   The upstream `util/simulate-readable-stream.test.ts` cases now have named
   Rust counterparts in `simulate_readable_stream_should_*`, including chunk
   collection, empty-stream completion, generic value preservation, injected
   initial/chunk delay recording, and `null` delay handling.
   The upstream `util/write-to-server-response.test.ts` cases now have named
   Rust counterparts in `write_to_server_response_should_*`, including byte
   chunk writes, status/status-text/header shaping, response finalization, and
   a writer-trait drain boundary for backpressure.
   The upstream `util/async-iterable-stream.test.ts` cases now have named Rust
   counterparts in `create_async_iterable_stream_should_*`, including chunk
   iteration, readable-stream collection, early-exit and thrown-error
   cancellation, natural completion without cancellation, exhausted iteration
   after break, source error propagation, active cancellation errors,
   already-cancelled empty iteration, and `return()` after completion.
   The upstream `util/create-stitchable-stream.test.ts` cases now have named
   Rust counterparts in `create_stitchable_stream_should_*`, including
   immediate close, one/two/three inner streams, empty inner streams,
   read-before-add behavior, pending reads resolving in order, inner stream
   errors, outer cancellation, add-after-close errors, termination
   cancellation, and add-after-terminate errors.
   The upstream `util/download/download.test.ts` cases now have named Rust
   counterparts in `download_should_*`, including initial and redirected SSRF
   rejection, safe redirects, successful bytes/media-type downloads with
   prepared headers, inline data URLs, non-OK and transport errors, default
   size-limit rejection, and abort-signal propagation to the injected transport
   boundary.
   The upstream `prepare-language-model-call-options.test.ts` timeout helper
   cases now have named Rust counterparts in `get_tool_timeout_ms_should_*`,
   `get_total_timeout_ms_should_*`, `get_step_timeout_ms_should_*`, and
   `get_chunk_timeout_ms_should_*`, including undefined timeouts, numeric
   timeout handling, missing detailed fields, and detailed field extraction.
   The same upstream file's `prepareLanguageModelCallOptions` cases now have
   named Rust counterparts in `prepare_language_model_call_options_should_*`,
   including valid and optional values, `maxOutputTokens >= 1` runtime
   validation, reasoning passthrough, limited returned values, and
   serde/type-boundary rejection for JavaScript-only dynamic type errors.
   The upstream `prompt/standardize-prompt.test.ts` cases now have named Rust
   counterparts in `standardize_prompt_should_*`, covering system-message
   rejection for both `messages` and prompt-array inputs, allow-system flags
   for both inputs, empty-message rejection, system-message and system-message
   array instructions, `system` alias fallback, `instructions` precedence, and
   typed-boundary rejection for JavaScript-only malformed system-message
   content parts.
   The upstream `prompt/convert-to-language-model-prompt.validation.test.ts`
   cases now have named Rust counterparts in
   `convert_to_language_model_prompt_validation_should_*`, covering
   provider-executed deferred tool calls, approval responses satisfying missing
   local result validation, provider-executed approval response preservation,
   and `MissingToolResultsError` for unresolved local tool calls.
   The upstream `prompt/prepare-tool-choice.test.ts` cases now have named Rust
   counterparts in `prepare_tool_choice_*`, covering missing/default `auto`,
   `none`, specific tool object with `toolName`, explicit `auto`, and
   `required`.
   The upstream `prompt/create-tool-model-output.test.ts` cases now have named
   Rust counterparts in `create_tool_model_output_should_*`, covering text and
   JSON error modes, custom `toModelOutput`, content output, string and JSON
   fallback output, `undefined`-as-null boundaries, and tool-call/input
   argument forwarding.
   The upstream `generate-text/filter-active-tools.test.ts` cases now have
   named Rust counterparts in `filter_active_tools_should_*`, including missing
   tools, missing active tool list, empty active tool list, and filtering with
   provider-defined tool preservation.
   The upstream `generate-text/collect-tool-approvals.test.ts` cases now have
   named Rust counterparts in `collect_tool_approvals_should_*`, including no
   approvals for a non-tool last message, ignoring unanswered approval
   requests, approved responses, processed approved responses with tool
   results, and denied responses with reasons. The upstream
   `generate-text/validate-tool-context.test.ts` cases now have named Rust
   counterparts in `validate_tool_context_*`, including no-schema passthrough,
   validated context return values, and `TypeValidationError` context metadata
   for invalid tool context.
   The upstream `generate-text/resolve-tool-approval.test.ts` context-schema
   cases now have named Rust counterparts in
   `resolve_tool_approval_passes_validated_context_to_user_defined_approval_callback`,
   `resolve_tool_approval_validates_context_before_user_defined_approval_callback`,
   and
   `resolve_tool_approval_validates_context_before_tool_defined_approval_callback`,
   proving validated tool context is passed to approval callbacks and invalid
   context fails before invoking user-defined or tool-defined approval
   callbacks.
   The same upstream file's callback precedence and normalization cases now
   have named Rust counterparts in
   `resolve_tool_approval_resolves_async_status_from_generic_function`,
   `resolve_tool_approval_passes_tool_call_tools_context_messages_and_runtime_to_generic_function`,
   `resolve_tool_approval_passes_through_object_status_reason_from_generic_function`,
   `resolve_tool_approval_passes_same_messages_and_validated_tool_context_to_per_tool_function`,
   `resolve_tool_approval_passes_tools_context_entry_through_after_schema_validation`,
   `resolve_tool_approval_normalizes_static_string_before_tool_defined_approval`,
   `resolve_tool_approval_passes_through_static_object_status_reason`,
   `resolve_tool_approval_uses_user_defined_callback_before_tool_defined_approval`,
   `resolve_tool_approval_passes_reason_returned_by_user_defined_callback`,
   `resolve_tool_approval_normalizes_string_status_returned_by_user_defined_callback`,
   `resolve_tool_approval_treats_none_from_generic_callback_as_not_applicable`,
   `resolve_tool_approval_uses_generic_callback_before_tool_defined_approval`,
   `resolve_tool_approval_treats_none_from_per_tool_callback_as_not_applicable`,
   and `resolve_tool_approval_passes_no_tool_context_without_context_schema`,
   proving generic callbacks run before tool-defined approval callbacks,
   async callback results resolve, option payloads and object-status reasons
   are preserved, static statuses short-circuit tool-defined callbacks,
   validated/schema-transformed contexts reach per-tool callbacks, missing
   callback returns normalize to `not-applicable`, and absent `toolsContext`
   entries become absent `toolContext` options. JavaScript reference-identity
   checks are documented as Rust value-equivalence at the owned callback
   boundary.
   The upstream `generate-text/sum-token-counts.test.ts` cases now have named
   Rust counterparts in `sum_token_counts_should_*`. The upstream
   `generate-text/calculate-tokens-per-second.test.ts` portable cases now have
   named Rust counterparts in `calculate_tokens_per_second_should_*`; its
   non-finite `Number.POSITIVE_INFINITY`/`Number.NaN` token-count case is
   JavaScript-number-only at Rust's `Option<u64>` token-count boundary.
   The upstream `generate-text/to-response-messages.test.ts` cases now have 27
   named Rust counterparts in `to_response_messages_should_*`, covering all
   portable assistant, tool-message, provider-executed, approval, metadata,
   file/reasoning/custom, empty-text, empty-content, and invalid tool-input
   cases.
   The upstream `generate-text/parse-tool-call.test.ts` validation, no-tool,
   provider-metadata, dynamic-tool, title, tool-metadata, repair, and
   refinement slices now have named Rust counterparts in
   `parse_tool_call_should_successfully_parse_a_valid_tool_call`,
   `parse_tool_call_should_refine_input_after_successfully_parsing_a_valid_tool_call`,
   `parse_tool_call_should_successfully_parse_a_valid_provider_executed_dynamic_tool_call`,
   `parse_tool_call_should_successfully_parse_a_valid_tool_call_with_provider_metadata`,
   `parse_tool_call_should_successfully_process_empty_tool_calls_for_tools_that_have_no_input_schema`,
   `parse_tool_call_should_successfully_process_empty_object_tool_calls_for_tools_that_have_no_input_schema`,
   `parse_tool_call_should_throw_no_such_tool_error_when_tools_is_null`,
   `parse_tool_call_should_throw_no_such_tool_error_when_tool_is_not_found`,
   `parse_tool_call_should_throw_invalid_tool_input_error_when_args_are_invalid`,
   `parse_tool_call_should_set_dynamic_to_true_for_dynamic_tools`,
   `parse_tool_call_should_include_title_in_parsed_dynamic_tool_call`,
   `parse_tool_call_should_include_title_in_parsed_static_tool_call`,
   `parse_tool_call_should_include_title_in_invalid_tool_call`,
   `parse_tool_call_should_propagate_tool_metadata_onto_a_parsed_dynamic_tool_call`,
   `parse_tool_call_should_propagate_tool_metadata_onto_a_parsed_static_tool_call`,
   `parse_tool_call_should_keep_tool_metadata_separate_from_model_supplied_provider_metadata`,
   `parse_tool_call_should_propagate_tool_metadata_onto_an_invalid_tool_call`,
   `parse_tool_call_repair_should_invoke_repair_tool_when_provided_and_use_its_result`,
   `parse_tool_call_repair_should_invoke_repair_tool_when_input_schema_validation_fails`,
   `parse_tool_call_repair_should_pass_instructions_to_repair_tool_call`,
   `parse_tool_call_repair_should_rethrow_error_if_tool_call_repair_returns_null`,
   `parse_tool_call_repair_should_throw_tool_call_repair_error_if_repair_tool_call_throws`,
   and `generate_text_marks_schema_invalid_tool_input_before_execution`. Exact
   JavaScript `Error` instance identity and Zod snapshot wording remain
   non-portable at Rust's native error type boundary.
   The upstream `generate-text/execute-tool-call.test.ts` basic execution
   slice now has named Rust counterparts in
   `execute_tool_call_should_return_none_when_tool_has_no_execute_function`,
   `execute_tool_call_should_return_tool_result_with_correct_data`,
   `execute_tool_call_should_preserve_provider_metadata_from_tool_call_on_success`,
   `execute_tool_call_should_preserve_tool_metadata_from_tool_call_on_success`,
   `execute_tool_call_should_preserve_metadata_from_tool_call_on_error`,
   `execute_tool_call_should_set_dynamic_true_for_dynamic_tools_on_success_and_error`,
   and
   `execute_tool_call_should_return_tool_error_when_tool_context_schema_fails_validation`;
   the callback/sandbox slice added
   `execute_tool_call_should_pass_sandbox_to_tool_execution`,
   `execute_tool_call_should_call_start_callback_with_correct_data_before_execution`,
   `execute_tool_call_should_not_break_execution_when_start_callback_panics`,
   `execute_tool_call_should_call_end_callback_with_success_data_when_tool_succeeds`,
   `execute_tool_call_should_call_end_callback_with_error_data_when_tool_fails`,
   `execute_tool_call_should_not_break_execution_when_end_callback_panics_on_success`,
   `execute_tool_call_should_not_break_execution_when_end_callback_panics_on_error`,
   `execute_tool_call_should_record_tool_execution_duration_on_success`,
   `execute_tool_call_should_record_tool_execution_duration_on_error`,
   `execute_tool_call_should_return_none_when_tools_is_empty`, and
   `execute_tool_call_should_return_none_when_tool_is_not_found_in_tools`;
   exact mocked JavaScript `now()` sequencing is replaced by native Rust
   monotonic duration equality between callback event and returned performance
   map. The abort/preliminary slice added
   `execute_tool_call_should_pass_abort_signal_to_tool_execution_when_available`,
   `execute_tool_call_should_not_pass_abort_signal_when_unavailable`,
   `execute_tool_call_should_call_preliminary_tool_result_callback_for_preliminary_results`,
   and
   `execute_tool_call_should_return_final_result_even_with_preliminary_results`;
   Rust maps upstream async-generator tool outputs through
   `Tool::with_execute_outputs`/`execute_tool` records, with final output
   normalization handled by provider-utils. The timeout/callback-array/
   telemetry-wrapper slice added
   `execute_tool_call_should_return_tool_result_when_tool_completes_before_timeout`,
   `execute_tool_call_should_pass_abort_signal_when_tool_timeout_is_set`,
   `execute_tool_call_should_merge_tool_timeout_with_existing_abort_signal`,
   `execute_tool_call_should_use_per_tool_timeout_without_default_tool_timeout`,
   `execute_tool_call_should_not_create_abort_signal_when_tool_timeout_does_not_match`,
   `execute_tool_call_should_call_all_start_listeners_in_an_array`,
   `execute_tool_call_should_call_all_end_listeners_in_an_array`,
   `execute_tool_call_should_not_break_when_one_listener_in_array_panics`,
   `execute_tool_call_should_execute_tool_inside_telemetry_context_wrapper_when_provided`,
   and
   `execute_tool_call_should_measure_only_inner_execute_duration_when_wrapped`.
   Rust documents JavaScript object identity checks for merged `AbortSignal`
   as native signal-linkage assertions instead of pointer identity.
   The upstream `execute-tools-from-stream.test.ts` context-validation error
   case now has the named high-level Rust counterpart
   `stream_text_validates_tool_context_before_approval_callback_and_execution`,
   proving invalid streamed tool context prevents approval callbacks and local
   tool execution before the tool-error output is surfaced. The exact
   JavaScript thrown-stream `TypeValidationError` boundary is represented as
   Rust's materialized stream result with the same pre-callback/pre-execution
   ordering guarantee.
   The upstream `generate-text/prune-messages.test.ts` cases now have named
   Rust counterparts in `prune_messages_should_*`, including all reasoning
   removal, before-last-message reasoning removal, all tool-call/result/error
   and approval pruning, before-last-message and before-last-2-message
   tool-reference preservation, multi-turn pruning when the final message has
   no tool calls, and sequential tool-specific pruning settings.
   The upstream `generate-text/stop-condition.test.ts` portable built-in
   predicate cases now have named Rust counterparts in
   `is_step_count_should_*`, `is_loop_finished_should_*`,
   `has_tool_call_should_*`, and `is_stop_condition_met_should_*`. The two
   upstream JavaScript async/rejection cases are documented as non-portable at
   the current Rust enum-based stop-condition boundary because there is no
   public caller-supplied async predicate or rejected Promise surface.
   The upstream `model/resolve-model.test.ts` current-provider cases now have
   named Rust counterparts in `resolve_*_model_should_*`, covering direct model
   identity, Gateway fallback model-id resolution, explicit default-provider
   model-id resolution, and typed optional video/reranking missing-model
   errors. The legacy v2/v3 conversion and unsupported-version throw cases are
   JavaScript runtime/object boundaries documented in the compatibility row.
   The upstream `packages/ai/src/model/as-*-v3.test.ts` and `as-*-v4.test.ts`
   legacy adapter corpus is documented as JavaScript-only package compatibility
   because Rust exposes only the current provider-v4 trait boundary and has no
   public legacy v2/v3 provider object versions, JavaScript prototypes, or Web
   `ReadableStream` object identity to adapt.
   Continue from the remaining utility corpus such as runtime-specific helpers,
   documenting JavaScript-only stream, server-response, and fetch cases where
   Rust has no equivalent runtime boundary.
8. Continue `streamText` parity with true post-return `createUIMessageStream`
   delayed-merge behavior if a live stream abstraction is introduced.
   The upstream `result.textStream` empty text-delta filtering and reasoning
   exclusion cases now have named Rust counterparts in
   `stream_text_result_text_stream_filters_out_empty_text_deltas` and
   `stream_text_result_text_stream_excludes_reasoning_content`. The upstream
   `result.fullStream` text-delta and reasoning-delta cases now have named
   Rust counterparts in `stream_text_result_full_stream_sends_text_deltas` and
   `stream_text_result_full_stream_sends_reasoning_deltas`. The upstream
   source and custom part cases now have named Rust counterparts in
   `stream_text_result_full_stream_sends_sources` and
   `stream_text_result_full_stream_sends_custom_parts`. The upstream generated
   file and reasoning-file cases now have named Rust counterparts in
   `stream_text_result_full_stream_sends_files`,
   `stream_text_result_full_stream_sends_files_with_provider_metadata`, and
   `stream_text_result_full_stream_sends_reasoning_files`. The upstream
   fallback response metadata case now has a named Rust counterpart in
   `stream_text_result_full_stream_uses_fallback_response_metadata_when_response_metadata_missing`.
   The upstream direct tool-call snapshot now has a named Rust counterpart in
   `stream_text_result_full_stream_sends_tool_calls`. The upstream tool-input
   refinement callback/stream-part case now has a named Rust counterpart in
   `stream_text_result_full_stream_refines_tool_input_before_execution_parts_and_callbacks`.
   The upstream tool-call delta, providerMetadata on tool-input-start, local
   tool-result, and delayed asynchronous tool-result cases now have named Rust
   counterparts in `stream_text_result_full_stream_sends_tool_call_deltas`,
   `stream_text_result_full_stream_passes_provider_metadata_on_tool_input_start`,
   `stream_text_result_full_stream_sends_tool_results`, and
   `stream_text_result_full_stream_sends_delayed_asynchronous_tool_results`.
   The portable upstream error callback cases for mid-stream provider error
   chunks and second-step continuation stream errors now have named Rust
   counterparts in
   `stream_text_invokes_finish_callback_when_error_chunk_occurs_mid_stream`
   and `stream_text_invokes_error_callback_when_error_occurs_in_second_step`.
   The upstream `result.toUIMessageStream` default/custom error masking cases
   and `result.pipeUIMessageStreamToResponse` data stream, custom response
   init, default/custom error masking, `sendFinish: false`, reasoning, source,
   and file cases now have named Rust counterparts in
   `stream_text_result_to_ui_message_stream_masks_error_messages_by_default`,
   `stream_text_result_to_ui_message_stream_supports_custom_error_messages`,
   `stream_text_result_pipe_ui_message_stream_to_response_writes_data_stream_parts`,
   `stream_text_result_pipe_ui_message_stream_to_response_applies_custom_headers`,
   `stream_text_result_pipe_ui_message_stream_to_response_masks_error_messages_by_default`,
   `stream_text_result_pipe_ui_message_stream_to_response_supports_custom_error_messages`,
   `stream_text_result_pipe_ui_message_stream_to_response_omits_finish_when_send_finish_false`,
   `stream_text_result_pipe_ui_message_stream_to_response_writes_reasoning_content`,
   `stream_text_result_pipe_ui_message_stream_to_response_writes_source_content`,
   and `stream_text_result_pipe_ui_message_stream_to_response_writes_file_content`.
   The upstream `multiple stream consumption` case now has the named Rust
   counterpart
   `stream_text_result_supports_text_ui_message_and_full_stream_from_single_result`,
   proving text-stream, full-stream, and UI-message views can be read from the
   same materialized `StreamTextResult`.
   The upstream `result.consumeStream` error-handling cases now have named
   Rust counterparts in `stream_text_result_consume_stream_*`, proving
   consumption ignores AbortError, ResponseAborted, and generic provider
   errors by default while `consume_stream_with_on_error` reports the error
   value to the callback. Rust consumes the materialized stream result rather
   than a Web `ReadableStream`, so JavaScript thrown `Error` object identity is
   represented as provider error JSON.
   The upstream automatic approval and automatic denial tool-approval stream
   cases now have Rust coverage in
   `stream_text_automatic_tool_approval_response_streams_before_tool_result`
   and the hardened
   `stream_text_applies_denied_tool_approval_to_continuation_messages`,
   proving `tool-approval-request` automatic metadata and
   `tool-approval-response` chunks are emitted into both `fullStream` and
   `toUIMessageStream` before approved local tool results or denied
   continuation prompts.
   The upstream `generateText` warning logger single-step, per-step multi-step,
   and empty-warning cases now have named Rust counterparts in
   `generate_text_calls_log_warnings_with_warnings_from_a_single_step`,
   `generate_text_calls_log_warnings_once_for_each_step_with_warnings_from_that_step`,
   and
   `generate_text_calls_log_warnings_with_empty_array_when_no_warnings_are_present`.
   The upstream `streamText` warning logger single-step, per-step multi-step,
   and empty-warning cases now have named Rust counterparts in
   `stream_text_calls_log_warnings_with_warnings_from_a_single_step`,
   `stream_text_calls_log_warnings_once_for_each_step_with_warnings_from_that_step`,
   and
   `stream_text_calls_log_warnings_with_empty_array_when_no_warnings_are_present`.
   The portable upstream `smoothStream` edge matrix now has named Rust
   counterparts for large text/reasoning splitting, whitespace buffering,
   line-mode final flushes, regex character splitting, text-id switching,
   tool-call and streamed tool-input flushes, text/reasoning switching,
   multi-switch ordering, and reasoning-start provider metadata preservation.
   Remaining `smoothStream` gaps are limited to JavaScript runtime boundaries:
   invalid untyped `chunking` construction and `Intl.Segmenter` locale
   segmentation.
9. Continue `streamObject` parity with remaining output-strategy stream-result
   edge cases after the Gateway text/stream/UI path is stronger.
   The upstream object-output `result.fullStream` ordering case now has a named
   Rust counterpart in
   `stream_object_result_full_stream_matches_upstream_object_chunks`.
   The current upstream `result.fullStream` finish providerMetadata/timestamp
   snapshot fields now have a named Rust counterpart in
   `stream_object_result_full_stream_sends_finish_provider_metadata_and_timestamp`.
   The upstream URL-file `options.messages` supported-URL hook cases for
   `generateObject`, `generateText`, `streamText`, and `streamObject` now have
   named Rust counterparts. The underlying `fixJson` and `parsePartialJson`
   partial-JSON utilities now have named one-to-one Rust coverage for every
   portable upstream test case. The deprecated `experimental_telemetry` alias
   cases for `generateObject`, `streamObject`, `generateText`, `streamText`,
   `embed`, `embedMany`, and `rerank` now have named Rust counterparts.
   The upstream `generateObject` `result.request`, `result.response`,
   `result.providerMetadata`, `options.headers`, and `options.providerOptions`
   cases now have named Rust counterparts. The upstream `generateObject`
   warning logger spy cases now have named Rust counterparts in
   `generate_object_calls_log_warnings_with_the_correct_warnings` and
   `generate_object_calls_log_warnings_with_empty_array_when_no_warnings_are_present`.
   The upstream `generateObject` callback cases now have named Rust
   counterparts in `generate_object_on_start_*`,
   `generate_object_on_step_start_*`, `generate_object_on_step_finish_*`,
   `generate_object_on_finish_*`, `generate_object_callbacks_fire_in_order`,
   `generate_object_callbacks_correlate_events_with_same_call_id`, and
   `generate_object_callbacks_should_not_break_generation_when_callback_panics`.
   The upstream `generate-object.test-d.ts` and `stream-object.test-d.ts`
   unsupported timeout-option assertions now have named Rust counterparts in
   `generate_object_type_counterpart_does_not_accept_timeout_option` and
   `stream_object_type_counterpart_does_not_accept_timeout_option`.
   The remaining portable upstream `generate-object.test-d.ts` result-type
   assertions now have typed Rust accessor counterparts for enum, schema,
   no-schema, and array output in
   `generate_object_type_counterpart_supports_enum_types`,
   `generate_object_type_counterpart_supports_schema_types`,
   `generate_object_type_counterpart_supports_no_schema_output_mode`, and
   `generate_object_type_counterpart_supports_array_output_mode`.
   The portable `stream-object.test-d.ts` result-type assertions now have typed
   Rust accessor counterparts for finish reason, schema, no-schema, enum, and
   array output. The upstream `callback ordering` call-id correlation case now
   has the named Rust counterpart
   `stream_object_callbacks_correlate_all_events_with_same_call_id`.
   The upstream callback error-handling case now has the named Rust counterpart
   `stream_object_callback_panics_do_not_break_stream`.
   The upstream warning logger spy cases now have named Rust counterparts in
   `stream_object_calls_log_warnings_with_the_correct_warnings` and
   `stream_object_calls_log_warnings_with_empty_array_when_no_warnings_are_present`.
   The upstream `partialObjectStream` provider-error suppression case now has
   the named Rust counterpart
   `stream_object_partial_object_stream_suppresses_provider_errors`.
   The upstream `result.objectStream` `onError` callback case for a rejected
   `doStream` call now has the named Rust counterpart
   `stream_object_object_stream_invokes_on_error_callback_with_error`, with the
   JavaScript promise rejection represented at Rust's typed provider boundary
   as an error stream part.
   The upstream object-stream `schemaName`/`schemaDescription` case now has the
   named Rust counterpart
   `stream_object_object_stream_uses_schema_name_and_description`.
   The upstream object-stream base delta case now has the named Rust
   counterpart `stream_object_object_stream_sends_object_deltas`.
10. Native Gateway provider package parity is closed for the current upstream
   inventory. The current `packages/gateway/src/**/*.test.ts` corpus has 372
   upstream `it`/`test` cases and the package-owned Rust `ai-sdk-gateway` crate
   lists 380 tests, with JavaScript-only request-context, callable-constructor,
   Date-object identity, thrown-error identity, cross-realm marker, and
   stack-trace assertions documented as non-portable. Do not spend another
   Gateway-provider slice unless upstream changes or a regression appears.
11. Do not resume unrelated standalone provider wrappers until the common/core
   SDK and Vercel AI Gateway rows above are verified or explicitly documented
   as intentionally non-portable. When standalone provider work resumes,
   continue it only as package-owned crates that match their upstream
   TypeScript packages; do not add new root-owned provider modules.
12. Crate splitting is an immediate hard acceptance gate, not optional cleanup
   after the port is otherwise complete. The Rust workspace must have a strict
   1:1 mapping between upstream `vercel/ai` TypeScript packages and Rust
   crates: every portable upstream package gets exactly one corresponding Rust
   crate, and that crate owns the package's public types, provider/options
   surfaces, implementation, docs, and tests. No Rust crate may own APIs from
   more than one upstream package. A slice that violates this boundary must be
   reworked before merge; it is not a successful port and must not be accepted
   as temporary progress.

   The current `ai-sdk-rust` root crate is already merging multiple upstream
   TypeScript packages into one Rust boundary. That is migration debt being
   created today, not a neutral staging choice. Every additional package folded
   into the root crate makes the eventual split harder, increases API coupling,
   and raises the risk of breaking users when the package boundary is finally
   extracted. Future work must stop growing this debt now. Adding another
   package-owned implementation to the root crate, or to any crate that already
   owns a different upstream package, is a regression even when the behavior and
   tests are otherwise correct.

   The root crate is a facade, not an implementation home for package-owned
   surfaces. If `ai-sdk-rust` is the Rust equivalent of upstream `packages/ai`,
   it may own only that package's API plus aggregate re-exports and
   compatibility shims. It must not also own `packages/provider`,
   `packages/provider-utils`, provider packages, MCP, Workflow, telemetry,
   framework adapters, or any other separately packaged upstream surface.
   Existing package-owned surfaces already merged into the root crate are
   extraction debt and should be moved behind their matching package crates
   before more API is layered on top of them. Do not mark those package rows
   `verified` until the crate ownership is correct.

   Before implementing or reviewing any parity slice, identify the upstream
   TypeScript package and create or use the matching Rust crate first. If the
   matching crate does not exist, crate creation is part of the slice, not a
   follow-up. A parity slice that ports a TypeScript package without its
   matching Rust crate is blocked and not mergeable, even if the API itself is
   otherwise implemented correctly. Passing tests in the wrong crate prove
   behavior, not parity. A new root module for package-owned API is also blocked
   unless it is only a re-export/compatibility shim.

   Temporary staging exceptions must not introduce new package-owned
   implementation. They may only cover unavoidable transitional shims or the
   extraction of existing root-crate debt, and only when the ledger names the
   destination crate, explains why the matching crate cannot land in the same
   slice, and records the smallest concrete extraction follow-up. Do not use
   that exception for convenience, to land a working implementation faster, or
   to continue consolidating unrelated packages into one crate. The acceptance
   target is one Rust crate per upstream TypeScript package, no crate owning
   APIs from multiple upstream packages, and the root crate limited to the
   `packages/ai` facade plus aggregate re-exports and compatibility shims.
