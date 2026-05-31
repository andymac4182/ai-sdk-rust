use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    thread,
};

type CheckResult<T> = Result<T, String>;

const BANNED_TOKENS: &[&str] = &[
    "helper", "helpers", "util", "utils", "common", "misc", "stuff", "shared",
];

fn main() -> ExitCode {
    match run() {
        Ok(failures) if failures.is_empty() => {
            println!("Naming convention check passed.");
            ExitCode::SUCCESS
        }
        Ok(failures) => {
            eprintln!("Naming convention check failed:");
            for failure in failures {
                eprintln!("  - {failure}");
            }
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("Naming convention check failed to run: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> CheckResult<Vec<String>> {
    let root =
        env::current_dir().map_err(|error| format!("failed to read current dir: {error}"))?;
    let files = git_ls_files()?;

    run_checks(&root, &files)
}

fn git_ls_files() -> CheckResult<Vec<String>> {
    let output = Command::new("git")
        .args(["ls-files", "-z"])
        .output()
        .map_err(|error| format!("failed to run git ls-files: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git ls-files failed: {}", stderr.trim()));
    }

    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            String::from_utf8(path.to_vec())
                .map_err(|error| format!("git ls-files returned non-UTF-8 path: {error}"))
        })
        .collect()
}

fn run_checks(root: &Path, files: &[String]) -> CheckResult<Vec<String>> {
    let mut failures = Vec::new();

    failures.extend(scan_strings_parallel(files, |path| Ok(check_path(path)))?);

    let cargo_manifests = files
        .iter()
        .filter(|path| path.as_str() == "Cargo.toml" || path.ends_with("/Cargo.toml"))
        .cloned()
        .collect::<Vec<_>>();
    failures.extend(scan_strings_parallel(&cargo_manifests, |path| {
        scan_cargo_manifest(root, path)
    })?);

    let rust_files = files
        .iter()
        .filter(|path| path.ends_with(".rs"))
        .cloned()
        .collect::<Vec<_>>();
    failures.extend(scan_strings_parallel(&rust_files, |path| {
        scan_rust_declarations(root, path)
    })?);

    let documented_files = files
        .iter()
        .filter(|path| path.ends_with(".md") || path.ends_with(".rs"))
        .cloned()
        .collect::<Vec<_>>();
    failures.extend(scan_strings_parallel(&documented_files, |path| {
        scan_documented_identifiers(root, path)
    })?);

    Ok(failures)
}

fn scan_strings_parallel<F>(items: &[String], scan: F) -> CheckResult<Vec<String>>
where
    F: Fn(&str) -> CheckResult<Vec<String>> + Sync,
{
    if items.is_empty() {
        return Ok(Vec::new());
    }

    let worker_count = thread::available_parallelism()
        .map(|workers| workers.get())
        .unwrap_or(1)
        .min(items.len());
    let chunk_size = items.len().div_ceil(worker_count);

    let mut indexed_results = thread::scope(|scope| {
        let mut handles = Vec::new();
        for (chunk_index, chunk) in items.chunks(chunk_size).enumerate() {
            let scan = &scan;
            let start_index = chunk_index * chunk_size;
            handles.push(scope.spawn(move || {
                chunk
                    .iter()
                    .enumerate()
                    .map(|(offset, path)| (start_index + offset, scan(path)))
                    .collect::<Vec<_>>()
            }));
        }

        let mut results = Vec::with_capacity(items.len());
        for handle in handles {
            let chunk_results = handle
                .join()
                .map_err(|_| "naming check worker panicked".to_string())?;
            results.extend(chunk_results);
        }
        Ok::<_, String>(results)
    })?;

    indexed_results.sort_by_key(|(index, _)| *index);

    let mut failures = Vec::new();
    for (_, result) in indexed_results {
        failures.extend(result?);
    }

    Ok(failures)
}

