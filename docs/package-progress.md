# AI SDK Rust Package Progress

_Generated from `docs/upstream-parity.md` and `docs/package-progress-estimates.tsv` with strict test inventory `docs/ai-strict-test-inventory.md`._

- Displayed package rows: 54
- Average estimated completion: 54.9%
- Portable package average: 44.7%
- Closed package rows: 21 / 54
- Strict portable verified rows: 11 / 44
- In-progress rows: 33
- Not-started rows: 0
- Strict inventory full upstream cases scanned: 9013
- Strict inventory full portable cases mapped: 5493 / 7671
- Strict inventory full portable cases unmapped: 2178
- Displayed-row strict portable test cases mapped: 5175 / 7268
- Displayed-row strict portable test cases unmapped: 2093

## 100% Closed

| Package | Completion | Status | Kind |
| --- | ---: | --- | --- |
| `@ai-sdk/provider-utils` | 100% | Verified | provider support library |
| `@ai-sdk/provider` | 100% | Verified | provider contracts |
| `@ai-sdk/openai-compatible` | 100% | Verified | provider base package |
| `@ai-sdk/open-responses` | 100% | Verified | provider package |
| `@ai-sdk/alibaba` | 100% | Verified | provider package |
| `@ai-sdk/cohere` | 100% | Verified | provider package |
| `@ai-sdk/fireworks` | 100% | Verified | provider package |
| `@ai-sdk/groq` | 100% | Verified | provider package |
| `@ai-sdk/klingai` | 100% | Verified | provider package |
| `@ai-sdk/vercel` | 100% | Verified | provider package |
| `@ai-sdk/otel` | 100% | Verified | telemetry package |
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
| `ai` | 44% | In progress | root core SDK package | strict test inventory: 1302 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/gateway` | 77% | In progress | provider package | strict test inventory: 90 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/openai` | 98% | In progress | provider package | strict test inventory: 6 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/anthropic` | 97% | In progress | provider package | strict test inventory: 10 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/amazon-bedrock` | 99% | In progress | provider package | strict test inventory: 2 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/google` | 94% | In progress | provider package | strict test inventory: 30 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/google-vertex` | 99% | In progress | provider package | strict test inventory: 2 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/xai` | 99% | In progress | provider package | strict test inventory: 1 portable upstream cases still need named Rust tests; sample failing IDs: `packages-xai-0136` |
| `@ai-sdk/assemblyai` | 0% | In progress | provider package | strict test inventory: 6 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/azure` | 0% | In progress | provider package | strict test inventory: 55 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/baseten` | 0% | In progress | provider package | strict test inventory: 25 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/black-forest-labs` | 0% | In progress | provider package | strict test inventory: 24 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/bytedance` | 0% | In progress | provider package | strict test inventory: 41 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/cerebras` | 0% | In progress | provider package | strict test inventory: 13 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/deepgram` | 0% | In progress | provider package | strict test inventory: 25 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/deepinfra` | 0% | In progress | provider package | strict test inventory: 25 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/deepseek` | 0% | In progress | provider package | strict test inventory: 38 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/elevenlabs` | 0% | In progress | provider package | strict test inventory: 15 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/fal` | 83% | In progress | provider package | strict test inventory: 12 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/gladia` | 0% | In progress | provider package | strict test inventory: 7 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/huggingface` | 0% | In progress | provider package | strict test inventory: 37 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/hume` | 0% | In progress | provider package | strict test inventory: 9 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/lmnt` | 0% | In progress | provider package | strict test inventory: 9 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/luma` | 0% | In progress | provider package | strict test inventory: 29 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/mistral` | 0% | In progress | provider package | strict test inventory: 72 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/moonshotai` | 0% | In progress | provider package | strict test inventory: 17 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/perplexity` | 0% | In progress | provider package | strict test inventory: 32 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/prodia` | 0% | In progress | provider package | strict test inventory: 54 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/replicate` | 0% | In progress | provider package | strict test inventory: 63 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/revai` | 0% | In progress | provider package | strict test inventory: 6 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/togetherai` | 75% | In progress | provider package | strict test inventory: 10 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/voyage` | 0% | In progress | provider package | strict test inventory: 21 portable upstream cases still need named Rust tests; sample failing IDs:... |
| `@ai-sdk/test-server` | 0% | In progress | testing support package | strict test inventory: 5 portable upstream cases still need named Rust tests; sample failing IDs:... |

## Not Started

| Package | Completion | Status | Kind |
| --- | ---: | --- | --- |
