//! Portable builder helpers for the standalone Vercel Workflow SDK port.
//!
//! This crate maps to upstream `packages/builders`. It intentionally ports
//! helper behavior that is useful outside JavaScript bundler hosts. Esbuild
//! plugin execution, Node package resolution hooks, and framework build
//! plugins remain documented as JavaScript-only host tooling.

#![forbid(unsafe_code)]

use regex::Regex;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Component, Path, PathBuf},
    sync::OnceLock,
};

/// Upstream repository used for this crate boundary.
pub const UPSTREAM_REPOSITORY: &str = "github.com/vercel/workflow";

/// Upstream ref fetched into the local source mirror.
pub const UPSTREAM_REF: &str = "main";

/// Upstream package owned by this crate.
pub const UPSTREAM_PACKAGE: &str = "@workflow/builders";

/// Upstream package version inventoried for this crate.
pub const UPSTREAM_VERSION: &str = "5.0.0-beta.10";

/// Rust crate version compiled into the library.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

const SOURCE_EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];
const PSEUDO_PACKAGE_NAMES: &[&str] = &[
    "server-only",
    "client-only",
    "next/dist/compiled/server-only",
    "next/dist/compiled/client-only",
];

/// Build targets relevant to portable diagnostics path resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildTarget {
    Standalone,
    VercelBuildOutputApi,
    Other(String),
}

/// Sourcemap mode accepted by `WORKFLOW_SOURCEMAP` and builder config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourcemapMode {
    Disabled,
    Enabled,
    Inline,
    Linked,
    External,
    Both,
}

/// Result of resolving an import path for a bundle virtual entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportPathResult {
    pub import_path: String,
    pub is_package: bool,
}

/// Result of resolving a versioned module specifier for workflow IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleSpecifierResult {
    pub module_specifier: Option<String>,
}

/// Workflow pattern detection used by discovery pre-scans.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorkflowPatterns {
    pub has_use_workflow: bool,
    pub has_use_step: bool,
    pub has_serde_import: bool,
    pub has_serde_symbol: bool,
    pub has_serde_computed_property: bool,
    pub has_directive: bool,
    pub has_serde: bool,
}

/// Location preview for a package violation in source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViolationLocation {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub line_text: String,
    pub length: usize,
}

#[derive(Debug, Clone)]
struct PackageInfo {
    name: String,
    version: String,
    dir: PathBuf,
    exports: Option<Value>,
    main: Option<String>,
    module: Option<String>,
}

/// Returns the pseudo-packages that the JS builder replaces with empty modules.
pub fn pseudo_packages() -> &'static [&'static str] {
    PSEUDO_PACKAGE_NAMES
}

/// Resolve the diagnostics manifest path for portable builder configs.
pub fn diagnostics_manifest_path(
    working_dir: impl AsRef<Path>,
    build_target: &BuildTarget,
    diagnostics_dir: Option<&str>,
) -> Option<PathBuf> {
    let working_dir = working_dir.as_ref();
    if let Some(diagnostics_dir) = diagnostics_dir {
        return Some(
            working_dir
                .join(diagnostics_dir)
                .join("workflows-manifest.json"),
        );
    }
    if matches!(build_target, BuildTarget::VercelBuildOutputApi) {
        Some(
            working_dir
                .join(".vercel/output/diagnostics")
                .join("workflows-manifest.json"),
        )
    } else {
        None
    }
}

/// Parse the `WORKFLOW_SOURCEMAP` environment value.
pub fn parse_sourcemap_env(value: Option<&str>) -> Option<SourcemapMode> {
    match value {
        None | Some("") => None,
        Some("0" | "false") => Some(SourcemapMode::Disabled),
        Some("1" | "true") => Some(SourcemapMode::Enabled),
        Some("inline") => Some(SourcemapMode::Inline),
        Some("linked") => Some(SourcemapMode::Linked),
        Some("external") => Some(SourcemapMode::External),
        Some("both") => Some(SourcemapMode::Both),
        Some(_) => None,
    }
}

/// Resolve sourcemap config precedence: explicit config, env, default.
pub fn resolve_sourcemap(
    config: Option<SourcemapMode>,
    env: Option<&str>,
    default_mode: SourcemapMode,
) -> SourcemapMode {
    config
        .or_else(|| parse_sourcemap_env(env))
        .unwrap_or(default_mode)
}

/// Returns whether sourcemaps are enabled under config/env precedence.
pub fn sourcemaps_enabled(config: Option<SourcemapMode>, env: Option<&str>) -> bool {
    resolve_sourcemap(config, env, SourcemapMode::Enabled) != SourcemapMode::Disabled
}

/// Discover input source files in configured directories, including dot files
/// while excluding explicit build/dependency directories.
pub fn get_input_files(working_dir: impl AsRef<Path>, dirs: &[&str]) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for dir in dirs {
        collect_input_files(&working_dir.as_ref().join(dir), &mut files)?;
    }
    files.sort();
    Ok(files)
}

fn collect_input_files(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if is_ignored_path(&path) {
            continue;
        }
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_input_files(&path, files)?;
        } else if file_type.is_file() && has_source_extension(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn has_source_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| SOURCE_EXTENSIONS.contains(&extension))
}

fn is_ignored_path(path: &Path) -> bool {
    let mut previous = "";
    for component in path.components() {
        let Component::Normal(segment) = component else {
            continue;
        };
        let segment = segment.to_string_lossy();
        if matches!(
            segment.as_ref(),
            "node_modules"
                | ".git"
                | ".next"
                | ".nitro"
                | ".nuxt"
                | ".output"
                | ".vercel"
                | ".workflow-data"
                | ".workflow-vitest"
                | ".svelte-kit"
                | ".turbo"
                | ".cache"
                | ".yarn"
                | ".pnpm-store"
        ) {
            return true;
        }
        if previous == ".well-known" && segment == "workflow" {
            return true;
        }
        previous = "";
        if segment == ".well-known" {
            previous = ".well-known";
        }
    }
    false
}

/// Resolve a versioned module specifier for a file.
pub fn resolve_module_specifier(
    file_path: impl AsRef<Path>,
    project_root: impl AsRef<Path>,
) -> ModuleSpecifierResult {
    let file_path = file_path.as_ref();
    let project_root = project_root.as_ref();
    let in_node_modules = is_in_node_modules(file_path);
    let in_workspace = !in_node_modules && is_workspace_package(file_path, project_root);
    if !in_node_modules && !in_workspace {
        return ModuleSpecifierResult {
            module_specifier: None,
        };
    }

    let Some(package) = find_package_json(file_path) else {
        return ModuleSpecifierResult {
            module_specifier: None,
        };
    };
    let subpath = resolve_export_subpath(file_path, &package, true);
    ModuleSpecifierResult {
        module_specifier: Some(if subpath.is_empty() {
            format!("{}@{}", package.name, package.version)
        } else {
            format!("{}{}@{}", package.name, subpath, package.version)
        }),
    }
}

/// Resolve the import path to use for a file in a bundle virtual entry.
pub fn get_import_path(
    file_path: impl AsRef<Path>,
    project_root: impl AsRef<Path>,
) -> ImportPathResult {
    let file_path = file_path.as_ref();
    let project_root = project_root.as_ref();
    let in_node_modules = is_in_node_modules(file_path);
    let in_workspace = !in_node_modules && is_workspace_package(file_path, project_root);

    if in_node_modules || in_workspace {
        if let Some(package) = find_package_json(file_path) {
            let can_use_package_specifier =
                in_workspace || project_dependencies(project_root).contains(&package.name);
            if !can_use_package_specifier {
                return ImportPathResult {
                    import_path: relative_import_path(file_path, project_root),
                    is_package: false,
                };
            }

            let subpath = resolve_export_subpath(file_path, &package, false);
            if !subpath.is_empty() {
                return ImportPathResult {
                    import_path: format!("{}{}", package.name, subpath),
                    is_package: true,
                };
            }

            if !is_root_entrypoint_file(file_path, &package) {
                return ImportPathResult {
                    import_path: relative_import_path(file_path, project_root),
                    is_package: false,
                };
            }

            return ImportPathResult {
                import_path: package.name,
                is_package: true,
            };
        }
    }

    ImportPathResult {
        import_path: relative_import_path(file_path, project_root),
        is_package: false,
    }
}