fn check_path(path: &str) -> Vec<String> {
    if allowed_path(path) {
        return Vec::new();
    }

    let mut failures = Vec::new();
    for component in path.split('/') {
        check_identifier(
            &format!("{path} path component"),
            component_stem(component),
            &mut failures,
        );
    }
    failures
}

fn component_stem(component: &str) -> &str {
    component
        .rfind('.')
        .map_or(component, |extension_start| &component[..extension_start])
}

fn scan_cargo_manifest(root: &Path, path: &str) -> CheckResult<Vec<String>> {
    let content = read_to_string(root, path)?;
    let mut failures = Vec::new();

    for line in content.lines() {
        if let Some(name) = cargo_name_value(line) {
            check_identifier(&format!("{path} crate name"), name, &mut failures);
        }
    }

    Ok(failures)
}

fn cargo_name_value(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix("name")?.trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn scan_rust_declarations(root: &Path, path: &str) -> CheckResult<Vec<String>> {
    if skip_declared_identifier_scan(path) {
        return Ok(Vec::new());
    }

    let content = read_to_string(root, path)?;
    let mut failures = Vec::new();

    for (line_index, line) in content.lines().enumerate() {
        let origin = format!("{path}:{}", line_index + 1);
        for name in rust_declaration_names(line) {
            check_identifier(&origin, name, &mut failures);
        }
    }

    Ok(failures)
}

fn rust_declaration_names(line: &str) -> Vec<&str> {
    let trimmed = line.trim_start();
    let mut names = Vec::new();

    let after_optional_visibility = strip_pub_visibility(trimmed).unwrap_or(trimmed);
    if let Some(name) = capture_after_keyword(after_optional_visibility, "mod") {
        names.push(name);
    }

    if let Some(after_visibility) = strip_pub_visibility(trimmed) {
        let after_visibility = after_visibility.trim_start();
        let after_async = strip_keyword_with_space(after_visibility, "async")
            .map(str::trim_start)
            .unwrap_or(after_visibility);

        for keyword in [
            "fn", "struct", "enum", "trait", "type", "const", "static", "mod",
        ] {
            if let Some(name) = capture_after_keyword(after_async, keyword) {
                names.push(name);
                break;
            }
        }
    }

    if let Some(name) = capture_pub_use_prefix(trimmed) {
        names.push(name);
    }

    names
}

fn strip_pub_visibility(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("pub")?;
    let first = rest.chars().next()?;

    if first.is_whitespace() {
        return Some(rest);
    }

    if first != '(' {
        return None;
    }

    let visibility_end = rest.find(')')?;
    let after_visibility = &rest[(visibility_end + 1)..];
    match after_visibility.chars().next() {
        Some(next) if next.is_whitespace() => Some(after_visibility),
        _ => None,
    }
}

fn capture_after_keyword<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = strip_keyword_with_space(line, keyword)?;
    capture_identifier(rest.trim_start())
}

fn strip_keyword_with_space<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(keyword)?;
    match rest.chars().next() {
        Some(next) if next.is_whitespace() => Some(rest),
        _ => None,
    }
}

fn capture_pub_use_prefix(line: &str) -> Option<&str> {
    let rest = strip_keyword_with_space(line, "pub")?.trim_start();
    let rest = strip_keyword_with_space(rest, "use")?.trim_start();
    let name = capture_identifier(rest)?;
    rest[name.len()..].starts_with("::").then_some(name)
}

fn capture_identifier(line: &str) -> Option<&str> {
    let mut chars = line.char_indices();
    let (_, first) = chars.next()?;
    if !is_identifier_start(first) {
        return None;
    }

    let mut end = first.len_utf8();
    for (index, character) in chars {
        if !is_identifier_continue(character) {
            break;
        }
        end = index + character.len_utf8();
    }

    Some(&line[..end])
}

fn scan_documented_identifiers(root: &Path, path: &str) -> CheckResult<Vec<String>> {
    if skip_declared_identifier_scan(path) {
        return Ok(Vec::new());
    }

    let content = read_to_string(root, path)?;
    let mut failures = Vec::new();

    for (line_index, line) in content.lines().enumerate() {
        for identifier in documented_identifiers(line) {
            check_identifier(
                &format!("{path}:{} documented identifier", line_index + 1),
                identifier,
                &mut failures,
            );
        }
    }

    Ok(failures)
}

