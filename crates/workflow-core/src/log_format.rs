use std::collections::{BTreeMap, BTreeSet};

pub type LogMetadata = BTreeMap<String, serde_json::Value>;

const MAX_VISIBLE_FRAMES: usize = 6;

/// Structured-log composition for warning and error output.
#[must_use]
pub fn compose_log_line(prefix: &str, message: &str, metadata: Option<&LogMetadata>) -> String {
    let mut pieces = message.split('\n');
    let framing = pieces.next().unwrap_or_default();
    let body = pieces.collect::<Vec<_>>().join("\n");
    let fields = render_structured_fields(framing, metadata);
    let trimmed_body = trim_stack_body(&body);

    let mut lines = vec![format!("{prefix} {framing}")];
    if let Some(fields) = fields {
        lines.push(fields);
    }
    if let Some(trimmed_body) = trimmed_body {
        lines.push(trimmed_body);
    }
    lines.join("\n")
}

fn render_structured_fields(_framing: &str, metadata: Option<&LogMetadata>) -> Option<String> {
    let metadata = metadata?;
    if metadata.is_empty() {
        return None;
    }

    let well_known = BTreeSet::from([
        "workflowRunId",
        "workflowName",
        "stepId",
        "stepName",
        "errorAttribution",
        "errorCode",
        "errorName",
        "errorMessage",
        "errorStack",
        "hint",
        "attempt",
        "retryCount",
    ]);
    let mut lines = Vec::new();

    let error_name = pick_string(metadata, "errorName");
    let attribution = pick_string(metadata, "errorAttribution");
    if error_name.is_some() || attribution.is_some() {
        let badge = attribution.map_or("", |value| {
            if value == "sdk" {
                "sdk error"
            } else {
                "user error"
            }
        });
        let class_name = error_name.unwrap_or("");
        let sep = if !badge.is_empty() && !class_name.is_empty() {
            " \u{00b7} "
        } else {
            ""
        };
        lines.push(format!("  {badge}{sep}{class_name}"));
    }

    let run_id = pick_string(metadata, "workflowRunId");
    let workflow_name = pick_string(metadata, "workflowName");
    if run_id.is_some() || workflow_name.is_some() {
        lines.push(format_id_row(
            "run",
            run_id,
            workflow_name,
            format_workflow_name,
        ));
    }

    let step_id = pick_string(metadata, "stepId");
    let step_name = pick_string(metadata, "stepName");
    if step_id.is_some() || step_name.is_some() {
        lines.push(format_id_row("step", step_id, step_name, format_step_name));
    }

    let attempt = metadata.get("attempt").filter(|value| !value.is_null());
    let retry_count = metadata.get("retryCount").filter(|value| !value.is_null());
    if attempt.is_some() || retry_count.is_some() {
        if let (Some(attempt), Some(retry_count)) = (attempt, retry_count) {
            lines.push(format!(
                "  {} {} attempts \u{00b7} {} max retries",
                kv_key("retry"),
                format_json_scalar(attempt),
                format_json_scalar(retry_count)
            ));
        } else if let Some(attempt) = attempt {
            lines.push(format!(
                "  {} {} attempts",
                kv_key("retry"),
                format_json_scalar(attempt)
            ));
        }
    }

    let error_code = pick_string(metadata, "errorCode");
    if let Some(error_code) = error_code {
        if Some(error_code) != error_name {
            lines.push(format!("  {} {error_code}", kv_key("code")));
        }
    }

    if let Some(hint) = pick_string(metadata, "hint") {
        lines.push(format!("  hint: {hint}"));
    }

    for (key, value) in metadata {
        if well_known.contains(key.as_str()) || value.is_null() {
            continue;
        }
        lines.push(format!(
            "  {} {}",
            kv_key(key),
            format_passthrough_value(value)
        ));
    }

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn trim_stack_body(body: &str) -> Option<String> {
    if body.is_empty() {
        return None;
    }

    let mut out = Vec::new();
    let mut dropped_run = 0usize;
    let mut visible_frame_count = 0usize;
    let mut capped_frames = 0usize;

    for line in body.split('\n') {
        let is_frame = line.trim_start().starts_with("at ");
        if is_frame && is_framework_frame(line) {
            dropped_run += 1;
            continue;
        }
        if is_frame && visible_frame_count >= MAX_VISIBLE_FRAMES {
            capped_frames += 1;
            continue;
        }
        flush_dropped(&mut out, &mut dropped_run);
        out.push(line.to_string());
        if is_frame {
            visible_frame_count += 1;
        }
    }

    flush_dropped(&mut out, &mut dropped_run);
    if capped_frames > 0 {
        out.push(format!(
            "        \u{2026} {capped_frames} more {} (run `pnpm wf inspect run <id>` for the full stack)",
            if capped_frames == 1 { "frame" } else { "frames" }
        ));
    }

    Some(out.join("\n"))
}

fn flush_dropped(out: &mut Vec<String>, dropped_run: &mut usize) {
    if *dropped_run > 0 {
        out.push(format!(
            "        \u{2026} {} more {} in framework internals",
            *dropped_run,
            if *dropped_run == 1 { "frame" } else { "frames" }
        ));
        *dropped_run = 0;
    }
}

fn is_framework_frame(line: &str) -> bool {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("at ") {
        return false;
    }
    trimmed.contains("node:internal/")
        || trimmed.contains("node_modules/.pnpm/")
        || trimmed.contains("node_modules__pnpm_")
        || trimmed.contains("_next_dist_")
        || trimmed.contains("node_modules/next/")
        || trimmed.contains("node_modules/@opentelemetry/")
        || trimmed.contains("node_modules/vitest/")
        || trimmed.contains("node_modules/@vitest/")
}

fn pick_string<'a>(metadata: &'a LogMetadata, key: &str) -> Option<&'a str> {
    metadata
        .get(key)?
        .as_str()
        .filter(|value| !value.is_empty())
}

