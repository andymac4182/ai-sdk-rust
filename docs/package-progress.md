# AI SDK Rust Package Progress

_Generated from `docs/upstream-parity.md` and `docs/package-progress-estimates.tsv`._

- Displayed package rows: 52
- Average estimated completion: 85.2%
- Portable package average: 81.7%
- Closed package rows: 37 / 52
- Strict portable verified rows: 27 / 42
- In-progress rows: 15
- Not-started rows: 0

## 100% Closed

| Package | Completion | Status | Kind |
| --- | ---: | --- | --- |
| `@ai-sdk/provider-utils` | 100% | Verified | provider support library |
| `@ai-sdk/provider` | 100% | Verified | provider contracts |
| `@ai-sdk/gateway` | 100% | Verified | provider package |
| `@ai-sdk/openai-compatible` | 100% | Verified | provider base package |
| `@ai-sdk/open-responses` | 100% | Verified | provider package |
| `@ai-sdk/anthropic` | 100% | Verified | provider package |
| `@ai-sdk/assemblyai` | 100% | Verified | provider package |
| `@ai-sdk/azure` | 100% | Verified | provider package |
| `@ai-sdk/baseten` | 100% | Verified | provider package |
| `@ai-sdk/bytedance` | 100% | Verified | provider package |
| `@ai-sdk/cerebras` | 100% | Verified | provider package |
| `@ai-sdk/deepgram` | 100% | Verified | provider package |
| `@ai-sdk/deepinfra` | 100% | Verified | provider package |
| `@ai-sdk/deepseek` | 100% | Verified | provider package |
| `@ai-sdk/elevenlabs` | 100% | Verified | provider package |
| `@ai-sdk/gladia` | 100% | Verified | provider package |
| `@ai-sdk/huggingface` | 100% | Verified | provider package |
| `@ai-sdk/hume` | 100% | Verified | provider package |
| `@ai-sdk/lmnt` | 100% | Verified | provider package |
| `@ai-sdk/mistral` | 100% | Verified | provider package |
| `@ai-sdk/moonshotai` | 100% | Verified | provider package |
| `@ai-sdk/perplexity` | 100% | Verified | provider package |
| `@ai-sdk/revai` | 100% | Verified | provider package |
| `@ai-sdk/vercel` | 100% | Verified | provider package |
| `@ai-sdk/voyage` | 100% | Verified | provider package |
| `@ai-sdk/otel` | 100% | Verified | telemetry package |
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
| `@ai-sdk/amazon-bedrock` | 5% | In progress | provider package | Package-owned crate boundary plus row-level upstream test/source inventory exists; all portable runtime behavior... |
| `@ai-sdk/google` | 5% | In progress | provider package | Package-owned crate boundary plus row-level upstream test/source inventory exists; all portable runtime behavior... |
| `@ai-sdk/google-vertex` | 5% | In progress | provider package | Package-owned crate boundary plus row-level upstream test/source inventory exists; all portable runtime behavior... |
| `@ai-sdk/xai` | 35% | In progress | provider package | Initial Responses, chat, and image provider wrapper exists; files, video, xAI server tools, custom usage/message... |
| `@ai-sdk/alibaba` | 70% | In progress | provider package | Initial provider crate exists with mapped deterministic chat/video provider, usage, cache-control, message-conversion,... |
| `@ai-sdk/black-forest-labs` | 80% | In progress | provider package | Initial provider crate exists with deterministic image request/response, polling, warning, and error metadata fixtures... |
| `@ai-sdk/cohere` | 35% | In progress | provider package | Embedding and reranking model slices exist; chat, prompt conversion, tool prep, streaming, citations, and full current... |
| `@ai-sdk/fal` | 65% | In progress | provider package | Image and video media generation landed with request/response, queue polling, warning, download, and error metadata... |
| `@ai-sdk/fireworks` | 45% | In progress | provider package | OpenAI-compatible chat, completion, embedding, and image wrapper exists with Fireworks option transform; custom image... |
| `@ai-sdk/groq` | 35% | In progress | provider package | Initial OpenAI-compatible chat wrapper exists; transcription, browser-search tool, Groq-specific message and usage... |
| `@ai-sdk/klingai` | 75% | In progress | provider package | Video generation landed with auth/header, task polling, warning, response metadata, and error metadata fixtures; JWT... |
| `@ai-sdk/luma` | 70% | In progress | provider package | Initial provider crate exists with deterministic image request/response, polling, warning, and error metadata fixtures... |
| `@ai-sdk/prodia` | 65% | In progress | provider package | Image and video media generation landed with JSON/multipart request, binary response, URL media download, and error... |
| `@ai-sdk/replicate` | 75% | In progress | provider package | Image and video media generation landed with prediction request, polling/wait headers, downloads, warning, response... |
| `@ai-sdk/togetherai` | 65% | In progress | provider package | Provider wrapper covers chat, completion, embeddings, custom image generation, and reranking; exact per-case upstream... |

## Not Started

| Package | Completion | Status | Kind |
| --- | ---: | --- | --- |
