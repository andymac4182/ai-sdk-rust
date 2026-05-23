# AI SDK Rust Package Progress

_Generated from `docs/upstream-parity.md` and `docs/package-progress-estimates.tsv`._

- Displayed package rows: 56
- Average estimated completion: 50.4%
- Portable package average: 39.6%
- Closed package rows: 13 / 56
- Strict portable verified rows: 3 / 46
- In-progress rows: 28
- Not-started rows: 15

## 100% Closed

| Package | Completion | Status | Kind |
| --- | ---: | --- | --- |
| `@ai-sdk/gateway` | 100% | Verified | provider package |
| `@ai-sdk/openai-compatible` | 100% | Verified | provider base package |
| `@ai-sdk/test-server` | 100% | Verified | testing support package |
| `@ai-sdk/devtools` | 100% | JavaScript-only | JavaScript devtools package |
| `@ai-sdk/codemod` | 100% | JavaScript-only | JavaScript migration tooling |
| `@ai-sdk/angular` | 100% | JavaScript-only | JavaScript framework adapter |
| `@ai-sdk/react` | 100% | JavaScript-only | JavaScript framework adapter |
| `@ai-sdk/rsc` | 100% | JavaScript-only | JavaScript framework adapter |
| `@ai-sdk/svelte` | 100% | JavaScript-only | JavaScript framework adapter |
| `@ai-sdk/vue` | 100% | JavaScript-only | JavaScript framework adapter |
| `@ai-sdk/langchain` | 100% | JavaScript-only | JavaScript library adapter |
| `@ai-sdk/llamaindex` | 100% | JavaScript-only | JavaScript library adapter |
| `@ai-sdk/valibot` | 100% | JavaScript-only | JavaScript schema adapter |

## In Progress

| Package | Est. completion | Status | Kind | Basis / remaining work |
| --- | ---: | --- | --- | --- |
| `ai` | 98% | In progress | core package | Core surface is broad; abort, callback, serial job, prepare-retries, request-timeout, simulate-readable-stream,... |
| `@ai-sdk/provider` | 75% | In progress | provider contracts | Provider-v4 contracts are largely represented; v2/v3 compatibility and exact stream abstractions remain. |
| `@ai-sdk/provider-utils` | 82% | In progress | provider support library | Most runtime helpers and many type-contract cases are mapped; browser stream/fetch parity and remaining type-level/Zod... |
| `@ai-sdk/openai` | 65% | In progress | provider package | OpenAI/Open Responses foundations exist; broader Responses, files, speech, transcription, and provider surfaces remain. |
| `@ai-sdk/open-responses` | 88% | In progress | provider package | Most fixture, tool, prompt, request, metadata, and stream slices are mapped; remaining... |
| `@ai-sdk/assemblyai` | 45% | In progress | provider package | Initial provider crate exists; full upstream test and live-provider parity remain. |
| `@ai-sdk/azure` | 45% | In progress | provider package | Initial provider crate exists; full upstream test and live-provider parity remain. |
| `@ai-sdk/baseten` | 35% | In progress | provider package | Initial provider wrapper exists; broader package behavior and live proof remain. |
| `@ai-sdk/black-forest-labs` | 45% | In progress | provider package | Initial provider crate exists; full upstream test and live-provider parity remain. |
| `@ai-sdk/bytedance` | 45% | In progress | provider package | Initial provider crate exists; full upstream test and live-provider parity remain. |
| `@ai-sdk/cerebras` | 45% | In progress | provider package | Initial provider wrapper exists; broader package behavior and live proof remain. |
| `@ai-sdk/deepgram` | 40% | In progress | provider package | Initial provider crate exists; full upstream test and live-provider parity remain. |
| `@ai-sdk/deepinfra` | 45% | In progress | provider package | Initial provider wrapper exists; broader package behavior and live proof remain. |
| `@ai-sdk/deepseek` | 45% | In progress | provider package | Initial provider crate exists; full upstream test and live-provider parity remain. |
| `@ai-sdk/huggingface` | 40% | In progress | provider package | Initial provider wrapper exists; SSE/tool parity and broader package behavior remain. |
| `@ai-sdk/hume` | 40% | In progress | provider package | Initial provider crate exists; full upstream test and live-provider parity remain. |
| `@ai-sdk/lmnt` | 45% | In progress | provider package | Initial provider crate exists; full upstream test and live-provider parity remain. |
| `@ai-sdk/luma` | 45% | In progress | provider package | Initial provider crate exists; full upstream test and live-provider parity remain. |
| `@ai-sdk/mistral` | 45% | In progress | provider package | Initial provider crate exists; full upstream test and live-provider parity remain. |
| `@ai-sdk/moonshotai` | 45% | In progress | provider package | Initial provider crate exists; full upstream test and live-provider parity remain. |
| `@ai-sdk/perplexity` | 45% | In progress | provider package | Initial provider crate exists; full upstream test and live-provider parity remain. |
| `@ai-sdk/revai` | 45% | In progress | provider package | Initial provider crate exists; full upstream test and live-provider parity remain. |
| `@ai-sdk/togetherai` | 45% | In progress | provider package | Initial provider wrapper exists; broader package behavior and live proof remain. |
| `@ai-sdk/vercel` | 40% | In progress | provider package | Construction wrapper exists; live validation and full package test parity remain. |
| `@ai-sdk/voyage` | 45% | In progress | provider package | Initial provider wrapper exists; broader package behavior and live proof remain. |
| `@ai-sdk/mcp` | 90% | In progress | protocol/client package | Protocol, transports, OAuth, examples, and Gateway tool bridge are broad; protected live auth validation and hosted... |
| `@ai-sdk/otel` | 82% | In progress | telemetry package | Package helpers, span lifecycle, local OTLP export, and Gateway live telemetry proof exist; broader provider live... |
| `@ai-sdk/workflow` | 72% | In progress | agent/workflow package | WorkflowAgent and iterator callback/continuation coverage is substantial; real model execution, HTTP/SSE adapters, and... |

## Not Started

| Package | Completion | Status | Kind |
| --- | ---: | --- | --- |
| `@ai-sdk/anthropic` | 0% | Not started | provider package |
| `@ai-sdk/amazon-bedrock` | 0% | Not started | provider package |
| `@ai-sdk/google` | 0% | Not started | provider package |
| `@ai-sdk/google-vertex` | 0% | Not started | provider package |
| `@ai-sdk/xai` | 0% | Not started | provider package |
| `@ai-sdk/alibaba` | 0% | Not started | provider package |
| `@ai-sdk/cohere` | 0% | Not started | provider package |
| `@ai-sdk/elevenlabs` | 0% | Not started | provider package |
| `@ai-sdk/fal` | 0% | Not started | provider package |
| `@ai-sdk/fireworks` | 0% | Not started | provider package |
| `@ai-sdk/gladia` | 0% | Not started | provider package |
| `@ai-sdk/groq` | 0% | Not started | provider package |
| `@ai-sdk/klingai` | 0% | Not started | provider package |
| `@ai-sdk/prodia` | 0% | Not started | provider package |
| `@ai-sdk/replicate` | 0% | Not started | provider package |
