//! Portable Rust port of the upstream `just-bash` search-engine command core.
//!
//! Mirrors `packages/just-bash/src/commands/search-engine/regex.ts` and
//! `matcher.ts`. The two safety-critical pieces are:
//!
//! - `build_regex`: translates a grep-style pattern (basic/extended/fixed/perl
//!   mode) into a Rust [`regex::Regex`] and, when provably safe, extracts a
//!   substring [`PreFilter`]. A false-positive needle would silently skip lines
//!   the regex would match, so extraction is deliberately conservative.
//! - `search_content`: scans content line-by-line (or as a multiline blob),
//!   honouring the pre-filter fast-path, invert-match, line numbers, only-match
//!   replacement (`$&`, `$1`, `$<name>`) and the file-level multiline pre-filter.
//!
//! Only the behaviour exercised by the upstream `matcher.test.ts` and
//! `regex.test.ts` colocated suites is ported here; richer rg/grep output
//! formatting lives in the runtime command layer.
//!
//! The public surface is currently consumed only by the colocated parity tests
//! below (the runtime grep/rg path has its own matcher); allow dead code so the
//! faithful 1:1 port can stand as the parity proof without a premature wiring.
#![allow(dead_code)]

use regex::Regex;

/// Regex flavour, matching the upstream `RegexMode` union.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegexMode {
    Basic,
    Extended,
    Fixed,
    Perl,
}

/// Options accepted by [`build_regex`], mirroring upstream `RegexOptions`.
#[derive(Clone, Copy, Debug)]
pub struct RegexOptions {
    pub mode: RegexMode,
    pub ignore_case: bool,
    pub whole_word: bool,
    pub line_regexp: bool,
    pub multiline: bool,
}

impl RegexOptions {
    pub fn new(mode: RegexMode) -> Self {
        Self {
            mode,
            ignore_case: false,
            whole_word: false,
            line_regexp: false,
            multiline: false,
        }
    }
}

/// Substring fast-path filter: any one needle must appear in a matching line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreFilter {
    pub needles: Vec<String>,
    pub ignore_case: bool,
}

/// Result of [`build_regex`]: the compiled regex plus an optional pre-filter.
pub struct RegexResult {
    pub regex: Regex,
    pub pre_filter: Option<PreFilter>,
}

/// Build a compiled regex and optional pre-filter from a grep-style pattern.
pub fn build_regex(pattern: &str, options: RegexOptions) -> RegexResult {
    let mut regex_pattern = match options.mode {
        RegexMode::Fixed => escape_fixed(pattern),
        RegexMode::Extended | RegexMode::Perl => {
            let transformed = transform_posix_character_classes(pattern);
            // Convert (?P<name>...) -> (?<name>...) to match upstream; the Rust
            // regex crate accepts both, so this keeps the translated pattern
            // identical to upstream for pre-filter extraction.
            convert_named_groups(&transformed)
        }
        RegexMode::Basic => {
            let transformed = transform_posix_character_classes(pattern);
            escape_regex_for_basic_grep(&transformed)
        }
    };

    if options.whole_word {
        regex_pattern = format!("\\b(?:{regex_pattern})\\b");
    }
    if options.line_regexp {
        regex_pattern = format!("^{regex_pattern}$");
    }

    let pre_filter = extract_pre_filter(&regex_pattern, options.ignore_case);

    // Build the matchable regex. `(?P<name>)` is the Rust-native named-group
    // form, so translate `(?<name>)` back for compilation.
    let compile_src = named_groups_for_rust(&regex_pattern);
    let mut builder = String::from("(?");
    if options.ignore_case {
        builder.push('i');
    }
    if options.multiline {
        builder.push('m');
    }
    builder.push(')');
    let final_src = if builder == "(?)" {
        compile_src
    } else {
        format!("{builder}{compile_src}")
    };
    let regex = Regex::new(&final_src)
        .unwrap_or_else(|_| Regex::new(&regex::escape(pattern)).expect("fallback regex compiles"));

    RegexResult { regex, pre_filter }
}

