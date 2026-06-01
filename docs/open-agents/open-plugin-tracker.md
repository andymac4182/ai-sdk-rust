# Open Agents Open Plugin Tracker

This tracker records the Open Plugin support slices for the Rust Open Agents
Slack service.

| Slice | Status | Surface | Verification |
| --- | --- | --- | --- |
| OP-04 Open Agents Plugin Service Wiring | implemented | `OPEN_AGENTS_PLUGIN_ROOTS`, optional `OPEN_AGENTS_PLUGIN_DATA_DIR`, service config validation, namespaced plugin skills, sanitized MCP planning surface | `cargo test -p open-agents-service from_reader_loads_open_plugin_fixture_components`; `cargo test -p open-agents-service local_runtime_exposes_open_plugin_components_without_starting_mcp`; `cargo test -p open-agents-runtime open_agent_prepare_composes_prompt_context_model_and_tools`; `scripts/open-agents-local-e2e.sh --check-config` |

## Current Contract

- Plugin roots use the platform path-list separator and must contain
  `.plugin/plugin.json`.
- Supported core components are skills and MCP server configs.
- Plugin skills are surfaced as `{plugin-name}:{skill-name}`.
- MCP configs are loaded and placeholder-expanded for `${PLUGIN_ROOT}` and,
  when configured, `${PLUGIN_DATA}`. The runtime receives a redaction-safe
  planning surface; executable MCP subprocess adapters are still pending.
- The checked-in fixture lives at
  `crates/open-agents-service/fixtures/open-plugin/minimal`.