fn documented_identifiers(line: &str) -> Vec<&str> {
    let mut identifiers = Vec::new();
    let mut rest = line;

    while let Some(start) = rest.find('`') {
        let after_start = &rest[(start + 1)..];
        let Some(end) = after_start.find('`') else {
            break;
        };
        let identifier = &after_start[..end];
        if is_documented_identifier(identifier) {
            identifiers.push(identifier);
        }
        rest = &after_start[(end + 1)..];
    }

    identifiers
}

fn is_documented_identifier(identifier: &str) -> bool {
    !identifier.contains("@ai-sdk/provider-utils")
        && !identifier.contains("provider_utils")
        && !identifier.contains("provider-utils")
        && identifier.chars().next().is_some_and(is_identifier_start)
        && identifier
            .chars()
            .skip(1)
            .all(|character| is_identifier_continue(character) || character == '-')
}

fn skip_declared_identifier_scan(path: &str) -> bool {
    path.starts_with("scripts/codex-goal/")
        || path.starts_with("scripts/codex-goal-chat/")
        || path == "scripts/run-gnhf-port.sh"
}

fn check_identifier(origin: &str, name: &str, failures: &mut Vec<String>) {
    for token in identifier_tokens(name) {
        if is_banned_token(&token) && !allowed_identifier_token(name, &token, origin) {
            failures.push(format!("{origin}: '{name}' uses vague token '{token}'"));
        }
    }
}

fn identifier_tokens(name: &str) -> Vec<String> {
    let mut normalized = String::with_capacity(name.len());
    let mut previous_was_lower_or_digit = false;

    for character in name.chars() {
        if character.is_ascii_uppercase() && previous_was_lower_or_digit {
            normalized.push('_');
        }
        normalized.push(character.to_ascii_lowercase());
        previous_was_lower_or_digit = character.is_ascii_lowercase() || character.is_ascii_digit();
    }

    normalized
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect()
}

fn is_banned_token(token: &str) -> bool {
    BANNED_TOKENS.contains(&token)
}

fn allowed_path(path: &str) -> bool {
    matches!(
        path,
        "src/provider_utils.rs"
            | "src/util.rs"
            | "crates/chat-sdk-adapter-gchat/src/thread_utils.rs"
            | "crates/chat-sdk-adapter-linear/src/utils.rs"
    )
}

fn allowed_identifier_token(name: &str, token: &str, origin: &str) -> bool {
    if token == "utils" && name == "utils" && origin.contains("chat-sdk-adapter-linear/src/lib.rs")
    {
        return true;
    }

    if token == "utils" && (name.contains("provider_utils") || name.contains("provider-utils")) {
        return true;
    }

    if token == "utils"
        && [
            "adapter_utils",
            "adapter-utils",
            "buffer_utils",
            "buffer-utils",
            "card_utils",
            "card-utils",
            "thread_utils",
            "thread-utils",
        ]
        .iter()
        .any(|allowed| name.contains(allowed))
    {
        return true;
    }

    if token == "util" && name.contains("mdast_util_to_markdown") {
        return true;
    }

    if token == "util" && name == "util" {
        return true;
    }

    if token == "shared" && name == "SharedV4ProviderReference" {
        return true;
    }

    if token == "shared" && (name.contains("adapter_shared") || name.contains("adapter-shared")) {
        return true;
    }

    token == "helper"
        && matches!(
            name,
            "EmojiHelper" | "BaseEmojiHelper" | "ExtendedEmojiHelper"
        )
}

fn is_identifier_start(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '_'
}