/// Resolve workflow alias paths such as `workflows/foo.ts` and `app/*`.
pub fn resolve_workflow_alias_relative_path(
    absolute_file_path: impl AsRef<Path>,
    working_dir: impl AsRef<Path>,
) -> Option<String> {
    let absolute_file_path = absolute_file_path.as_ref();
    let working_dir = working_dir.as_ref();
    let resolved_file_path = fs::canonicalize(absolute_file_path).ok()?;
    let normalized_absolute = normalize_path(absolute_file_path);
    let normalized_resolved = normalize_path(resolved_file_path);
    for alias_relative_path in alias_relative_path_candidates(&normalized_absolute) {
        let candidate_path = working_dir.join(&alias_relative_path);
        let resolved_candidate = fs::canonicalize(candidate_path).ok()?;
        if normalize_path(resolved_candidate) == normalized_resolved {
            return Some(alias_relative_path);
        }
    }
    None
}

fn alias_relative_path_candidates(normalized_absolute_path: &str) -> Vec<String> {
    let mut candidates = BTreeSet::new();
    for alias_root in [
        "src/workflows",
        "workflows",
        "src/app",
        "app",
        "src/pages",
        "pages",
    ] {
        let marker = format!("/{alias_root}/");
        if let Some(index) = normalized_absolute_path.rfind(&marker) {
            candidates.insert(normalized_absolute_path[index + 1..].to_string());
        }
    }
    candidates.into_iter().collect()
}

/// Detect directive and serde patterns with the same regexp-level behavior as
/// the upstream builder pre-scan.
pub fn detect_workflow_patterns(source: &str) -> WorkflowPatterns {
    let has_use_workflow = use_workflow_pattern(source);
    let has_use_step = use_step_pattern(source);
    let has_serde_import = workflow_serde_import_pattern(source);
    let has_serde_symbol = workflow_serde_symbol_pattern(source);
    let has_serde_computed_property = workflow_serde_computed_property_pattern(source);
    let has_directive = has_use_workflow || has_use_step;
    let has_serde = has_serde_import || has_serde_symbol || has_serde_computed_property;
    WorkflowPatterns {
        has_use_workflow,
        has_use_step,
        has_serde_import,
        has_serde_symbol,
        has_serde_computed_property,
        has_directive,
        has_serde,
    }
}

