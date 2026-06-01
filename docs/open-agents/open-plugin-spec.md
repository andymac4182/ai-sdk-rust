# Open Plugin Spec Tracker

This tracker covers the Open Plugin v1.0.0 MCP integration slice for Open
Agents. Source was refreshed with:

```text
npx opensrc fetch https://github.com/vercel-labs/open-plugin-spec
```

Local source inspected:
`/Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel-labs/open-plugin-spec/main/README.md`.

## OP-03 MCP Integration

`open-agents-core::open_plugin` is the owning Rust surface for this slice. It
loads Open Plugin package manifests, discovers MCP server declarations, expands
Open Plugin placeholders in runtime fields, returns deterministic
Open Agents-facing config, and emits non-fatal diagnostics. It does not start
stdio/http/sse MCP transports; later runtime code can map the returned config to
`ai-sdk-mcp` (`StdioConfig`, `StdioMcpTransport`, `McpTransportConfig::http`,
`McpTransportConfig::sse`, and `McpClientConfig`).

| Spec source | Requirement | Rust evidence | Status |
| --- | --- | --- | --- |
| README section 4.1 | Manifest-declared relative paths must start with `./` and must not escape the plugin root. | `resolve_manifest_mcp_path`; `open_plugin_mcp_rejects_invalid_paths_and_keeps_valid_sources` | verified |
| README sections 6.4 and 7.1-7.2 | Use default `.mcp.json` only when no manifest `mcpServers` field is present; manifest paths override defaults. | `default_mcp_sources`; `manifest_mcp_sources`; `open_plugin_mcp_loads_default_mcp_json_when_manifest_field_absent`; `open_plugin_mcp_manifest_path_override_skips_default_config` | verified |
| README section 6.5 | Support path config object shape with `paths`. | `manifest_mcp_sources`; `open_plugin_mcp_duplicate_server_names_emit_conflict_and_first_source_wins` | verified |
| README section 6.6 and Appendix A | Treat ambiguous or unrecognized object field shapes as non-fatal diagnostics. | `OPEN_PLUGIN_INVALID_OBJECT_EVENT`; `open_plugin_mcp_invalid_manifest_object_shape_is_non_fatal` | verified |
| README section 8.2 | Load config files or inline config containing top-level `mcpServers`; require manifest paths to be explicit files, not directories. | `load_mcp_config_value`; `read_mcp_config_file`; `open_plugin_mcp_inline_config_uses_manifest_servers`; `open_plugin_mcp_rejects_invalid_paths_and_keeps_valid_sources`; `open_plugin_mcp_invalid_config_shape_does_not_block_other_sources` | verified |
| README section 8.2.2 and Appendix A | Duplicate MCP server names across discovery sources must not crash loading; warn and choose a deterministic winner. | `OPEN_PLUGIN_MCP_NAME_CONFLICT_EVENT`; first source wins; `open_plugin_mcp_duplicate_server_names_emit_conflict_and_first_source_wins` | verified |
| README section 9.2 | Surface MCP tool ids with plugin and server names. | `open_plugin_mcp_tool_id`; `OpenPluginMcpServerConfig::namespaced_tool_id`; `open_plugin_mcp_namespaced_tool_id_matches_spec_format` | verified |
| README section 10 | Expand `${PLUGIN_ROOT}` in MCP `command`, `args`, `env`, and `cwd`; expand `${PLUGIN_DATA}` when a plugin data directory is configured. | `expand_mcp_runtime_fields`; `expand_placeholders`; `open_plugin_mcp_expands_plugin_root_and_data_placeholders` | verified |
| README section 12 | Continue loading supported components when MCP declaration failures are independent and non-fatal. | `OpenPluginDiagnostic`; `open_plugin_mcp_invalid_config_shape_does_not_block_other_sources`; invalid paths/configs are skipped while valid sources load | verified |

## Naming

Open Agents follows the Open Plugin v1 recommended model-compatible identifier
format exactly:

```text
mcp__plugin_{plugin-name}_{server-name}__{tool-name}
```

The current implementation does not rewrite plugin, server, or tool names. This
matches the spec examples and keeps double underscores as the parse boundaries.

## Runtime Launch Deferral

OP-03 deliberately stops before process or network startup. The output config
captures stdio command/args/env/cwd and http/sse URL/header data in deterministic
Rust types. Starting subprocesses, discovering live MCP tools, and adapting
those live tool definitions into model tools remains a later runtime slice.
