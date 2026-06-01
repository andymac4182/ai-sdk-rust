# Open Plugin Spec Tracker

This tracker gates Rust Open Agents support for Open Plugin Specification
v1.0.0. OP-01 owns the reusable manifest parser in
`open-agents-core::plugin`; OP-02 and OP-03 have since landed skill discovery
and deterministic MCP discovery/runtime-config expansion on top of that spec
surface. Runtime process startup and service-level host conformance remain
explicit OP-04 work.

## Source Snapshot

| Field | Value |
| --- | --- |
| Upstream repo | `vercel-labs/open-plugin-spec` |
| Spec version | `1.0.0` |
| Inventory command | `npx opensrc fetch https://github.com/vercel-labs/open-plugin-spec` |
| Local source path | `/Users/andrewmcclenaghan/.opensrc/repos/github.com/vercel-labs/open-plugin-spec/main` |
| Source file | `README.md` |
| Remote HEAD verification | `git ls-remote https://github.com/vercel-labs/open-plugin-spec HEAD` |
| Upstream commit | `cd5f34e7f1b9398267843d2e32f38e57a58597c2` |
| OP-01 Rust owner | `crates/open-agents-core/src/plugin.rs` |
| MCP Rust owner | `crates/open-agents-core/src/open_plugin.rs` |
| CI gate | `node scripts/open-plugin-spec-gate.mjs --check` |

## Status Rules

Rows marked `implemented` must map to named Rust tests that exist in the
workspace. Rows marked `deferred-op-04` are not implemented and must keep the
future owner visible. Optional extended component types are ignored unless a
later host slice intentionally supports them.

## Conformance Checklist

| ID | Spec area | Status | Rust owner | Rust tests | Handoff |
| --- | --- | --- | --- | --- | --- |
| OP-01-001 | Sections 1-3: v1.0.0 source identity, terminology, conformance floor | implemented | `open-agents-core::plugin` | `open_plugin_spec_snapshot_records_verified_remote_head` | n/a |
| OP-01-002 | Section 4.1: plugin root plus required `.plugin/plugin.json` manifest | implemented | `open-agents-core::plugin` | `open_plugin_missing_manifest_is_fatal` | n/a |
| OP-01-003 | Section 4.1: relative paths start with `./`, reject `../`, stay inside plugin root | implemented | `open-agents-core::plugin`; `open-agents-core::open_plugin` | `open_plugin_path_safety_rejects_missing_prefix_traversal_and_mcp_directories`; `open_plugin_mcp_rejects_invalid_paths_and_keeps_valid_sources` | n/a |
| OP-01-004 | Sections 5.1-5.2: vendor-neutral manifest plus optional vendor manifest precedence diagnostics | implemented | `open-agents-core::plugin` | `open_plugin_vendor_manifest_inconsistency_warns_and_vendor_wins`; `open_plugin_invalid_vendor_manifest_location_is_ignored` | n/a |
| OP-01-005 | Sections 6.1-6.3: manifest object, required `name`, metadata fields | implemented | `open-agents-core::plugin` | `open_plugin_manifest_loads_metadata_and_core_component_paths`; `open_plugin_invalid_metadata_fields_warn_and_load` | n/a |
| OP-01-006 | Section 6.4-6.6: `skills` path field string, array, and `{ paths }` shapes | implemented | `open-agents-core::plugin`; `ai-sdk-rust::skills` | `open_plugin_manifest_loads_metadata_and_core_component_paths`; `open_plugin_manifest_accepts_string_array_component_paths`; `open_plugin_manifest_skill_paths_override_default`; `open_plugin_manifest_can_explicitly_retain_default_skills` | n/a |
| OP-01-007 | Section 6.4-6.6 and 8.2: `mcpServers` path config vs inline object disambiguation | implemented | `open-agents-core::plugin`; `open-agents-core::open_plugin` | `open_plugin_manifest_supports_inline_mcp_servers`; `open_plugin_invalid_mcp_object_is_diagnostic_and_non_fatal`; `open_plugin_ambiguous_mcp_object_is_diagnostic_and_ignored`; `open_plugin_mcp_inline_config_uses_manifest_servers`; `open_plugin_mcp_invalid_manifest_object_shape_is_non_fatal` | n/a |
| OP-01-008 | Section 6.7: plugin name length, character set, alphanumeric edges, and repetition rules | implemented | `open-agents-core::plugin` | `open_plugin_name_validation_enforces_spec_constraints` | n/a |
| OP-01-009 | Section 7.1-7.2: default skill and MCP discovery locations | implemented | `ai-sdk-rust::skills`; `open-agents-core::open_plugin` | `open_plugin_default_discovery_namespaces_skills`; `open_plugin_mcp_loads_default_mcp_json_when_manifest_field_absent`; `open_plugin_mcp_manifest_path_override_skips_default_config` | n/a |
| OP-01-010 | Section 8.1: Agent Skills `SKILL.md` discovery and loading | implemented | `ai-sdk-rust::skills` | `open_plugin_default_discovery_namespaces_skills`; `open_plugin_namespaced_slash_invocation_loads_plugin_skill_directory`; `open_plugin_skills_are_additive_to_existing_discovery` | n/a |
| OP-01-011 | Section 8.2: MCP config file, inline config, conflict diagnostics, and non-fatal config failures | implemented | `open-agents-core::open_plugin` | `open_plugin_mcp_inline_config_uses_manifest_servers`; `open_plugin_mcp_duplicate_server_names_emit_conflict_and_first_source_wins`; `open_plugin_mcp_invalid_config_shape_does_not_block_other_sources` | n/a |
| OP-01-012 | Section 9: component and MCP tool namespacing identifiers | implemented | `open-agents-core::plugin`; `open-agents-core::open_plugin`; `ai-sdk-rust::skills` | `open_plugin_namespacing_identifiers_match_recommended_format`; `open_plugin_mcp_namespaced_tool_id_matches_spec_format`; `open_plugin_duplicate_names_are_first_wins_after_namespacing` | n/a |
| OP-01-013 | Section 10: `PLUGIN_ROOT` and `${PLUGIN_ROOT}` MCP runtime expansion | implemented | `open-agents-core::open_plugin` | `open_plugin_mcp_expands_plugin_root_and_data_placeholders` | n/a |
| OP-01-014 | Section 11: plugin version metadata retained without SemVer overclaim | implemented | `open-agents-core::plugin` | `open_plugin_manifest_loads_metadata_and_core_component_paths` | n/a |
| OP-01-015 | Section 12.1-12.3: service-level minimum host conformance and partial support reporting | deferred-op-04 | `open-agents-service` | n/a | OP-04 must close host-level conformance once at least one core component type is loaded end to end. |
| OP-01-016 | Section 12.3: unsupported extended component types ignored with diagnostics | implemented | `open-agents-core::plugin` | `open_plugin_unsupported_component_types_are_diagnostic_not_fatal` | n/a |