pub fn use_workflow_pattern(source: &str) -> bool {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN
        .get_or_init(|| Regex::new(r#"(?m)^\s*['"]use workflow['"];?"#).expect("valid regex"))
        .is_match(source)
}

pub fn use_step_pattern(source: &str) -> bool {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN
        .get_or_init(|| Regex::new(r#"(?m)^\s*['"]use step['"];?"#).expect("valid regex"))
        .is_match(source)
}

pub fn workflow_serde_import_pattern(source: &str) -> bool {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN
        .get_or_init(|| {
            Regex::new(r#"(?s)import\s+(?:type\s+)?[^;]*?\s+from\s+['"]@workflow/serde['"]"#)
                .expect("valid regex")
        })
        .is_match(source)
}

pub fn workflow_serde_symbol_pattern(source: &str) -> bool {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN
        .get_or_init(|| {
            Regex::new(r#"Symbol\.for\(\s*['"]workflow-(?:serialize|deserialize)['"]\s*\)"#)
                .expect("valid regex")
        })
        .is_match(source)
}

pub fn workflow_serde_computed_property_pattern(source: &str) -> bool {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN
        .get_or_init(|| {
            Regex::new(r#"\[\s*WORKFLOW_(?:SERIALIZE|DESERIALIZE)\s*\]"#).expect("valid regex")
        })
        .is_match(source)
}

/// Returns whether a file should be sent through the SWC transform.
pub fn should_transform_file(file_path: &str, patterns: WorkflowPatterns) -> bool {
    let normalized = file_path.replace('\\', "/");
    if normalized.contains("/.well-known/workflow/") {
        return false;
    }
    patterns.has_directive || patterns.has_serde
}

/// Extract a top-level package name from a `node_modules` file path.
pub fn get_package_name(file_path: &str) -> Option<String> {
    let normalized = file_path.replace('\\', "/");
    let marker = "/node_modules/";
    let index = normalized.rfind(marker)?;
    let mut parts = normalized[index + marker.len()..].split('/');
    let first = parts.next()?;
    if first.is_empty() || first == ".pnpm" {
        return None;
    }
    if first.starts_with('@') {
        let second = parts.next()?;
        Some(format!("{first}/{second}"))
    } else {
        Some(first.to_string())
    }
}

/// Escape a string for literal use in a regular expression.
pub fn escape_reg_exp(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(
            ch,
            '.' | '*' | '+' | '?' | '^' | '$' | '{' | '}' | '(' | ')' | '|' | '[' | ']' | '\\'
        ) {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

/// Extract the identifier used by an import specifier.
pub fn get_imported_identifier(specifier: &str) -> Option<String> {
    let specifier = specifier.trim();
    if specifier.is_empty() || specifier == "*" {
        return None;
    }

    if let (Some(open), Some(close)) = (specifier.find('{'), specifier.find('}')) {
        if close > open {
            let first_named = specifier[open + 1..close].split(',').next()?.trim();
            if first_named.is_empty() {
                return None;
            }
            let alias_parts: Vec<&str> = first_named.split_whitespace().collect();
            if let Some(as_index) = alias_parts.iter().position(|part| *part == "as") {
                return alias_parts
                    .get(as_index + 1)
                    .map(|value| (*value).to_string());
            }
            return alias_parts.last().map(|value| (*value).to_string());
        }
    }

    if let Some(rest) = specifier.strip_prefix('*') {
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.first().copied() == Some("as") {
            return parts.get(1).map(|value| (*value).to_string());
        }
        return None;
    }

    specifier
        .split(',')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Find the first runtime usage of an imported package identifier in a file.
pub fn get_violation_location(
    cwd: impl AsRef<Path>,
    file: &str,
    package_name: &str,
) -> Option<ViolationLocation> {
    let path = cwd.as_ref().join(file);
    let contents = fs::read_to_string(path).ok()?;
    let identifier = imported_identifier_for_package(&contents, package_name)?;
    let mut in_block_comment = false;
    for (line_index, line) in contents.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("import ") {
            continue;
        }
        let visible = strip_comments_and_strings(line, &mut in_block_comment);
        if let Some(column) = find_identifier(&visible, &identifier) {
            return Some(ViolationLocation {
                file: file.to_string(),
                line: line_index + 1,
                column,
                line_text: line.to_string(),
                length: identifier.len(),
            });
        }
    }
    None
}

fn imported_identifier_for_package(contents: &str, package_name: &str) -> Option<String> {
    for line in contents.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("import ") || !trimmed.contains(package_name) {
            continue;
        }
        let before_from = trimmed.split(" from ").next()?;
        let specifier = before_from.strip_prefix("import ")?.trim();
        if specifier.starts_with('"') || specifier.starts_with('\'') {
            return Some(package_name.to_string());
        }
        if let Some(identifier) = get_imported_identifier(specifier) {
            return Some(identifier);
        }
    }
    None
}

fn strip_comments_and_strings(line: &str, in_block_comment: &mut bool) -> String {
    let mut result = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(ch) = chars.next() {
        if *in_block_comment {
            if ch == '*' && chars.peek() == Some(&'/') {
                chars.next();
                *in_block_comment = false;
            }
            result.push(' ');
            continue;
        }
        if let Some(current_quote) = quote {
            if ch == '\\' {
                result.push(' ');
                if chars.next().is_some() {
                    result.push(' ');
                }
                continue;
            }
            if ch == current_quote {
                quote = None;
            }
            result.push(' ');
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'/') {
            result.extend(std::iter::repeat_n(
                ' ',
                line.len().saturating_sub(result.len()),
            ));
            break;
        }
        if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            *in_block_comment = true;
            result.push(' ');
            result.push(' ');
            continue;
        }
        if matches!(ch, '"' | '\'' | '`') {
            quote = Some(ch);
            result.push(' ');
            continue;
        }
        result.push(ch);
    }
    result
}

fn find_identifier(line: &str, identifier: &str) -> Option<usize> {
    for (index, _) in line.match_indices(identifier) {
        let before = line[..index].chars().next_back();
        let after = line[index + identifier.len()..].chars().next();
        if !before.is_some_and(is_identifier_char) && !after.is_some_and(is_identifier_char) {
            return Some(index);
        }
    }
    None
}

fn is_identifier_char(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_ascii_alphanumeric()
}

fn find_package_json(file_path: &Path) -> Option<PackageInfo> {
    let mut dir = file_path.parent()?;
    loop {
        let package_json = dir.join("package.json");
        if let Ok(content) = fs::read_to_string(package_json)
            && let Ok(parsed) = serde_json::from_str::<Value>(&content)
        {
            let name = parsed.get("name")?.as_str()?.to_string();
            let version = parsed
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or("0.0.0")
                .to_string();
            return Some(PackageInfo {
                name,
                version,
                dir: dir.to_path_buf(),
                exports: parsed.get("exports").cloned(),
                main: parsed
                    .get("main")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                module: parsed
                    .get("module")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            });
        }
        let Some(parent) = dir.parent() else {
            break;
        };
        if parent == dir {
            break;
        }
        dir = parent;
    }
    None
}

fn to_package_relative_path(file_path: &Path, package: &PackageInfo) -> Option<String> {
    let normalized_file = normalize_path(file_path);
    let normalized_dir = normalize_path(&package.dir);
    normalized_file
        .strip_prefix(&(normalized_dir + "/"))
        .map(str::to_string)
}

fn resolve_export_subpath(
    file_path: &Path,
    package: &PackageInfo,
    allow_source_fallback: bool,
) -> String {
    let Some(exports) = &package.exports else {
        return String::new();
    };
    let Some(relative_path) = to_package_relative_path(file_path, package) else {
        return String::new();
    };
    let mut comparable_targets = BTreeSet::from([format!("./{relative_path}")]);
    if allow_source_fallback {
        for fallback in source_fallback_export_targets(&relative_path) {
            comparable_targets.insert(format!("./{fallback}"));
        }
    }
    let Value::Object(exports_object) = exports else {
        return String::new();
    };
    for (subpath, target) in exports_object {
        if let Some(resolved_target) = resolve_export_target(target) {
            let normalized_target = normalize_export_path(&resolved_target);
            if comparable_targets.contains(&normalized_target) {
                return if subpath == "." {
                    String::new()
                } else {
                    subpath.strip_prefix('.').unwrap_or(subpath).to_string()
                };
            }
        }
    }
    String::new()
}

fn source_fallback_export_targets(relative_path: &str) -> Vec<String> {
    let Some((path_without_extension, extension)) = split_extension(relative_path) else {
        return Vec::new();
    };
    let extension_targets = match extension {
        ".ts" | ".tsx" | ".jsx" => &[".js"][..],
        ".mts" => &[".mjs"][..],
        ".cts" => &[".cjs"][..],
        _ => &[][..],
    };
    let mut targets = BTreeSet::new();
    for fallback_extension in extension_targets {
        targets.insert(format!("{path_without_extension}{fallback_extension}"));
    }
    if let Some(dist_without_extension) = relative_path
        .strip_prefix("src/")
        .and_then(|path| path.strip_suffix(extension))
    {
        for fallback_extension in extension_targets {
            targets.insert(format!("dist/{dist_without_extension}{fallback_extension}"));
        }
    }
    targets.into_iter().collect()
}

fn split_extension(path: &str) -> Option<(&str, &str)> {
    for extension in [
        ".d.ts", ".d.mts", ".d.cts", ".tsx", ".jsx", ".mts", ".cts", ".ts", ".js", ".mjs", ".cjs",
    ] {
        if let Some(path_without_extension) = path.strip_suffix(extension) {
            return Some((path_without_extension, extension));
        }
    }
    None
}

fn resolve_export_target(target: &Value) -> Option<String> {
    match target {
        Value::String(value) => Some(value.clone()),
        Value::Array(values) => values.iter().find_map(resolve_export_target),
        Value::Object(object) => {
            for condition in ["workflow", "default", "require", "import", "node"] {
                if let Some(value) = object.get(condition)
                    && let Some(resolved) = resolve_export_target(value)
                {
                    return Some(resolved);
                }
            }
            None
        }
        _ => None,
    }
}

fn normalize_export_path(path: &str) -> String {
    if path.starts_with("./") {
        path.to_string()
    } else {
        format!("./{path}")
    }
}

fn is_in_node_modules(file_path: &Path) -> bool {
    normalize_path(file_path).contains("/node_modules/")
}

fn project_dependencies(project_root: &Path) -> BTreeSet<String> {
    let mut dependencies = BTreeSet::new();
    let package_json = project_root.join("package.json");
    let Ok(content) = fs::read_to_string(package_json) else {
        return dependencies;
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&content) else {
        return dependencies;
    };
    for dep_type in [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ] {
        if let Some(Value::Object(values)) = parsed.get(dep_type) {
            dependencies.extend(values.keys().cloned());
        }
    }
    dependencies
}

fn is_workspace_package(file_path: &Path, project_root: &Path) -> bool {
    if is_in_node_modules(file_path) {
        return false;
    }
    let Some(package) = find_package_json(file_path) else {
        return false;
    };
    if normalize_path(&package.dir) == normalize_path(project_root) {
        return false;
    }
    project_dependencies(project_root).contains(&package.name)
}

fn relative_import_path(file_path: &Path, project_root: &Path) -> String {
    let normalized_file = normalize_path(file_path);
    let normalized_root = normalize_path(project_root);
    let mut relative =
        if let Some(stripped) = normalized_file.strip_prefix(&(normalized_root + "/")) {
            stripped.to_string()
        } else {
            relative_path_between(project_root, file_path)
        };
    if !relative.starts_with('.') {
        relative = format!("./{relative}");
    }
    relative
}

fn relative_path_between(from: &Path, to: &Path) -> String {
    let from_parts = path_parts(from);
    let to_parts = path_parts(to);
    let common = from_parts
        .iter()
        .zip(to_parts.iter())
        .take_while(|(left, right)| left == right)
        .count();
    let mut relative = Vec::new();
    relative.extend(std::iter::repeat_n(
        "..".to_string(),
        from_parts.len() - common,
    ));
    relative.extend(to_parts[common..].iter().cloned());
    relative.join("/")
}

fn path_parts(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().to_string()),
            Component::RootDir => Some(String::new()),
            _ => None,
        })
        .collect()
}

fn has_root_export(exports_field: &Value) -> bool {
    match exports_field {
        Value::String(_) | Value::Array(_) => true,
        Value::Object(object) => {
            (!object.is_empty() && object.keys().all(|key| !key.starts_with('.')))
                || object.contains_key(".")
        }
        _ => false,
    }
}

fn normalize_package_target_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    normalized
        .strip_prefix("./")
        .or_else(|| normalized.strip_prefix('/'))
        .unwrap_or(&normalized)
        .to_string()
}

fn is_root_entrypoint_file(file_path: &Path, package: &PackageInfo) -> bool {
    let Some(relative_file_path) = to_package_relative_path(file_path, package) else {
        return false;
    };

    if let Some(exports) = &package.exports {
        let root_target = match exports {
            Value::Object(object) if object.contains_key(".") => object.get("."),
            value if has_root_export(value) => Some(value),
            _ => None,
        };
        let Some(resolved_target) = root_target.and_then(resolve_export_target) else {
            return false;
        };
        return normalize_package_target_path(&resolved_target) == relative_file_path;
    }

    let mut root_candidates = vec![
        "index.js".to_string(),
        "index.mjs".to_string(),
        "index.cjs".to_string(),
        "index.ts".to_string(),
        "index.mts".to_string(),
        "index.cts".to_string(),
    ];
    if let Some(module) = &package.module {
        root_candidates.push(module.clone());
    }
    if let Some(main) = &package.main {
        root_candidates.push(main.clone());
    }
    root_candidates
        .iter()
        .map(|candidate| normalize_package_target_path(candidate))
        .any(|candidate| candidate == relative_file_path)
}

fn normalize_path(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::{self, File},
        io::Write,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(prefix: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos();
            let counter = TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("{prefix}-{nanos}-{}-{counter}", std::process::id()));
            fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }

        fn join(&self, path: &str) -> PathBuf {
            self.path.join(path)
        }

        fn write(&self, path: &str, contents: &str) -> PathBuf {
            let full_path = self.join(path);
            fs::create_dir_all(full_path.parent().expect("has parent")).expect("create parent");
            let mut file = File::create(&full_path).expect("create file");
            file.write_all(contents.as_bytes()).expect("write file");
            full_path
        }

        fn write_json(&self, path: &str, value: Value) -> PathBuf {
            self.write(
                path,
                &serde_json::to_string_pretty(&value).expect("serialize json"),
            )
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn normalized(paths: Vec<PathBuf>) -> Vec<String> {
        paths.into_iter().map(normalize_path).collect()
    }

    #[test]
    fn builders_get_input_files_discovers_files_inside_dot_prefixed_directories() {
        let root = TestDir::new("get-input-files");
        let src = root.join("src");
        root.write("src/.hidden/step.ts", "'use step';");
        root.write("src/.config/workflow.ts", "'use workflow';");
        root.write("src/regular/step.ts", "'use step';");

        let files = normalized(get_input_files(&root.path, &["src"]).expect("input files"));
        assert!(files.contains(&normalize_path(src.join(".hidden/step.ts"))));
        assert!(files.contains(&normalize_path(src.join(".config/workflow.ts"))));
        assert!(files.contains(&normalize_path(src.join("regular/step.ts"))));
    }

    #[test]
    fn builders_get_input_files_discovers_dot_prefixed_files() {
        let root = TestDir::new("get-input-files");
        let src = root.join("src");
        root.write("src/.hidden-step.ts", "'use step';");
        root.write("src/visible-step.ts", "'use step';");

        let files = normalized(get_input_files(&root.path, &["src"]).expect("input files"));
        assert!(files.contains(&normalize_path(src.join(".hidden-step.ts"))));
        assert!(files.contains(&normalize_path(src.join("visible-step.ts"))));
    }

    #[test]
    fn builders_get_input_files_still_excludes_explicitly_ignored_dot_directories() {
        let root = TestDir::new("get-input-files");
        for path in [
            "src/.git/hooks/pre-commit.ts",
            "src/.next/server/page.ts",
            "src/.nuxt/workflow/steps.mjs",
            "src/.vercel/output/step.ts",
            "src/.svelte-kit/output/step.ts",
            "src/.workflow-data/state.ts",
            "src/.workflow-vitest/workflows.mjs",
            "src/.well-known/workflow/route.ts",
            "src/.turbo/cache/build.ts",
            "src/.cache/babel/plugin.js",
            "src/.yarn/releases/yarn.cjs",
            "src/.pnpm-store/v3/files.ts",
            "src/node_modules/pkg/index.ts",
        ] {
            root.write(path, "");
        }
        let custom = root.write("src/.custom/step.ts", "'use step';");

        let files = normalized(get_input_files(&root.path, &["src"]).expect("input files"));
        assert_eq!(files, vec![normalize_path(custom)]);
    }

    #[test]
    fn builders_get_input_files_discovers_files_with_various_supported_extensions_in_dot_directories()
     {
        let root = TestDir::new("get-input-files");
        let src = root.join("src");
        for path in [
            "src/.api/route.tsx",
            "src/.api/handler.mts",
            "src/.api/utils.js",
            "src/.api/config.cjs",
        ] {
            root.write(path, "");
        }
        let files = normalized(get_input_files(&root.path, &["src"]).expect("input files"));
        for path in [
            ".api/route.tsx",
            ".api/handler.mts",
            ".api/utils.js",
            ".api/config.cjs",
        ] {
            assert!(files.contains(&normalize_path(src.join(path))));
        }
    }

    #[test]
    fn builders_get_diagnostics_manifest_path_uses_an_explicit_diagnostics_dir_when_configured() {
        let root = TestDir::new("diagnostics-path");
        assert_eq!(
            diagnostics_manifest_path(
                &root.path,
                &BuildTarget::Standalone,
                Some(".next/diagnostics")
            ),
            Some(root.join(".next/diagnostics/workflows-manifest.json"))
        );
    }

    #[test]
    fn builders_get_diagnostics_manifest_path_does_not_emit_vercel_diagnostics_for_non_vercel_builder_targets()
     {
        let root = TestDir::new("diagnostics-path");
        assert_eq!(
            diagnostics_manifest_path(&root.path, &BuildTarget::Standalone, None),
            None
        );
    }

    #[test]
    fn builders_get_diagnostics_manifest_path_falls_back_to_vercel_output_diagnostics_for_the_vercel_builder()
     {
        let root = TestDir::new("diagnostics-path");
        assert_eq!(
            diagnostics_manifest_path(&root.path, &BuildTarget::VercelBuildOutputApi, None),
            Some(root.join(".vercel/output/diagnostics/workflows-manifest.json"))
        );
    }

    #[test]
    fn builders_get_import_path_uses_package_subpath_import_when_file_matches_an_export_subpath() {
        let root = TestDir::new("module-specifier");
        let project_root = root.join("apps/chat");
        let file_path = root.join("packages/agent/src/server.ts");
        root.write_json(
            "apps/chat/package.json",
            serde_json::json!({
                "name": "chat",
                "dependencies": { "@internal/agent": "workspace:*" }
            }),
        );
        root.write_json(
            "packages/agent/package.json",
            serde_json::json!({
                "name": "@internal/agent",
                "version": "1.0.0",
                "exports": { "./server": "./src/server.ts" }
            }),
        );
        root.write("packages/agent/src/server.ts", "'use step';");
        assert_eq!(
            get_import_path(file_path, project_root),
            ImportPathResult {
                import_path: "@internal/agent/server".to_string(),
                is_package: true,
            }
        );
    }

    #[test]
    fn builders_get_import_path_falls_back_to_relative_import_when_package_has_no_root_export() {
        let root = TestDir::new("module-specifier");
        let project_root = root.join("apps/chat");
        let file_path = root.join("packages/agent/src/server.ts");
        root.write_json(
            "apps/chat/package.json",
            serde_json::json!({
                "name": "chat",
                "dependencies": { "@internal/agent": "workspace:*" }
            }),
        );
        root.write_json(
            "packages/agent/package.json",
            serde_json::json!({
                "name": "@internal/agent",
                "version": "1.0.0",
                "exports": { "./server": "./dist/server.js" }
            }),
        );
        root.write("packages/agent/src/server.ts", "'use step';");
        assert_eq!(
            get_import_path(file_path, project_root),
            ImportPathResult {
                import_path: "../../packages/agent/src/server.ts".to_string(),
                is_package: false,
            }
        );
    }

    #[test]
    fn builders_get_import_path_uses_package_root_import_for_root_exports() {
        let root = TestDir::new("module-specifier");
        let project_root = root.join("apps/chat");
        let file_path = root.join("packages/agent/src/index.ts");
        root.write_json(
            "apps/chat/package.json",
            serde_json::json!({
                "name": "chat",
                "dependencies": { "@internal/agent": "workspace:*" }
            }),
        );
        root.write_json(
            "packages/agent/package.json",
            serde_json::json!({
                "name": "@internal/agent",
                "version": "1.0.0",
                "exports": { ".": "./src/index.ts" }
            }),
        );
        root.write("packages/agent/src/index.ts", "'use workflow';");
        assert_eq!(
            get_import_path(file_path, project_root),
            ImportPathResult {
                import_path: "@internal/agent".to_string(),
                is_package: true,
            }
        );
    }

    #[test]
    fn builders_get_import_path_uses_package_root_import_when_package_module_points_to_file() {
        let root = TestDir::new("module-specifier");
        let project_root = root.join("apps/chat");
        let file_path = root.join("packages/agent/src/index.mjs");
        root.write_json(
            "apps/chat/package.json",
            serde_json::json!({
                "name": "chat",
                "dependencies": { "@internal/agent": "workspace:*" }
            }),
        );
        root.write_json(
            "packages/agent/package.json",
            serde_json::json!({
                "name": "@internal/agent",
                "version": "1.0.0",
                "module": "./src/index.mjs",
                "main": "./dist/index.cjs"
            }),
        );
        root.write("packages/agent/src/index.mjs", "'use workflow';");
        assert_eq!(
            get_import_path(file_path, project_root),
            ImportPathResult {
                import_path: "@internal/agent".to_string(),
                is_package: true,
            }
        );
    }

    #[test]
    fn builders_get_import_path_uses_package_root_import_for_conditional_root_exports() {
        let root = TestDir::new("module-specifier");
        let project_root = root.join("apps/chat");
        let file_path = root.join("packages/agent/src/index.js");
        root.write_json(
            "apps/chat/package.json",
            serde_json::json!({
                "name": "chat",
                "dependencies": { "@internal/agent": "workspace:*" }
            }),
        );
        root.write_json(
            "packages/agent/package.json",
            serde_json::json!({
                "name": "@internal/agent",
                "version": "1.0.0",
                "exports": { ".": { "import": "./src/index.mjs", "default": "./src/index.js" } }
            }),
        );
        root.write("packages/agent/src/index.js", "'use workflow';");
        assert_eq!(
            get_import_path(file_path, project_root),
            ImportPathResult {
                import_path: "@internal/agent".to_string(),
                is_package: true,
            }
        );
    }

    #[test]
    fn builders_get_import_path_falls_back_to_relative_import_for_deep_files_in_packages_without_exports()
     {
        let root = TestDir::new("module-specifier");
        let project_root = root.join("apps/chat");
        let file_path = root.join("packages/agent/lib/tools/dynamic/workflow.ts");
        root.write_json(
            "apps/chat/package.json",
            serde_json::json!({
                "name": "chat",
                "dependencies": { "@internal/agent": "workspace:*" }
            }),
        );
        root.write_json(
            "packages/agent/package.json",
            serde_json::json!({
                "name": "@internal/agent",
                "version": "1.0.0"
            }),
        );
        root.write(
            "packages/agent/lib/tools/dynamic/workflow.ts",
            "'use workflow';",
        );
        assert_eq!(
            get_import_path(file_path, project_root),
            ImportPathResult {
                import_path: "../../packages/agent/lib/tools/dynamic/workflow.ts".to_string(),
                is_package: false,
            }
        );
    }

    #[test]
    fn builders_get_import_path_uses_package_root_import_when_package_main_points_to_file() {
        let root = TestDir::new("module-specifier");
        let project_root = root.join("apps/chat");
        let file_path = root.join("packages/agent/src/index.ts");
        root.write_json(
            "apps/chat/package.json",
            serde_json::json!({
                "name": "chat",
                "dependencies": { "@internal/agent": "workspace:*" }
            }),
        );
        root.write_json(
            "packages/agent/package.json",
            serde_json::json!({
                "name": "@internal/agent",
                "version": "1.0.0",
                "main": "./src/index.ts"
            }),
        );
        root.write("packages/agent/src/index.ts", "'use step';");
        assert_eq!(
            get_import_path(file_path, project_root),
            ImportPathResult {
                import_path: "@internal/agent".to_string(),
                is_package: true,
            }
        );
    }

    #[test]
    fn builders_get_import_path_uses_package_subpath_import_for_direct_node_modules_dependencies() {
        let root = TestDir::new("module-specifier");
        let project_root = root.join("apps/chat");
        let file_path = root.join("apps/chat/node_modules/@workflow/core/dist/serialization.js");
        root.write_json(
            "apps/chat/package.json",
            serde_json::json!({
                "name": "chat",
                "dependencies": { "@workflow/core": "1.0.0" }
            }),
        );
        root.write_json(
            "apps/chat/node_modules/@workflow/core/package.json",
            serde_json::json!({
                "name": "@workflow/core",
                "version": "1.0.0",
                "exports": { "./serialization": "./dist/serialization.js" }
            }),
        );
        root.write(
            "apps/chat/node_modules/@workflow/core/dist/serialization.js",
            "'use workflow';",
        );
        assert_eq!(
            get_import_path(file_path, project_root),
            ImportPathResult {
                import_path: "@workflow/core/serialization".to_string(),
                is_package: true,
            }
        );
    }

    #[test]
    fn builders_get_import_path_falls_back_to_relative_import_for_transitive_node_modules_dependencies()
     {
        let root = TestDir::new("module-specifier");
        let project_root = root.join("apps/chat");
        let file_path = root.join("apps/chat/node_modules/@workflow/core/dist/serialization.js");
        root.write_json(
            "apps/chat/package.json",
            serde_json::json!({
                "name": "chat",
                "dependencies": { "workflow": "1.0.0" }
            }),
        );
        root.write_json(
            "apps/chat/node_modules/@workflow/core/package.json",
            serde_json::json!({
                "name": "@workflow/core",
                "version": "1.0.0",
                "exports": { "./serialization": "./dist/serialization.js" }
            }),
        );
        root.write(
            "apps/chat/node_modules/@workflow/core/dist/serialization.js",
            "'use workflow';",
        );
        assert_eq!(
            get_import_path(file_path, project_root),
            ImportPathResult {
                import_path: "./node_modules/@workflow/core/dist/serialization.js".to_string(),
                is_package: false,
            }
        );
    }

    #[test]
    fn builders_resolve_module_specifier_treats_a_workspace_package_file_as_local_when_project_root_is_the_package_itself()
     {
        let root = TestDir::new("module-specifier");
        let project_root = root.join("packages/vade");
        let file_path = root.join("packages/vade/src/internal/message/workflow/handle-message.ts");
        root.write_json(
            "packages/vade/package.json",
            serde_json::json!({
                "name": "vade",
                "version": "0.0.0"
            }),
        );
        root.write(
            "packages/vade/src/internal/message/workflow/handle-message.ts",
            "'use workflow';",
        );
        assert_eq!(
            resolve_module_specifier(file_path, project_root),
            ModuleSpecifierResult {
                module_specifier: None
            }
        );
    }

    #[test]
    fn builders_resolve_module_specifier_uses_the_consuming_app_root_to_resolve_workspace_package_workflow_ids()
     {
        let root = TestDir::new("module-specifier");
        let project_root = root.join("apps/chat");
        let file_path = root.join("packages/vade/src/internal/message/workflow/handle-message.ts");
        root.write_json(
            "apps/chat/package.json",
            serde_json::json!({
                "name": "chat",
                "dependencies": { "vade": "workspace:*" }
            }),
        );
        root.write_json(
            "packages/vade/package.json",
            serde_json::json!({
                "name": "vade",
                "version": "0.0.0"
            }),
        );
        root.write(
            "packages/vade/src/internal/message/workflow/handle-message.ts",
            "'use workflow';",
        );
        assert_eq!(
            resolve_module_specifier(file_path, project_root),
            ModuleSpecifierResult {
                module_specifier: Some("vade@0.0.0".to_string())
            }
        );
    }

    #[test]
    fn builders_resolve_module_specifier_preserves_package_export_subpaths_when_source_files_back_dist_exports()
     {
        let root = TestDir::new("module-specifier");
        let project_root = root.join("apps/chat");
        let file_path = root.join("packages/agent/src/server.ts");
        root.write_json(
            "apps/chat/package.json",
            serde_json::json!({
                "name": "chat",
                "dependencies": { "@internal/agent": "workspace:*" }
            }),
        );
        root.write_json(
            "packages/agent/package.json",
            serde_json::json!({
                "name": "@internal/agent",
                "version": "1.0.0",
                "exports": { "./server": "./dist/server.js" }
            }),
        );
        root.write("packages/agent/src/server.ts", "'use step';");
        assert_eq!(
            resolve_module_specifier(file_path, project_root),
            ModuleSpecifierResult {
                module_specifier: Some("@internal/agent/server@1.0.0".to_string())
            }
        );
    }

    #[test]
    fn builders_resolve_workflow_alias_relative_path_maps_files_in_workflows_to_workflows_aliases()
    {
        let root = TestDir::new("workflow-alias");
        let working_dir = root.join("project");
        let file_path = root.write("project/workflows/foo.ts", "'use workflow';");
        assert_eq!(
            resolve_workflow_alias_relative_path(file_path, working_dir),
            Some("workflows/foo.ts".to_string())
        );
    }

    #[test]
    fn builders_resolve_workflow_alias_relative_path_maps_files_in_src_workflows_to_src_workflows_aliases()
     {
        let root = TestDir::new("workflow-alias");
        let working_dir = root.join("project");
        let file_path = root.write("project/src/workflows/foo.ts", "'use workflow';");
        assert_eq!(
            resolve_workflow_alias_relative_path(file_path, working_dir),
            Some("src/workflows/foo.ts".to_string())
        );
    }

    #[test]
    fn builders_resolve_workflow_alias_relative_path_returns_undefined_for_files_that_are_not_under_workflows_paths()
     {
        let root = TestDir::new("workflow-alias");
        let working_dir = root.join("project");
        let file_path = root.write("project/lib/foo.ts", "'use workflow';");
        assert_eq!(
            resolve_workflow_alias_relative_path(file_path, working_dir),
            None
        );
    }

    #[test]
    fn builders_resolve_workflow_alias_relative_path_returns_undefined_when_basename_matches_but_realpath_differs()
     {
        let root = TestDir::new("workflow-alias");
        let working_dir = root.join("project");
        root.write("project/workflows/foo.ts", "'use workflow';");
        let external = root.write("external/workflows/foo.ts", "'use workflow';");
        assert_eq!(
            resolve_workflow_alias_relative_path(external, working_dir),
            None
        );
    }

    #[test]
    #[cfg(unix)]
    fn builders_resolve_workflow_alias_relative_path_maps_symlinked_app_files_to_app_aliases() {
        use std::os::unix::fs::symlink;

        let root = TestDir::new("workflow-alias");
        let working_dir = root.join("project");
        fs::create_dir_all(&working_dir).expect("create working dir");
        let external_app = root.join("external/app");
        let external_file = root.write(
            "external/app/.well-known/agent/v1/steps.ts",
            "'use workflow';",
        );
        symlink(&external_app, working_dir.join("app")).expect("create symlink");
        assert_eq!(
            resolve_workflow_alias_relative_path(external_file, working_dir),
            Some("app/.well-known/agent/v1/steps.ts".to_string())
        );
    }

    #[test]
    fn builders_resolve_sourcemap_returns_the_default_when_no_config_or_env_var_is_set() {
        assert_eq!(
            resolve_sourcemap(None, None, SourcemapMode::Inline),
            SourcemapMode::Inline
        );
        assert_eq!(
            resolve_sourcemap(None, None, SourcemapMode::Disabled),
            SourcemapMode::Disabled
        );
        assert_eq!(
            resolve_sourcemap(None, None, SourcemapMode::Enabled),
            SourcemapMode::Enabled
        );
    }

    #[test]
    fn builders_resolve_sourcemap_prefers_explicit_config_over_the_default() {
        assert_eq!(
            resolve_sourcemap(Some(SourcemapMode::Disabled), None, SourcemapMode::Inline),
            SourcemapMode::Disabled
        );
        assert_eq!(
            resolve_sourcemap(Some(SourcemapMode::External), None, SourcemapMode::Inline),
            SourcemapMode::External
        );
        assert_eq!(
            resolve_sourcemap(Some(SourcemapMode::Linked), None, SourcemapMode::Disabled),
            SourcemapMode::Linked
        );
        assert_eq!(
            resolve_sourcemap(Some(SourcemapMode::Enabled), None, SourcemapMode::Inline),
            SourcemapMode::Enabled
        );
    }

    #[test]
    fn builders_resolve_sourcemap_prefers_explicit_config_over_environment_variable() {
        assert_eq!(
            resolve_sourcemap(
                Some(SourcemapMode::Disabled),
                Some("inline"),
                SourcemapMode::Inline
            ),
            SourcemapMode::Disabled
        );
        assert_eq!(
            resolve_sourcemap(
                Some(SourcemapMode::External),
                Some("inline"),
                SourcemapMode::Inline
            ),
            SourcemapMode::External
        );
    }

    #[test]
    fn builders_resolve_sourcemap_uses_environment_variable_when_config_is_not_set() {
        assert_eq!(
            resolve_sourcemap(None, Some("false"), SourcemapMode::Inline),
            SourcemapMode::Disabled
        );
        assert_eq!(
            resolve_sourcemap(None, Some("true"), SourcemapMode::Disabled),
            SourcemapMode::Enabled
        );
        for (value, mode) in [
            ("inline", SourcemapMode::Inline),
            ("linked", SourcemapMode::Linked),
            ("external", SourcemapMode::External),
            ("both", SourcemapMode::Both),
        ] {
            assert_eq!(
                resolve_sourcemap(None, Some(value), SourcemapMode::Inline),
                mode
            );
        }
    }

    #[test]
    fn builders_resolve_sourcemap_accepts_0_1_as_environment_variable_aliases_for_false_true() {
        assert_eq!(
            resolve_sourcemap(None, Some("0"), SourcemapMode::Inline),
            SourcemapMode::Disabled
        );
        assert_eq!(
            resolve_sourcemap(None, Some("1"), SourcemapMode::Disabled),
            SourcemapMode::Enabled
        );
    }

    #[test]
    fn builders_resolve_sourcemap_falls_back_to_default_when_env_var_is_empty_or_unrecognized() {
        assert_eq!(
            resolve_sourcemap(None, Some(""), SourcemapMode::Inline),
            SourcemapMode::Inline
        );
        assert_eq!(
            resolve_sourcemap(None, Some("nonsense"), SourcemapMode::Inline),
            SourcemapMode::Inline
        );
    }

    #[test]
    fn builders_sourcemaps_enabled_is_true_by_default() {
        assert!(sourcemaps_enabled(None, None));
    }

    #[test]
    fn builders_sourcemaps_enabled_is_false_when_config_sourcemap_is_false() {
        assert!(!sourcemaps_enabled(Some(SourcemapMode::Disabled), None));
    }

    #[test]
    fn builders_sourcemaps_enabled_is_true_for_any_non_false_config_value() {
        for mode in [
            SourcemapMode::Enabled,
            SourcemapMode::Inline,
            SourcemapMode::Linked,
            SourcemapMode::External,
            SourcemapMode::Both,
        ] {
            assert!(sourcemaps_enabled(Some(mode), None));
        }
    }

    #[test]
    fn builders_sourcemaps_enabled_is_false_when_workflow_sourcemap_env_is_false() {
        assert!(!sourcemaps_enabled(None, Some("false")));
    }

    #[test]
    fn builders_use_workflow_pattern_should_match_use_workflow_with_single_quotes() {
        assert!(use_workflow_pattern("'use workflow';"));
        assert!(use_workflow_pattern("'use workflow'"));
    }

    #[test]
    fn builders_use_workflow_pattern_should_match_use_workflow_with_double_quotes() {
        assert!(use_workflow_pattern("\"use workflow\";"));
        assert!(use_workflow_pattern("\"use workflow\""));
    }

    #[test]
    fn builders_use_workflow_pattern_should_match_with_leading_whitespace() {
        assert!(use_workflow_pattern("  'use workflow';"));
        assert!(use_workflow_pattern("\t\"use workflow\";"));
    }

    #[test]
    fn builders_use_workflow_pattern_should_not_match_inline_usage() {
        assert!(!use_workflow_pattern("const x = 'use workflow';"));
    }

    #[test]
    fn builders_use_step_pattern_should_match_use_step_with_single_quotes() {
        assert!(use_step_pattern("'use step';"));
        assert!(use_step_pattern("'use step'"));
    }

    #[test]
    fn builders_use_step_pattern_should_match_use_step_with_double_quotes() {
        assert!(use_step_pattern("\"use step\";"));
        assert!(use_step_pattern("\"use step\""));
    }

    #[test]
    fn builders_workflow_serde_import_pattern_should_match_import_from_workflow_serde_with_single_quotes()
     {
        assert!(workflow_serde_import_pattern(
            "import { WORKFLOW_SERIALIZE } from '@workflow/serde';"
        ));
    }

    #[test]
    fn builders_workflow_serde_import_pattern_should_match_import_from_workflow_serde_with_double_quotes()
     {
        assert!(workflow_serde_import_pattern(
            "import { WORKFLOW_SERIALIZE } from \"@workflow/serde\";"
        ));
    }

    #[test]
    fn builders_workflow_serde_import_pattern_should_match_import_with_multiple_specifiers() {
        assert!(workflow_serde_import_pattern(
            "import { WORKFLOW_SERIALIZE, WORKFLOW_DESERIALIZE } from '@workflow/serde';"
        ));
    }

    #[test]
    fn builders_workflow_serde_import_pattern_should_match_import_with_type() {
        assert!(workflow_serde_import_pattern(
            "import type { SerializationSymbol } from '@workflow/serde';"
        ));
    }

    #[test]
    fn builders_workflow_serde_import_pattern_should_not_match_similar_but_different_packages() {
        assert!(!workflow_serde_import_pattern(
            "import { x } from '@other/serde';"
        ));
        assert!(!workflow_serde_import_pattern(
            "import { x } from '@workflow/serde-utils';"
        ));
    }

    #[test]
    fn builders_workflow_serde_symbol_pattern_should_match_symbol_for_with_workflow_serialize() {
        assert!(workflow_serde_symbol_pattern(
            "static [Symbol.for('workflow-serialize')](instance) {}"
        ));
    }

    #[test]
    fn builders_workflow_serde_symbol_pattern_should_match_symbol_for_with_workflow_deserialize() {
        assert!(workflow_serde_symbol_pattern(
            "static [Symbol.for('workflow-deserialize')](data) {}"
        ));
    }

    #[test]
    fn builders_workflow_serde_symbol_pattern_should_match_with_double_quotes() {
        assert!(workflow_serde_symbol_pattern(
            "static [Symbol.for(\"workflow-serialize\")](instance) {}"
        ));
    }

    #[test]
    fn builders_workflow_serde_symbol_pattern_should_match_with_whitespace_variations() {
        assert!(workflow_serde_symbol_pattern(
            "Symbol.for( 'workflow-serialize' )"
        ));
        assert!(workflow_serde_symbol_pattern(
            "Symbol.for('workflow-deserialize')"
        ));
    }

    #[test]
    fn builders_workflow_serde_symbol_pattern_should_match_in_a_full_class_definition() {
        let source = r#"
        export class Point {
          static [Symbol.for('workflow-serialize')](instance) { return {}; }
          static [Symbol.for('workflow-deserialize')](data) { return data; }
        }
      "#;
        assert!(workflow_serde_symbol_pattern(source));
    }

    #[test]
    fn builders_workflow_serde_symbol_pattern_should_not_match_other_symbol_for_usage() {
        assert!(!workflow_serde_symbol_pattern("Symbol.for('other-symbol')"));
        assert!(!workflow_serde_symbol_pattern(
            "Symbol.for('workflow-something-else')"
        ));
    }

    #[test]
    fn builders_workflow_serde_symbol_pattern_should_not_match_non_symbol_for_patterns() {
        assert!(!workflow_serde_symbol_pattern("'workflow-serialize'"));
        assert!(!workflow_serde_symbol_pattern("workflow-deserialize"));
    }

    #[test]
    fn builders_transform_utils_combined_detection_should_detect_file_using_imported_symbols() {
        let source = r#"
        import { WORKFLOW_SERIALIZE, WORKFLOW_DESERIALIZE } from '@workflow/serde';
        export class MyClass {
          static [WORKFLOW_SERIALIZE](instance) { return {}; }
          static [WORKFLOW_DESERIALIZE](data) { return data; }
        }
      "#;
        assert!(workflow_serde_import_pattern(source));
        assert!(!workflow_serde_symbol_pattern(source));
    }

    #[test]
    fn builders_transform_utils_combined_detection_should_detect_file_using_direct_symbol_for() {
        let source = r#"
        export class Point {
          static [Symbol.for('workflow-serialize')](instance) { return {}; }
          static [Symbol.for('workflow-deserialize')](data) { return data; }
        }
      "#;
        assert!(!workflow_serde_import_pattern(source));
        assert!(workflow_serde_symbol_pattern(source));
    }

    #[test]
    fn builders_transform_utils_combined_detection_should_detect_file_with_both_patterns() {
        let source = r#"
        import { WORKFLOW_SERIALIZE } from '@workflow/serde';
        export class Point {
          static [WORKFLOW_SERIALIZE](instance) { return {}; }
          static [Symbol.for('workflow-deserialize')](data) { return data; }
        }
      "#;
        assert!(workflow_serde_import_pattern(source));
        assert!(workflow_serde_symbol_pattern(source));
    }

    #[test]
    fn builders_workflow_serde_computed_property_pattern_should_match_workflow_serialize_computed_property()
     {
        assert!(workflow_serde_computed_property_pattern(
            "static [WORKFLOW_SERIALIZE](instance) {}"
        ));
    }

    #[test]
    fn builders_workflow_serde_computed_property_pattern_should_match_workflow_deserialize_computed_property()
     {
        assert!(workflow_serde_computed_property_pattern(
            "static [WORKFLOW_DESERIALIZE](data) {}"
        ));
    }

    #[test]
    fn builders_workflow_serde_computed_property_pattern_should_match_with_whitespace_inside_brackets()
     {
        assert!(workflow_serde_computed_property_pattern(
            "[ WORKFLOW_SERIALIZE ]"
        ));
        assert!(workflow_serde_computed_property_pattern(
            "[  WORKFLOW_DESERIALIZE  ]"
        ));
    }

    #[test]
    fn builders_workflow_serde_computed_property_pattern_should_match_in_bundled_code_where_symbols_are_imported_from_chunks()
     {
        let source = r#"
        import { WORKFLOW_DESERIALIZE, WORKFLOW_SERIALIZE } from "./chunks/chunk-453323QY.js";
        var Bash = class _Bash {
          static [WORKFLOW_SERIALIZE](instance) { return {}; }
          static [WORKFLOW_DESERIALIZE](serialized) { return serialized; }
        };
      "#;
        assert!(workflow_serde_computed_property_pattern(source));
        assert!(!workflow_serde_import_pattern(source));
    }

    #[test]
    fn builders_workflow_serde_computed_property_pattern_should_not_match_partial_names() {
        assert!(!workflow_serde_computed_property_pattern(
            "[WORKFLOW_SERIALIZE_EXTRA]"
        ));
        assert!(!workflow_serde_computed_property_pattern(
            "[MY_WORKFLOW_SERIALIZE]"
        ));
    }

    #[test]
    fn builders_workflow_serde_computed_property_pattern_should_not_match_string_literals() {
        assert!(!workflow_serde_computed_property_pattern(
            "['WORKFLOW_SERIALIZE']"
        ));
        assert!(!workflow_serde_computed_property_pattern(
            "[\"WORKFLOW_DESERIALIZE\"]"
        ));
    }

    #[test]
    fn builders_detect_workflow_patterns_should_detect_has_serde_for_workflow_serde_import() {
        let result =
            detect_workflow_patterns("import { WORKFLOW_SERIALIZE } from '@workflow/serde';");
        assert!(result.has_serde);
        assert!(result.has_serde_import);
    }

    #[test]
    fn builders_detect_workflow_patterns_should_detect_has_serde_for_symbol_for_pattern() {
        let result =
            detect_workflow_patterns("static [Symbol.for('workflow-serialize')](instance) {}");
        assert!(result.has_serde);
        assert!(result.has_serde_symbol);
    }

    #[test]
    fn builders_detect_workflow_patterns_should_detect_has_serde_for_computed_property_pattern() {
        let result = detect_workflow_patterns("static [WORKFLOW_SERIALIZE](instance) {}");
        assert!(result.has_serde);
    }

    #[test]
    fn builders_detect_workflow_patterns_should_detect_has_serde_for_bundled_third_party_packages()
    {
        let source = r#"
        import { WORKFLOW_DESERIALIZE, WORKFLOW_SERIALIZE } from "./chunks/chunk-ABC123.js";
        var MyClass = class {
          static [WORKFLOW_SERIALIZE](instance) { return {}; }
          static [WORKFLOW_DESERIALIZE](serialized) { return serialized; }
        };
      "#;
        assert!(detect_workflow_patterns(source).has_serde);
    }

    #[test]
    fn builders_detect_workflow_patterns_should_not_detect_has_serde_for_unrelated_code() {
        let result = detect_workflow_patterns(
            "export class RegularClass { constructor(value) { this.value = value; } }",
        );
        assert!(!result.has_serde);
    }

    #[test]
    fn builders_detect_workflow_patterns_should_detect_both_directive_and_serde_patterns() {
        let source = r#"
        'use step';
        import { WORKFLOW_SERIALIZE } from '@workflow/serde';
        export class Point {
          static [WORKFLOW_SERIALIZE](instance) { return {}; }
        }
      "#;
        let result = detect_workflow_patterns(source);
        assert!(result.has_directive);
        assert!(result.has_use_step);
        assert!(result.has_serde);
    }

    #[test]
    fn builders_detect_workflow_patterns_regexp_detection_matches_directives_inside_template_literals_false_positive()
     {
        let source = "'use client';\nconst CODE_SNIPPET = `import { sleep } from \"workflow\";\n\nexport async function handleUserSignup(email: string) {\n  \"use workflow\";\n  const user = await createUser(email);\n}\n`;\nexport default function Page() { return null; }\n";
        let result = detect_workflow_patterns(source);
        assert!(result.has_use_workflow);
        assert!(result.has_directive);
    }

    #[test]
    fn builders_should_transform_file_excludes_generated_workflow_route_files_even_with_directives()
    {
        assert!(!should_transform_file(
            "/app/.well-known/workflow/v1/route.ts",
            WorkflowPatterns {
                has_use_workflow: true,
                has_directive: true,
                ..WorkflowPatterns::default()
            }
        ));
    }

    #[test]
    fn builders_should_transform_file_transforms_files_with_directive_patterns() {
        assert!(should_transform_file(
            "/app/workflows/my-workflow.ts",
            WorkflowPatterns {
                has_use_workflow: true,
                has_directive: true,
                ..WorkflowPatterns::default()
            }
        ));
    }

    #[test]
    fn builders_should_transform_file_transforms_files_with_serde_patterns() {
        assert!(should_transform_file(
            "/app/lib/my-class.ts",
            WorkflowPatterns {
                has_serde_import: true,
                has_serde: true,
                ..WorkflowPatterns::default()
            }
        ));
    }

    #[test]
    fn builders_should_transform_file_transforms_sdk_files_with_serde_patterns_no_longer_excluded()
    {
        assert!(should_transform_file(
            "/app/node_modules/@workflow/core/dist/serialization.js",
            WorkflowPatterns {
                has_serde_symbol: true,
                has_serde: true,
                ..WorkflowPatterns::default()
            }
        ));
    }

    #[test]
    fn builders_should_transform_file_does_not_transform_files_without_directives_or_serde_patterns()
     {
        assert!(!should_transform_file(
            "/app/lib/utils.ts",
            WorkflowPatterns::default()
        ));
    }

    #[test]
    fn builders_pseudo_packages_constant_should_contain_next_marker_packages() {
        let packages = pseudo_packages();
        assert!(packages.contains(&"server-only"));
        assert!(packages.contains(&"client-only"));
        assert!(packages.contains(&"next/dist/compiled/server-only"));
        assert!(packages.contains(&"next/dist/compiled/client-only"));
        assert_eq!(packages.len(), 4);
    }

    #[test]
    fn builders_get_package_name_should_get_the_package_name_from_simple_node_modules_path() {
        assert_eq!(
            get_package_name(
                "/Users/adrianlam/GitHub/workflow/node_modules/node-fetch/src/index.js"
            ),
            Some("node-fetch".to_string())
        );
    }

    #[test]
    fn builders_get_package_name_should_get_the_package_name_from_pnpm_nested_path() {
        assert_eq!(
            get_package_name(
                "/Users/adrianlam/GitHub/workflow/node_modules/.pnpm/node-fetch@3.3.2/node_modules/node-fetch/src/index.js"
            ),
            Some("node-fetch".to_string())
        );
    }

    #[test]
    fn builders_get_package_name_should_get_scoped_package_name() {
        assert_eq!(
            get_package_name("/project/node_modules/@supabase/supabase-js/dist/index.js"),
            Some("@supabase/supabase-js".to_string())
        );
    }

    #[test]
    fn builders_get_package_name_should_return_null_for_paths_without_node_modules() {
        assert_eq!(
            get_package_name("/Users/adrianlam/GitHub/workflow/src/index.js"),
            None
        );
    }

    #[test]
    fn builders_escape_reg_exp_should_escape_regex_special_characters() {
        assert_eq!(escape_reg_exp("test.file"), "test\\.file");
        assert_eq!(escape_reg_exp("test*file"), "test\\*file");
        assert_eq!(escape_reg_exp("test+file"), "test\\+file");
        assert_eq!(escape_reg_exp("test?file"), "test\\?file");
        assert_eq!(escape_reg_exp("test^file"), "test\\^file");
        assert_eq!(escape_reg_exp("test$file"), "test\\$file");
    }

    #[test]
    fn builders_escape_reg_exp_should_escape_brackets_and_braces() {
        assert_eq!(escape_reg_exp("test{file}"), "test\\{file\\}");
        assert_eq!(escape_reg_exp("test[file]"), "test\\[file\\]");
        assert_eq!(escape_reg_exp("test(file)"), "test\\(file\\)");
    }

    #[test]
    fn builders_escape_reg_exp_should_escape_pipes_and_backslashes() {
        assert_eq!(escape_reg_exp("test|file"), "test\\|file");
        assert_eq!(escape_reg_exp("test\\file"), "test\\\\file");
    }

    #[test]
    fn builders_escape_reg_exp_should_handle_strings_without_special_characters() {
        assert_eq!(escape_reg_exp("testfile"), "testfile");
        assert_eq!(escape_reg_exp("test-file"), "test-file");
    }

    #[test]
    fn builders_escape_reg_exp_should_handle_package_names_with_special_characters() {
        assert_eq!(
            escape_reg_exp("@supabase/supabase-js"),
            "@supabase/supabase-js"
        );
        assert_eq!(escape_reg_exp("package.name"), "package\\.name");
    }

    #[test]
    fn builders_get_imported_identifier_should_extract_namespace_import_identifier() {
        assert_eq!(get_imported_identifier("* as fs"), Some("fs".to_string()));
        assert_eq!(
            get_imported_identifier("*   as   path"),
            Some("path".to_string())
        );
    }

    #[test]
    fn builders_get_imported_identifier_should_extract_first_named_import() {
        assert_eq!(
            get_imported_identifier("{ readFile }"),
            Some("readFile".to_string())
        );
        assert_eq!(
            get_imported_identifier("{ readFile, writeFile }"),
            Some("readFile".to_string())
        );
    }

    #[test]
    fn builders_get_imported_identifier_should_extract_aliased_named_import() {
        assert_eq!(
            get_imported_identifier("{ readFile as read }"),
            Some("read".to_string())
        );
        assert_eq!(
            get_imported_identifier("{ readFile as read, writeFile }"),
            Some("read".to_string())
        );
    }

    #[test]
    fn builders_get_imported_identifier_should_extract_default_import() {
        assert_eq!(get_imported_identifier("fs"), Some("fs".to_string()));
        assert_eq!(
            get_imported_identifier("myDefault"),
            Some("myDefault".to_string())
        );
    }

    #[test]
    fn builders_get_imported_identifier_should_extract_first_identifier_from_mixed_imports() {
        assert_eq!(
            get_imported_identifier("fs, { readFile }"),
            Some("readFile".to_string())
        );
        assert_eq!(
            get_imported_identifier("defaultExport, { named }"),
            Some("named".to_string())
        );
    }

    #[test]
    fn builders_get_imported_identifier_should_handle_whitespace_variations() {
        assert_eq!(
            get_imported_identifier("  { readFile }  "),
            Some("readFile".to_string())
        );
        assert_eq!(
            get_imported_identifier("{readFile}"),
            Some("readFile".to_string())
        );
        assert_eq!(
            get_imported_identifier("{ readFile , writeFile }"),
            Some("readFile".to_string())
        );
    }

    #[test]
    fn builders_get_imported_identifier_should_handle_complex_named_imports() {
        assert_eq!(
            get_imported_identifier("type { ReadStream }"),
            Some("ReadStream".to_string())
        );
        assert_eq!(
            get_imported_identifier("{ default as fs }"),
            Some("fs".to_string())
        );
    }

    #[test]
    fn builders_get_imported_identifier_should_return_undefined_for_edge_cases() {
        assert_eq!(get_imported_identifier("*"), None);
        assert_eq!(get_imported_identifier(""), None);
        assert_eq!(get_imported_identifier("{}"), None);
    }

    #[test]
    fn builders_get_violation_location_should_find_violation_location_for_package_name_that_appears_in_file()
     {
        let root = TestDir::new("node-module-location");
        root.write(
            "src/node-module-esbuild-plugin.test.ts",
            "import { describe, expect } from 'vitest';\n\ndescribe('suite', () => {\n  expect(true).toBe(true);\n});\n",
        );
        let location = get_violation_location(
            &root.path,
            "src/node-module-esbuild-plugin.test.ts",
            "vitest",
        )
        .expect("location");
        assert_eq!(location.file, "src/node-module-esbuild-plugin.test.ts");
        assert_eq!(location.line, 3);
        assert_eq!(location.column, 0);
        assert!(location.line_text.contains("describe("));
        assert_eq!(location.length, 8);
    }

    #[test]
    fn builders_get_violation_location_should_return_undefined_for_non_existent_files() {
        let root = TestDir::new("node-module-location");
        assert_eq!(
            get_violation_location(&root.path, "non-existent-file.ts", "some-package"),
            None
        );
    }

    #[test]
    fn builders_get_violation_location_should_return_undefined_for_files_without_the_package_import()
     {
        let root = TestDir::new("node-module-location");
        root.write(
            "src/node-module-esbuild-plugin.test.ts",
            "import { describe } from 'vitest';\n\ndescribe('suite', () => {});\n",
        );
        assert_eq!(
            get_violation_location(
                &root.path,
                "src/node-module-esbuild-plugin.test.ts",
                "non-existent-package"
            ),
            None
        );
    }

    #[test]
    fn builders_get_violation_location_should_return_undefined_when_import_is_unused_even_if_it_can_be_parsed()
     {
        let root = TestDir::new("node-module-location");
        root.write(
            "src/node-module-esbuild-plugin.test.ts",
            "import http from 'node:http';\n\nexport const value = 1;\n",
        );
        assert_eq!(
            get_violation_location(
                &root.path,
                "src/node-module-esbuild-plugin.test.ts",
                "node:http"
            ),
            None
        );
    }
}
