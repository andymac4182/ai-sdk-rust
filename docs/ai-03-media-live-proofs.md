# AI-03 Media Generation Live Proofs

AI-03 is covered by deterministic fixture tests for portable request and
response behavior, polling, warnings, downloads, multipart handling, and error
metadata. Live media-provider proofs remain credential-gated and are not run by
default.

## Deterministic Fixture Command

```sh
cargo test \
  -p ai-sdk-black-forest-labs \
  -p ai-sdk-luma \
  -p ai-sdk-replicate \
  -p ai-sdk-fal \
  -p ai-sdk-klingai \
  -p ai-sdk-prodia
```

## Credential Gates

| Provider package | Environment variables |
| --- | --- |
| `@ai-sdk/black-forest-labs` | `BFL_API_KEY` |
| `@ai-sdk/luma` | `LUMA_API_KEY` |
| `@ai-sdk/replicate` | `REPLICATE_API_TOKEN` |
| `@ai-sdk/fal` | `FAL_API_KEY` or `FAL_KEY` |
| `@ai-sdk/klingai` | `KLINGAI_ACCESS_KEY`, `KLINGAI_SECRET_KEY` |
| `@ai-sdk/prodia` | `PRODIA_TOKEN` |

Ignored live tests should live beside the corresponding provider crate tests and
must skip unless the required environment variables are present. They should be
run only with explicit credentials and should not be part of the default local
or CI verification path.
