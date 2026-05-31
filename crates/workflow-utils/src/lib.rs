//! Utility crate for the standalone Vercel Workflow SDK Rust port.
//!
//! This crate maps to upstream `packages/utils`.

#![forbid(unsafe_code)]

/// Parsed components of an upstream workflow machine-readable name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedName {
    /// Human-friendly function name used by observability tools.
    pub short_name: String,
    /// Module specifier or relative path embedded in the workflow name.
    pub module_specifier: String,
    /// Full function path, including nested function segments.
    pub function_name: String,
}

/// Upstream repository used for this crate boundary.
pub const UPSTREAM_REPOSITORY: &str = "github.com/vercel/workflow";

/// Upstream ref fetched into the local source mirror.
pub const UPSTREAM_REF: &str = "main";

/// Remote HEAD verified for the initial crate skeleton.
pub const UPSTREAM_HEAD: &str = "1ee63b870afbf9754eb1022b1bb5f02d0ab042f9";

/// Upstream package owned by this crate.
pub const UPSTREAM_PACKAGE: &str = "@workflow/utils";

/// Upstream package version inventoried for this skeleton.
pub const UPSTREAM_VERSION: &str = "5.0.0-beta.3";

/// Rust crate version compiled into the library.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

fn parse_name(tag: &str, name: &str) -> Option<ParsedName> {
    let mut parts = name.split("//");
    let prefix = parts.next()?;
    let module_specifier = parts.next()?;
    let function_parts = parts.collect::<Vec<_>>();

    if prefix != tag || module_specifier.is_empty() || function_parts.is_empty() {
        return None;
    }

    let function_name = function_parts.join("//");
    let mut short_name = function_name
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_string();
    let module_short_name = module_short_name(module_specifier);

    if matches!(short_name.as_str(), "default" | "__default") && !module_short_name.is_empty() {
        short_name = module_short_name;
    }

    Some(ParsedName {
        short_name,
        module_specifier: module_specifier.to_string(),
        function_name,
    })
}

fn module_short_name(module_specifier: &str) -> String {
    if module_specifier.starts_with("./") {
        return module_specifier
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_string();
    }

    let split = module_specifier.split('@').collect::<Vec<_>>();
    let without_version = if split.len() > 1 {
        split[..split.len() - 1].join("@")
    } else {
        String::new()
    };
    let package_name = if without_version.is_empty() {
        split.first().copied().unwrap_or_default().to_string()
    } else {
        without_version
    };

    package_name
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_string()
}

/// Parses an upstream workflow id.
pub fn parse_workflow_name(name: &str) -> Option<ParsedName> {
    parse_name("workflow", name)
}

/// Parses an upstream step id.
pub fn parse_step_name(name: &str) -> Option<ParsedName> {
    parse_name("step", name)
}

/// Parses an upstream serialized class id.
pub fn parse_class_name(name: &str) -> Option<ParsedName> {
    parse_name("class", name)
}

/// Formats a step name for logs and observability surfaces.
pub fn format_step_name(name: &str) -> String {
    format_parsed_name(parse_step_name(name), name)
}

/// Formats a workflow name for logs and observability surfaces.
pub fn format_workflow_name(name: &str) -> String {
    format_parsed_name(parse_workflow_name(name), name)
}

fn format_parsed_name(parsed: Option<ParsedName>, fallback: &str) -> String {
    match parsed {
        Some(parsed) => format!("{} ({})", parsed.short_name, parsed.module_specifier),
        None => fallback.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_step_name_extracts_short_name() {
        let parsed = parse_step_name("step//./src/workflows/pulse//queryKBStep").unwrap();

        assert_eq!(parsed.short_name, "queryKBStep");
        assert_eq!(parsed.module_specifier, "./src/workflows/pulse");
        assert_eq!(parsed.function_name, "queryKBStep");
    }

    #[test]
    fn parse_default_export_uses_module_short_name() {
        let parsed = parse_workflow_name("workflow//point@0.0.1//default").unwrap();

        assert_eq!(parsed.short_name, "point");
    }
}