fn kv_key(key: &str) -> String {
    format!("{key:<6}")
}

fn format_id_row(
    label: &str,
    id: Option<&str>,
    name: Option<&str>,
    format_name: fn(&str) -> Option<String>,
) -> String {
    let id_cell = id.unwrap_or("\u{2014}");
    let name_cell = name
        .and_then(format_name)
        .map(|name| format!(" \u{00b7} {name}"))
        .unwrap_or_default();
    format!("  {} {id_cell}{name_cell}", kv_key(label))
}

fn format_passthrough_value(value: &serde_json::Value) -> String {
    if let Some(value) = value.as_str() {
        if value.contains('\n') {
            return value
                .split('\n')
                .enumerate()
                .map(|(index, line)| {
                    if index == 0 {
                        line.to_string()
                    } else {
                        format!("         {line}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
        }
        return value.to_string();
    }

    format_json_scalar(value)
}

fn format_json_scalar(value: &serde_json::Value) -> String {
    if let Some(value) = value.as_i64() {
        return value.to_string();
    }
    if let Some(value) = value.as_u64() {
        return value.to_string();
    }
    if let Some(value) = value.as_f64() {
        return value.to_string();
    }
    if let Some(value) = value.as_bool() {
        return value.to_string();
    }
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}

fn format_workflow_name(name: &str) -> Option<String> {
    format_parsed_name(parse_name("workflow", name))
}

fn format_step_name(name: &str) -> Option<String> {
    format_parsed_name(parse_name("step", name))
}

struct ParsedName {
    short_name: String,
    module_specifier: String,
}

fn parse_name(tag: &str, name: &str) -> Option<ParsedName> {
    let mut parts = name.split("//");
    let prefix = parts.next()?;
    let module_specifier = parts.next()?;
    let function_name = parts.collect::<Vec<_>>().join("//");

    if prefix != tag || module_specifier.is_empty() || function_name.is_empty() {
        return None;
    }

    let mut short_name = function_name
        .split('/')
        .next_back()
        .unwrap_or_default()
        .to_string();
    let module_short_name = if module_specifier.starts_with("./") {
        module_specifier
            .split('/')
            .next_back()
            .unwrap_or_default()
            .to_string()
    } else {
        let without_version = module_specifier.rsplit_once('@').map_or_else(
            || module_specifier.to_string(),
            |(name, _)| name.to_string(),
        );
        without_version
            .split('/')
            .next_back()
            .unwrap_or_default()
            .to_string()
    };

    if matches!(short_name.as_str(), "default" | "__default") && !module_short_name.is_empty() {
        short_name = module_short_name;
    }

    Some(ParsedName {
        short_name,
        module_specifier: module_specifier.to_string(),
    })
}

fn format_parsed_name(parsed: Option<ParsedName>) -> Option<String> {
    parsed.map(|parsed| format!("{} ({})", parsed.short_name, parsed.module_specifier))
}