## Diagnostics Matrix

| Decision point | Status | Rust owner | Rust tests | Handoff |
| --- | --- | --- | --- | --- |
| Inconsistent manifest content | implemented | `open-agents-core::plugin` | `open_plugin_vendor_manifest_inconsistency_warns_and_vendor_wins` | n/a |
| Invalid ambiguous object field | implemented | `open-agents-core::plugin`; `open-agents-core::open_plugin` | `open_plugin_invalid_mcp_object_is_diagnostic_and_non_fatal`; `open_plugin_ambiguous_mcp_object_is_diagnostic_and_ignored`; `open_plugin_mcp_invalid_manifest_object_shape_is_non_fatal` | n/a |
| MCP server startup failure | deferred-op-04 | `open-agents-service` | n/a | OP-04 must emit startup-failure diagnostics when real MCP process or network startup exists. |
| MCP server name conflict | implemented | `open-agents-core::open_plugin` | `open_plugin_mcp_duplicate_server_names_emit_conflict_and_first_source_wins` | n/a |
| Unsupported component type | implemented | `open-agents-core::plugin` | `open_plugin_unsupported_component_types_are_diagnostic_not_fatal` | n/a |
| Partial host support | deferred-op-04 | `open-agents-service` | n/a | OP-04 must report supported and unsupported core components once service-level plugin loading exists. |

## OP-02 Skill Discovery

OP-02 is verified in `ai-sdk-rust::skills`. It loads `.plugin/plugin.json`,
scans default `skills/` only when no manifest `skills` field is present,
respects manifest-declared skill paths, ignores missing skill directories, and
keeps non-fatal diagnostics for invalid manifests or rejected paths. Namespaced
plugin skills surface as `{plugin-name}:{skill-name}` and `/plugin:skill`.

## OP-03 MCP Integration

OP-03 is verified in `open-agents-core::open_plugin`. It loads Open Plugin
package manifests, discovers MCP server declarations, expands Open Plugin
placeholders in runtime fields, returns deterministic Open Agents-facing config,
and emits non-fatal diagnostics. It does not start stdio/http/sse MCP
transports; OP-04 must map the returned config to live runtime launch behavior.

Open Agents follows the Open Plugin v1 recommended model-compatible identifier
format exactly:

```text
mcp__plugin_{plugin-name}_{server-name}__{tool-name}
```

## Deferred Rows

| Future row | Scope | Required proof |
| --- | --- | --- |
| OP-04 | Host conformance closure | Service-level plugin load path proving at least one core component type works end to end with partial-support diagnostics and non-fatal MCP startup failure handling. |
