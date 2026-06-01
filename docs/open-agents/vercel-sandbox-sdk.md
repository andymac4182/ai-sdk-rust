# Vercel Sandbox SDK Rust Port

## Source Inventory

Rechecked on 2026-06-01:

- Official docs: <https://vercel.com/docs/vercel-sandbox/sdk-reference>
- Official source: <https://github.com/vercel/sandbox>, fetched with
  `npx opensrc fetch https://github.com/vercel/sandbox`
- Upstream commit observed through the GitHub API:
  `731d0a8fecf78c8fb951e736e290ba080557dd79`
- Package: `packages/vercel-sandbox/package.json`, `@vercel/sandbox` `2.0.2`
- Rust SDK search: crates.io exact search for `vercel-sandbox` returned no
  crates. A broader `vercel sandbox` search found third-party crates such as
  `arcan-provider-vercel`, owned by `broomva`, not an official Vercel SDK.

No official Vercel Rust SDK for Sandbox was found, so `open-agents-sandbox`
ports the TypeScript SDK surface needed by Open Agents against Vercel's v2 API.

## Ported Surface

The Rust port lives in `crates/open-agents-sandbox/src/vercel.rs` and is exposed
through the existing sandbox crate. It implements:

- `VercelSandboxClient` for `POST /v2/sandboxes`, `GET /v2/sandboxes/{name}`,
  `GET /v2/sandboxes`, `GET /v2/sandboxes/sessions/{id}`,
  command start/wait/log streaming, file read/write, mkdir, stop, timeout
  extension, and snapshot creation.
- Typed request/response models for sandbox metadata, sessions, routes,
  commands, sources, resources, and create requests.
- `VercelSandbox`, an implementation of the existing Open Agents `Sandbox`
  trait, so Vercel-shaped `SandboxState` no longer returns unsupported
  operation from `connect_sandbox`.
- The same gzip tar file upload shape used by `@vercel/sandbox` for
  `fs/write`.

## Configuration

Set `OPEN_AGENTS_SANDBOX=vercel` to select the backend. Live use requires:

- `OPEN_AGENTS_VERCEL_TOKEN`, `VERCEL_TOKEN`, or `VERCEL_OIDC_TOKEN`
- `VERCEL_TEAM_ID`
- `VERCEL_PROJECT_ID`

Use `OPEN_AGENTS_VERCEL_TOKEN` for deployed Open Agents services to avoid
conflicting with Vercel CLI build authentication.

Optional:

- `VERCEL_SANDBOX_NAME`, resume a named sandbox instead of always creating one
- `VERCEL_SANDBOX_BASE_SNAPSHOT_ID`, create from a snapshot when no name is set
- `VERCEL_SANDBOX_RUNTIME`, for example `node24`
- `VERCEL_SANDBOX_VCPUS`
- `VERCEL_SANDBOX_TIMEOUT_MS`
- `VERCEL_SANDBOX_PERSISTENT`
- `VERCEL_SANDBOX_API_BASE_URL`, test/private API override

## Test Inventory

Portable upstream cases mapped to Rust tests:

| Upstream area | Rust counterpart |
| --- | --- |
| `APIClient.createSandbox` request shape | `vercel_client_create_sandbox_sends_upstream_shape` |
| `APIClient.getSandbox` project/resume query | `vercel_client_get_sandbox_passes_project_and_resume_query` |
| `APIClient.listSandboxes` pagination query | `vercel_client_list_sandboxes_passes_project_and_cursor_query` |
| `APIClient.runCommand(wait: true)` NDJSON command stream | `vercel_client_run_command_wait_reads_finished_chunk_and_logs` |
| `APIClient.getLogs` stdout/stderr/error handling | `vercel_client_run_command_wait_reads_finished_chunk_and_logs`; `vercel_client_api_errors_include_status_and_message` |
| `APIClient.readFile` 404 and content-type checks | `vercel_client_read_file_maps_404_to_none_and_content_type_errors` |
| `APIClient.writeFiles` gzip tar upload | `vercel_client_write_files_uses_gzip_tar_upload_shape` |
| `APIClient.extendTimeout` and `createSnapshot` | `vercel_client_extend_timeout_and_snapshot_parse_session_updates` |
| Open Agents connect/resume/exec/read/write/stat/list/domain/stop/state | `vercel_sandbox_backend_connects_execs_reads_writes_lists_and_stops` |
| Live create/exec/read/write/list/stop smoke | `live_vercel_sandbox_create_exec_read_write_list_stop_smoke` ignored by default |

Explicit exceptions:

- TypeScript compile-time tests, package export tests, and `@workflow/serde`
  serialization tests are JavaScript/toolchain-only.
- CLI tests under `packages/sandbox`, interactive shell/WebSocket tests, proxy
  OIDC-forwarding tests, and `pty-tunnel` tests are outside the Open Agents
  Rust sandbox trait boundary.
- High-level Node `fs/promises` convenience methods that are implemented in
  TypeScript by shelling out (`appendFile`, `chmod`, `rename`, `symlink`, and
  similar) are not public Open Agents sandbox trait methods. The portable
  primitive operations they depend on are covered by the client/backend tests
  above.