/// Escape every regex metacharacter for a literal (`-F`) match.
fn escape_fixed(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    for ch in pattern.chars() {
        if matches!(
            ch,
            '.' | '*' | '+' | '?' | '^' | '$' | '{' | '}' | '(' | ')' | '|' | '[' | ']' | '\\'
        ) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Convert `(?P<name>` to `(?<name>` (upstream-style translated pattern).
fn convert_named_groups(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    let bytes: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '('
            && i + 2 < bytes.len()
            && bytes[i + 1] == '?'
            && bytes[i + 2] == 'P'
            && i + 3 < bytes.len()
            && bytes[i + 3] == '<'
        {
            out.push_str("(?<");
            i += 4;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Convert `(?<name>` back to `(?P<name>` for the Rust regex crate.
fn named_groups_for_rust(pattern: &str) -> String {
    let bytes: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '('
            && i + 2 < bytes.len()
            && bytes[i + 1] == '?'
            && bytes[i + 2] == '<'
            && i + 3 < bytes.len()
            && bytes[i + 3] != '='
            && bytes[i + 3] != '!'
        {
            out.push_str("(?P<");
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

const POSIX_CLASSES: &[(&str, &str)] = &[
    ("alpha", "a-zA-Z"),
    ("digit", "0-9"),
    ("alnum", "a-zA-Z0-9"),
    ("lower", "a-z"),
    ("upper", "A-Z"),
    ("xdigit", "0-9A-Fa-f"),
    ("space", " \\t\\n\\r\\f\\v"),
    ("blank", " \\t"),
    ("punct", "!-/:-@\\[-`{-~"),
    ("graph", "!-~"),
    ("print", " -~"),
    ("cntrl", "\\x00-\\x1F\\x7F"),
    ("ascii", "\\x00-\\x7F"),
    ("word", "a-zA-Z0-9_"),
];

fn posix_class(name: &str) -> Option<&'static str> {
    POSIX_CLASSES
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| *value)
}

/// Port of upstream `transformPosixCharacterClasses`.
fn transform_posix_character_classes(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut result = String::new();
    let mut i = 0;
    while i < chars.len() {
        if slice_eq(&chars, i, "[[:<:]]") {
            result.push_str("\\b");
            i += 7;
            continue;
        }
        if slice_eq(&chars, i, "[[:>:]]") {
            result.push_str("\\b");
            i += 7;
            continue;
        }
        if chars[i] == '[' {
            let mut bracket = String::from("[");
            i += 1;
            if i < chars.len() && (chars[i] == '^' || chars[i] == '!') {
                bracket.push('^');
                i += 1;
            }
            if i < chars.len() && chars[i] == ']' {
                bracket.push_str("\\]");
                i += 1;
            }
            while i < chars.len() && chars[i] != ']' {
                if chars[i] == '[' && i + 1 < chars.len() && chars[i + 1] == ':' {
                    if let Some(close) = find_subseq(&chars, i + 2, ":]") {
                        let name: String = chars[i + 2..close].iter().collect();
                        if let Some(replacement) = posix_class(&name) {
                            bracket.push_str(replacement);
                            i = close + 2;
                            continue;
                        }
                    }
                }
                if chars[i] == '\\' && i + 1 < chars.len() {
                    bracket.push(chars[i]);
                    bracket.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                bracket.push(chars[i]);
                i += 1;
            }
            if i < chars.len() && chars[i] == ']' {
                bracket.push(']');
                i += 1;
            }
            result.push_str(&bracket);
            continue;
        }
        if chars[i] == '\\' && i + 1 < chars.len() {
            result.push(chars[i]);
            result.push(chars[i + 1]);
            i += 2;
            continue;
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

fn slice_eq(chars: &[char], start: usize, needle: &str) -> bool {
    let needle_chars: Vec<char> = needle.chars().collect();
    if start + needle_chars.len() > chars.len() {
        return false;
    }
    chars[start..start + needle_chars.len()] == needle_chars[..]
}

fn find_subseq(chars: &[char], from: usize, needle: &str) -> Option<usize> {
    let needle_chars: Vec<char> = needle.chars().collect();
    if needle_chars.is_empty() || from >= chars.len() {
        return None;
    }
    let mut i = from;
    while i + needle_chars.len() <= chars.len() {
        if chars[i..i + needle_chars.len()] == needle_chars[..] {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Port of upstream `escapeRegexForBasicGrep` (BRE -> JS/Rust regex).
fn escape_regex_for_basic_grep(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut result = String::new();
    let mut i = 0;
    let mut at_pattern_start = true;

    while i < chars.len() {
        let ch = chars[i];

        if ch == '[' {
            result.push(ch);
            i += 1;
            if i < chars.len() && (chars[i] == '^' || chars[i] == '!') {
                result.push(chars[i]);
                i += 1;
            }
            if i < chars.len() && chars[i] == ']' {
                result.push(chars[i]);
                i += 1;
            }
            while i < chars.len() && chars[i] != ']' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    result.push(chars[i]);
                    result.push(chars[i + 1]);
                    i += 2;
                } else {
                    result.push(chars[i]);
                    i += 1;
                }
            }
            if i < chars.len() && chars[i] == ']' {
                result.push(chars[i]);
                i += 1;
            }
            at_pattern_start = false;
            continue;
        }

        if ch == '\\' && i + 1 < chars.len() {
            let next = chars[i + 1];
            if next == '|' {
                result.push('|');
                i += 2;
                at_pattern_start = true;
                continue;
            }
            if next == '(' {
                result.push('(');
                i += 2;
                at_pattern_start = true;
                continue;
            }
            if next == ')' {
                result.push(')');
                i += 2;
                at_pattern_start = false;
                continue;
            }
            if next == '{' {
                if let Some((consumed, replacement)) = basic_interval(&chars, i) {
                    result.push_str(&replacement);
                    i += consumed;
                    at_pattern_start = false;
                    continue;
                }
                result.push_str("\\{");
                i += 2;
                at_pattern_start = false;
                continue;
            }
            if next == '}' {
                result.push_str("\\}");
                i += 2;
                at_pattern_start = false;
                continue;
            }
            result.push(ch);
            result.push(next);
            i += 2;
            at_pattern_start = false;
            continue;
        }

        if ch == '*' && at_pattern_start {
            result.push_str("\\*");
            i += 1;
            continue;
        }

        if ch == '^' {
            if at_pattern_start {
                result.push('^');
                i += 1;
                continue;
            }
            result.push_str("\\^");
            i += 1;
            continue;
        }

        if ch == '$' {
            let is_at_end = i == chars.len() - 1;
            let is_before_group_end =
                i + 2 < chars.len() && chars[i + 1] == '\\' && chars[i + 2] == ')';
            if is_at_end || is_before_group_end {
                result.push('$');
            } else {
                result.push_str("\\$");
            }
            i += 1;
            at_pattern_start = false;
            continue;
        }

        if matches!(ch, '+' | '?' | '|' | '(' | ')' | '{' | '}') {
            result.push('\\');
            result.push(ch);
        } else {
            result.push(ch);
        }
        i += 1;
        at_pattern_start = false;
    }

    result
}

/// Match a BRE interval `\{n\}`, `\{n,\}`, `\{n,m\}` starting at `i`.
fn basic_interval(chars: &[char], i: usize) -> Option<(usize, String)> {
    // Expect: \ { digits [ , digits? ]? \ }
    let mut j = i + 2; // skip "\{"
    let start = j;
    while j < chars.len() && chars[j].is_ascii_digit() {
        j += 1;
    }
    if j == start {
        return None; // must start with a digit
    }
    let min: String = chars[start..j].iter().collect();
    let mut has_comma = false;
    let mut max = String::new();
    if j < chars.len() && chars[j] == ',' {
        has_comma = true;
        j += 1;
        let max_start = j;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
        max = chars[max_start..j].iter().collect();
    }
    // Expect closing \}
    if j + 1 < chars.len() && chars[j] == '\\' && chars[j + 1] == '}' {
        j += 2;
        let replacement = if has_comma {
            format!("{{{min},{max}}}")
        } else {
            format!("{{{min}}}")
        };
        Some((j - i, replacement))
    } else {
        None
    }
}

/// Port of upstream `extractPreFilter`.
fn extract_pre_filter(js_pattern: &str, ignore_case: bool) -> Option<PreFilter> {
    let mut core = js_pattern;

    if core.starts_with("\\b(?:") && core.ends_with(")\\b") {
        core = &core["\\b(?:".len()..core.len() - ")\\b".len()];
    } else if core.starts_with("\\b") && core.ends_with("\\b") && core.chars().count() >= 4 {
        // Strip the leading and trailing `\b` (two chars each).
        let chars: Vec<char> = core.chars().collect();
        let inner: String = chars[2..chars.len() - 2].iter().collect();
        return extract_pre_filter_core(&inner, ignore_case);
    }

    extract_pre_filter_core(core, ignore_case)
}

fn extract_pre_filter_core(core: &str, ignore_case: bool) -> Option<PreFilter> {
    if core.is_empty() {
        return None;
    }
    let alternatives = split_top_level_alternation(core)?;
    let mut needles = Vec::new();
    for alt in &alternatives {
        let literal = literal_from_alternative(alt)?;
        if literal.is_empty() {
            return None;
        }
        needles.push(literal);
    }
    if needles.is_empty() {
        return None;
    }
    let needles = if ignore_case {
        needles.iter().map(|n| n.to_lowercase()).collect()
    } else {
        needles
    };
    Some(PreFilter {
        needles,
        ignore_case,
    })
}

/// Port of upstream `splitTopLevelAlternation`.
fn split_top_level_alternation(pattern: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut in_class = false;
    let mut last = 0usize;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            i += 2;
            continue;
        }
        if in_class {
            if c == ']' {
                in_class = false;
            }
            i += 1;
            continue;
        }
        if c == '[' {
            in_class = true;
        } else if c == '(' {
            depth += 1;
        } else if c == ')' {
            depth -= 1;
            if depth < 0 {
                return None;
            }
        } else if c == '|' && depth == 0 {
            parts.push(chars[last..i].iter().collect());
            last = i + 1;
        }
        i += 1;
    }
    if depth != 0 || in_class {
        return None;
    }
    parts.push(chars[last..].iter().collect());
    Some(parts)
}

/// Port of upstream `literalFromAlternative`.
fn literal_from_alternative(alt: &str) -> Option<String> {
    let mut inner: Vec<char> = alt.chars().collect();

    if inner.first() == Some(&'^') {
        inner.remove(0);
    }
    if inner.last() == Some(&'$') {
        // $ is an anchor iff the run of trailing backslashes before it is even.
        let mut bs = 0;
        let mut k = inner.len() as isize - 2;
        while k >= 0 && inner[k as usize] == '\\' {
            bs += 1;
            k -= 1;
        }
        if bs % 2 == 0 {
            inner.pop();
        }
    }
    if inner.is_empty() {
        return None;
    }

    let mut out = String::new();
    let mut i = 0;
    while i < inner.len() {
        let c = inner[i];
        if c == '\\' {
            let next = inner.get(i + 1).copied()?;
            if matches!(
                next,
                'd' | 'D'
                    | 'w'
                    | 'W'
                    | 's'
                    | 'S'
                    | 'b'
                    | 'B'
                    | 'A'
                    | 'Z'
                    | 'z'
                    | 'G'
                    | 'Q'
                    | 'E'
                    | 'c'
                    | 'k'
                    | 'p'
                    | 'P'
                    | 'N'
                    | 'X'
                    | 'R'
                    | 'x'
                    | 'u'
                    | 'U'
                    | '0'..='9'
            ) {
                return None;
            }
            match next {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'f' => out.push('\u{0c}'),
                'v' => out.push('\u{0b}'),
                other => out.push(other),
            }
            i += 2;
            continue;
        }
        if matches!(
            c,
            '.' | '*' | '+' | '?' | '^' | '$' | '{' | '}' | '(' | ')' | '|' | '[' | ']'
        ) {
            return None;
        }
        out.push(c);
        i += 1;
    }
    Some(out)
}

/// Substring fast-path check: at least one needle present in `line`.
fn pre_filter_matches(pre_filter: &PreFilter, line: &str) -> bool {
    if pre_filter.ignore_case {
        let haystack = line.to_lowercase();
        pre_filter.needles.iter().any(|n| haystack.contains(n))
    } else {
        pre_filter.needles.iter().any(|n| line.contains(n))
    }
}

/// Options accepted by [`search_content`], subset mirroring upstream
/// `SearchOptions` for the behaviour ported here.
#[derive(Default, Clone)]
pub struct SearchOptions {
    pub invert_match: bool,
    pub show_line_numbers: bool,
    pub only_matching: bool,
    pub multiline: bool,
    pub replace: Option<String>,
    pub pre_filter: Option<PreFilter>,
}

/// Result of [`search_content`], mirroring upstream `SearchResult`.
#[derive(Debug, PartialEq, Eq)]
pub struct SearchResult {
    pub output: String,
    pub matched: bool,
    pub match_count: usize,
}

/// Port of upstream `searchContent` (single-line and multiline paths) for the
/// behaviour covered by the colocated matcher suite.
pub fn search_content(content: &str, regex: &Regex, options: &SearchOptions) -> SearchResult {
    if options.multiline {
        return search_content_multiline(content, regex, options);
    }

    let lines: Vec<&str> = content.split('\n').collect();
    let line_count = lines.len();
    let last_idx = if line_count > 0 && lines[line_count - 1].is_empty() {
        line_count - 1
    } else {
        line_count
    };

    let mut output_lines: Vec<String> = Vec::new();
    let mut has_match = false;
    let mut match_count = 0usize;

    for (i, &line) in lines.iter().enumerate().take(last_idx) {
        let first_match = if options
            .pre_filter
            .as_ref()
            .is_some_and(|pf| !pre_filter_matches(pf, line))
        {
            None
        } else {
            regex.captures(line)
        };
        let matches = first_match.is_some();

        if matches != options.invert_match {
            has_match = true;
            match_count += 1;
            if options.only_matching {
                for caps in regex.captures_iter(line) {
                    let m = caps.get(0).expect("group 0 always present");
                    let match_text = match &options.replace {
                        Some(replacement) => apply_replacement(replacement, &caps),
                        None => m.as_str().to_string(),
                    };
                    let mut prefix = String::new();
                    if options.show_line_numbers {
                        prefix.push_str(&format!("{}:", i + 1));
                    }
                    output_lines.push(prefix + &match_text);
                }
            } else {
                let output_line = match &options.replace {
                    Some(replacement) => apply_inline_replacement(regex, line, replacement),
                    None => line.to_string(),
                };
                let mut prefix = String::new();
                if options.show_line_numbers {
                    prefix.push_str(&format!("{}:", i + 1));
                }
                output_lines.push(prefix + &output_line);
            }
        }
    }

    SearchResult {
        output: if output_lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", output_lines.join("\n"))
        },
        matched: has_match,
        match_count,
    }
}

fn search_content_multiline(content: &str, regex: &Regex, options: &SearchOptions) -> SearchResult {
    // File-level pre-filter: if no needle appears anywhere and not inverting,
    // no line can match.
    if !options.invert_match {
        if let Some(pf) = &options.pre_filter {
            if !pre_filter_matches(pf, content) {
                return SearchResult {
                    output: String::new(),
                    matched: false,
                    match_count: 0,
                };
            }
        }
    }

    let lines: Vec<&str> = content.split('\n').collect();
    let line_count = lines.len();
    let last_idx = if line_count > 0 && lines[line_count - 1].is_empty() {
        line_count - 1
    } else {
        line_count
    };

    // Byte offset where each line starts.
    let mut line_offsets = vec![0usize];
    for (idx, byte) in content.bytes().enumerate() {
        if byte == b'\n' {
            line_offsets.push(idx + 1);
        }
    }
    let line_index = |byte_offset: usize| -> usize {
        let mut line = 0;
        for (i, &start) in line_offsets.iter().enumerate() {
            if start > byte_offset {
                break;
            }
            line = i;
        }
        line
    };

    struct Span {
        start_line: usize,
        end_line: usize,
    }
    let mut spans: Vec<Span> = Vec::new();
    for m in regex.find_iter(content) {
        let start_line = line_index(m.start());
        let end_line = line_index(m.start() + m.as_str().len().saturating_sub(1));
        spans.push(Span {
            start_line,
            end_line,
        });
    }

    if options.invert_match {
        let mut matched_lines = std::collections::HashSet::new();
        for span in &spans {
            for i in span.start_line..=span.end_line {
                matched_lines.insert(i);
            }
        }
        let mut output_lines = Vec::new();
        for (i, &line) in lines.iter().enumerate().take(last_idx) {
            if !matched_lines.contains(&i) {
                let mut out = String::new();
                if options.show_line_numbers {
                    out.push_str(&format!("{}:", i + 1));
                }
                out.push_str(line);
                output_lines.push(out);
            }
        }
        return SearchResult {
            output: if output_lines.is_empty() {
                String::new()
            } else {
                format!("{}\n", output_lines.join("\n"))
            },
            matched: !output_lines.is_empty(),
            match_count: output_lines.len(),
        };
    }

    if spans.is_empty() {
        return SearchResult {
            output: String::new(),
            matched: false,
            match_count: 0,
        };
    }

    let mut printed = std::collections::HashSet::new();
    let mut last_printed: isize = -1;
    let mut output_lines: Vec<String> = Vec::new();
    for span in &spans {
        let context_start = span.start_line;
        if last_printed >= 0 && context_start as isize > last_printed + 1 {
            output_lines.push("--".to_string());
        }
        #[allow(clippy::needless_range_loop)]
        for i in span.start_line..=span.end_line.min(last_idx.saturating_sub(1)) {
            if i >= last_idx {
                break;
            }
            if !printed.contains(&i) {
                printed.insert(i);
                last_printed = i as isize;
                let mut out = String::new();
                if options.show_line_numbers {
                    out.push_str(&format!("{}:", i + 1));
                }
                out.push_str(lines[i]);
                output_lines.push(out);
            }
        }
    }

    SearchResult {
        output: if output_lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", output_lines.join("\n"))
        },
        matched: true,
        match_count: spans.len(),
    }
}

/// Port of upstream `applyReplacement`: substitute `$&`, `$<n>`, `$<name>`.
fn apply_replacement(replacement: &str, caps: &regex::Captures) -> String {
    let chars: Vec<char> = replacement.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' && i + 1 < chars.len() {
            let next = chars[i + 1];
            if next == '&' {
                out.push_str(caps.get(0).map_or("", |m| m.as_str()));
                i += 2;
                continue;
            }
            if next == '<' {
                if let Some(close) = chars[i + 2..].iter().position(|&c| c == '>') {
                    let name: String = chars[i + 2..i + 2 + close].iter().collect();
                    out.push_str(caps.name(&name).map_or("", |m| m.as_str()));
                    i += 2 + close + 1;
                    continue;
                }
            }
            if next.is_ascii_digit() {
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_ascii_digit() {
                    j += 1;
                }
                let num: usize = chars[i + 1..j]
                    .iter()
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0);
                out.push_str(caps.get(num).map_or("", |m| m.as_str()));
                i = j;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Inline replacement on the whole line (skipping empty matches), mirroring the
/// upstream `regex.replace(line, replacer)` behaviour for the non-only-matching
/// path.
fn apply_inline_replacement(regex: &Regex, line: &str, replacement: &str) -> String {
    let mut out = String::new();
    let mut last_end = 0;
    for caps in regex.captures_iter(line) {
        let m = caps.get(0).expect("group 0 always present");
        if m.as_str().is_empty() {
            continue;
        }
        out.push_str(&line[last_end..m.start()]);
        out.push_str(&apply_replacement(replacement, &caps));
        last_end = m.end();
    }
    out.push_str(&line[last_end..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basic(pattern: &str) -> RegexResult {
        build_regex(pattern, RegexOptions::new(RegexMode::Basic))
    }
    fn extended(pattern: &str) -> RegexResult {
        build_regex(pattern, RegexOptions::new(RegexMode::Extended))
    }

    fn pf(needles: &[&str], ignore_case: bool) -> Option<PreFilter> {
        Some(PreFilter {
            needles: needles.iter().map(|s| s.to_string()).collect(),
            ignore_case,
        })
    }

    // ---- matcher.test.ts: preFilterMatches — substring fast-path -----------

    #[test]
    fn search_engine_matcher_prefilter_skips_lines_without_needle_case_sensitive() {
        let content = "hello world\nfoo bar\nhello foo\n";
        let RegexResult { regex, .. } = basic("foo");
        let result = search_content(
            content,
            &regex,
            &SearchOptions {
                pre_filter: pf(&["foo"], false),
                ..Default::default()
            },
        );
        assert_eq!(result.output, "foo bar\nhello foo\n");
        assert_eq!(result.match_count, 2);
    }

    #[test]
    fn search_engine_matcher_prefilter_lowercases_needle_and_line_when_ignore_case() {
        let content = "FOO\nfoo\nbar\n";
        let RegexResult { regex, .. } = build_regex(
            "foo",
            RegexOptions {
                ignore_case: true,
                ..RegexOptions::new(RegexMode::Basic)
            },
        );
        let result = search_content(
            content,
            &regex,
            &SearchOptions {
                pre_filter: pf(&["foo"], true),
                ..Default::default()
            },
        );
        assert_eq!(result.output, "FOO\nfoo\n");
        assert_eq!(result.match_count, 2);
    }

    #[test]
    fn search_engine_matcher_prefilter_or_logic_across_multiple_needles() {
        let content = "alpha\nbeta\ngamma\ndelta\n";
        let RegexResult { regex, .. } = basic("alpha\\|delta");
        let result = search_content(
            content,
            &regex,
            &SearchOptions {
                pre_filter: pf(&["alpha", "delta"], false),
                ..Default::default()
            },
        );
        assert_eq!(result.output, "alpha\ndelta\n");
        assert_eq!(result.match_count, 2);
    }

    #[test]
    fn search_engine_matcher_prefilter_invert_outputs_non_needle_lines() {
        let content = "foo\nbar\nbaz\n";
        let RegexResult { regex, .. } = basic("foo");
        let result = search_content(
            content,
            &regex,
            &SearchOptions {
                pre_filter: pf(&["foo"], false),
                invert_match: true,
                ..Default::default()
            },
        );
        assert_eq!(result.output, "bar\nbaz\n");
        assert_eq!(result.match_count, 2);
    }

    #[test]
    fn search_engine_matcher_no_fast_path_skip_when_prefilter_absent() {
        let content = "alpha\nbeta\n";
        let RegexResult { regex, .. } = basic("alpha");
        let result = search_content(content, &regex, &SearchOptions::default());
        assert_eq!(result.output, "alpha\n");
        assert_eq!(result.match_count, 1);
    }

    // ---- matcher.test.ts: applyReplacement — token substitution ------------

    #[test]
    fn search_engine_matcher_replacement_full_match_token() {
        let RegexResult { regex, .. } = basic("foo");
        let result = search_content(
            "foo bar\n",
            &regex,
            &SearchOptions {
                replace: Some("[$&]".to_string()),
                only_matching: true,
                ..Default::default()
            },
        );
        assert_eq!(result.output, "[foo]\n");
    }

    #[test]
    fn search_engine_matcher_replacement_numbered_capture_groups() {
        let RegexResult { regex, .. } = extended("(\\w+)@(\\w+)");
        let result = search_content(
            "user@host\n",
            &regex,
            &SearchOptions {
                replace: Some("$2/$1".to_string()),
                only_matching: true,
                ..Default::default()
            },
        );
        assert_eq!(result.output, "host/user\n");
    }

    #[test]
    fn search_engine_matcher_replacement_named_capture_groups() {
        let RegexResult { regex, .. } = build_regex(
            "(?P<user>\\w+)@(?P<host>\\w+)",
            RegexOptions::new(RegexMode::Perl),
        );
        let result = search_content(
            "alice@example\n",
            &regex,
            &SearchOptions {
                replace: Some("$<host>/$<user>".to_string()),
                only_matching: true,
                ..Default::default()
            },
        );
        assert_eq!(result.output, "example/alice\n");
    }

    #[test]
    fn search_engine_matcher_replacement_missing_capture_group_is_empty() {
        let RegexResult { regex, .. } = extended("(foo)(bar)?");
        let result = search_content(
            "foo\n",
            &regex,
            &SearchOptions {
                replace: Some("$1-$2".to_string()),
                only_matching: true,
                ..Default::default()
            },
        );
        assert_eq!(result.output, "foo-\n");
    }

    #[test]
    fn search_engine_matcher_replacement_inline_on_full_line() {
        let RegexResult { regex, .. } = basic("world");
        let result = search_content(
            "hello world\n",
            &regex,
            &SearchOptions {
                replace: Some("WORLD".to_string()),
                ..Default::default()
            },
        );
        assert_eq!(result.output, "hello WORLD\n");
    }

    // ---- matcher.test.ts: searchContentMultiline — file-level preFilter ----

    #[test]
    fn search_engine_matcher_multiline_empty_when_no_needle_in_content() {
        let content: String = (0..500)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let RegexResult { regex, pre_filter } =
            build_regex("^def \\|^async def ", RegexOptions::new(RegexMode::Basic));
        let result = search_content(
            &content,
            &regex,
            &SearchOptions {
                multiline: true,
                pre_filter,
                ..Default::default()
            },
        );
        assert!(!result.matched);
        assert_eq!(result.output, "");
    }

    #[test]
    fn search_engine_matcher_multiline_finds_matches_with_line_numbers() {
        let content = "class Foo:\n    pass\ndef bar():\n    pass\n";
        let RegexResult { regex, pre_filter } = build_regex(
            "^def \\|^class ",
            RegexOptions {
                multiline: true,
                ..RegexOptions::new(RegexMode::Basic)
            },
        );
        let result = search_content(
            content,
            &regex,
            &SearchOptions {
                multiline: true,
                pre_filter,
                show_line_numbers: true,
                ..Default::default()
            },
        );
        assert!(result.matched);
        assert_eq!(result.output, "1:class Foo:\n--\n3:def bar():\n");
    }

    #[test]
    fn search_engine_matcher_multiline_invert_does_not_skip_when_needle_absent() {
        let content = "hello\nworld\n";
        let RegexResult { regex, pre_filter } =
            build_regex("^def ", RegexOptions::new(RegexMode::Basic));
        let result = search_content(
            content,
            &regex,
            &SearchOptions {
                multiline: true,
                pre_filter,
                invert_match: true,
                show_line_numbers: true,
                ..Default::default()
            },
        );
        assert!(result.matched);
        assert_eq!(result.output, "1:hello\n2:world\n");
    }

    // ---- regex.test.ts: buildRegex preFilter — happy path ------------------

    fn opt(mode: RegexMode, ignore_case: bool, whole_word: bool) -> RegexOptions {
        RegexOptions {
            mode,
            ignore_case,
            whole_word,
            line_regexp: false,
            multiline: false,
        }
    }

    #[test]
    fn search_engine_regex_prefilter_bare_literal_basic() {
        assert_eq!(basic("interface").pre_filter, pf(&["interface"], false));
    }

    #[test]
    fn search_engine_regex_prefilter_strips_w_wrapper_single_literal() {
        let r = build_regex("type", opt(RegexMode::Basic, false, true));
        assert_eq!(r.pre_filter, pf(&["type"], false));
    }

    #[test]
    fn search_engine_regex_prefilter_strips_w_wrapper_around_alternation() {
        let r = build_regex("foo|bar", opt(RegexMode::Extended, false, true));
        assert_eq!(r.pre_filter, pf(&["foo", "bar"], false));
    }

    #[test]
    fn search_engine_regex_prefilter_splits_top_level_alternation_extended() {
        assert_eq!(
            extended("interface|type").pre_filter,
            pf(&["interface", "type"], false)
        );
    }

    #[test]
    fn search_engine_regex_prefilter_lowercases_needles_when_ignore_case() {
        let r = build_regex("Async", opt(RegexMode::Basic, true, false));
        assert_eq!(r.pre_filter, pf(&["async"], true));
    }

    #[test]
    fn search_engine_regex_prefilter_fixed_strings_are_literal_needles() {
        let r = build_regex("Promise<T>", RegexOptions::new(RegexMode::Fixed));
        assert_eq!(r.pre_filter, pf(&["Promise<T>"], false));
    }

    #[test]
    fn search_engine_regex_prefilter_decodes_meta_escaped_by_fixed() {
        let r = build_regex("a.b", RegexOptions::new(RegexMode::Fixed));
        assert_eq!(r.pre_filter, pf(&["a.b"], false));
    }

    #[test]
    fn search_engine_regex_prefilter_decodes_n_t_r_escapes() {
        assert_eq!(extended("foo\\nbar").pre_filter, pf(&["foo\nbar"], false));
    }

    #[test]
    fn search_engine_regex_prefilter_leading_anchored_literal() {
        assert_eq!(extended("^foo").pre_filter, pf(&["foo"], false));
    }

    #[test]
    fn search_engine_regex_prefilter_trailing_anchored_literal() {
        assert_eq!(extended("foo$").pre_filter, pf(&["foo"], false));
    }

    #[test]
    fn search_engine_regex_prefilter_fully_anchored_literal() {
        assert_eq!(extended("^foo$").pre_filter, pf(&["foo"], false));
    }

    #[test]
    fn search_engine_regex_prefilter_anchored_alternation_issue_case() {
        assert_eq!(
            basic("^def \\|^async def ").pre_filter,
            pf(&["def ", "async def "], false)
        );
    }

    #[test]
    fn search_engine_regex_prefilter_mixed_anchored_unanchored_alternation() {
        assert_eq!(
            extended("^foo|bar|baz$").pre_filter,
            pf(&["foo", "bar", "baz"], false)
        );
    }

    #[test]
    fn search_engine_regex_prefilter_preserves_escaped_dollar() {
        assert_eq!(extended("foo\\$").pre_filter, pf(&["foo$"], false));
    }

    // ---- regex.test.ts: buildRegex preFilter — safety (must NOT extract) ---

    #[test]
    fn search_engine_regex_prefilter_rejects_quantifier_plus() {
        assert_eq!(extended("a+").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_rejects_quantifier_star() {
        assert_eq!(extended("a*").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_rejects_quantifier_question() {
        assert_eq!(extended("a?").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_rejects_quantifier_braces() {
        assert_eq!(extended("a{2,4}").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_rejects_character_class() {
        assert_eq!(extended("[abc]").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_rejects_dot_basic() {
        assert_eq!(basic("a.b").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_rejects_digit_class() {
        assert_eq!(extended("\\d").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_rejects_word_class() {
        assert_eq!(extended("\\w+").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_rejects_whitespace_class() {
        assert_eq!(extended("\\s").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_rejects_bare_word_boundary() {
        assert_eq!(extended("\\bfoo").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_rejects_capturing_group() {
        assert_eq!(extended("(foo)").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_rejects_non_capturing_group_without_frame() {
        assert_eq!(extended("(?:foo)").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_rejects_alternation_branch_with_meta() {
        assert_eq!(extended("foo|bar+").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_rejects_nested_alternation_in_group() {
        assert_eq!(extended("foo(bar|baz)").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_rejects_hex_escape() {
        assert_eq!(extended("\\x41").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_rejects_unicode_escape() {
        assert_eq!(extended("\\u2764").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_rejects_bare_caret() {
        assert_eq!(extended("^").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_rejects_bare_dollar() {
        assert_eq!(extended("$").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_rejects_caret_dollar_pair() {
        assert_eq!(extended("^$").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_rejects_one_branch_without_needle() {
        assert_eq!(extended("^a|$").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_rejects_mid_alternative_caret() {
        assert_eq!(extended("foo^bar").pre_filter, None);
    }

    // ---- regex.test.ts: buildRegex preFilter — structural correctness ------

    #[test]
    fn search_engine_regex_prefilter_does_not_split_in_non_capturing_group() {
        assert_eq!(extended("(?:a|b)c").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_does_not_split_in_character_class() {
        assert_eq!(extended("[a|b]").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_preserves_alternation_ordering() {
        assert_eq!(
            extended("alpha|beta|gamma").pre_filter,
            pf(&["alpha", "beta", "gamma"], false)
        );
    }

    #[test]
    fn search_engine_regex_prefilter_does_not_over_strip_leading_word_boundary() {
        assert_eq!(extended("\\bfoo").pre_filter, None);
    }

    #[test]
    fn search_engine_regex_prefilter_undefined_for_empty_pattern() {
        assert_eq!(extended("").pre_filter, None);
    }
}