fn is_identifier_continue(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn read_to_string(root: &Path, path: &str) -> CheckResult<String> {
    let full_path = root.join(PathBuf::from(path));
    fs::read_to_string(&full_path)
        .map_err(|error| format!("failed to read {}: {error}", full_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn splits_mixed_identifier_tokens() {
        assert_eq!(
            identifier_tokens("BaseEmojiHelper-provider-utils2"),
            vec!["base", "emoji", "helper", "provider", "utils2"]
        );
        assert_eq!(
            identifier_tokens("HTTPServer2Thing"),
            vec!["httpserver2", "thing"]
        );
    }

    #[test]
    fn preserves_allowlisted_exceptions() {
        let mut failures = Vec::new();
        for (origin, name) in [
            ("src/provider_utils.rs path component", "provider_utils"),
            ("src/lib.rs:1", "util"),
            ("src/lib.rs:2", "SharedV4ProviderReference"),
            ("src/lib.rs:3", "EmojiHelper"),
            ("crates/chat-sdk-adapter-linear/src/lib.rs:1", "utils"),
            (
                "docs/chat.md:1 documented identifier",
                "mdast_util_to_markdown",
            ),
            ("docs/chat.md:2 documented identifier", "adapter-shared"),
        ] {
            check_identifier(origin, name, &mut failures);
        }

        assert!(failures.is_empty(), "{failures:?}");
    }

    #[test]
    fn reports_path_component_failures() {
        assert_eq!(
            check_path("src/common.rs"),
            vec!["src/common.rs path component: 'common' uses vague token 'common'"]
        );
        assert!(check_path("src/provider_utils.rs").is_empty());
    }

    #[test]
    fn scans_public_rust_declarations() {
        let root = temp_root("rust-declarations");
        let source = [
            "mod stuff;",
            "pub struct CommonThing;",
            "pub(crate) async fn helper_call() {}",
            "pub use shared_area::Thing;",
            "",
        ]
        .join("\n");
        write_file(&root, "src/lib.rs", &source);

        let failures = scan_rust_declarations(&root, "src/lib.rs").unwrap();

        assert_eq!(
            failures,
            vec![
                "src/lib.rs:1: 'stuff' uses vague token 'stuff'",
                "src/lib.rs:2: 'CommonThing' uses vague token 'common'",
                "src/lib.rs:3: 'helper_call' uses vague token 'helper'",
                "src/lib.rs:4: 'shared_area' uses vague token 'shared'",
            ]
        );
    }

    #[test]
    fn scans_documented_backtick_identifiers() {
        let root = temp_root("documented-identifiers");
        let tick = char::from(96);
        write_file(
            &root,
            "docs/names.md",
            &format!(
                "Flag {tick}shared_value{tick}\nIgnore {tick}provider_utils{tick}\nIgnore {tick}@ai-sdk/provider-utils{tick}\n"
            ),
        );

        let failures = scan_documented_identifiers(&root, "docs/names.md").unwrap();

        assert_eq!(
            failures,
            vec!["docs/names.md:1 documented identifier: 'shared_value' uses vague token 'shared'"]
        );
    }

    #[test]
    fn returns_deterministic_failure_order() {
        let root = temp_root("deterministic-order");
        write_file(&root, "b.rs", "pub struct SharedName;\n");
        write_file(&root, "a.rs", "pub struct HelperName;\n");
        write_file(&root, "Cargo.toml", "name = \"misc-box\"\n");

        let files = vec![
            "b.rs".to_string(),
            "a.rs".to_string(),
            "Cargo.toml".to_string(),
        ];
        let failures = run_checks(&root, &files).unwrap();

        assert_eq!(
            failures,
            vec![
                "Cargo.toml crate name: 'misc-box' uses vague token 'misc'",
                "b.rs:1: 'SharedName' uses vague token 'shared'",
                "a.rs:1: 'HelperName' uses vague token 'helper'",
            ]
        );
    }

    fn temp_root(label: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!(
            "repo-naming-check-{}-{id}-{label}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_file(root: &Path, path: &str, content: &str) {
        let full_path = root.join(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(full_path, content).unwrap();
    }
}
