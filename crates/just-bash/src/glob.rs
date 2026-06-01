use std::collections::BTreeSet;

use crate::fs::VirtualFileSystem;
use crate::path::{join_path, normalize_path};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GlobOptions {
    pub ignore_case: bool,
    pub strip_quotes: bool,
    pub include_hidden: bool,
}

/// Matches one path against a glob pattern.
pub fn match_glob(name: &str, pattern: &str, options: GlobOptions) -> bool {
    let mut pattern = pattern;
    let stripped;
    if options.strip_quotes
        && ((pattern.starts_with('"') && pattern.ends_with('"'))
            || (pattern.starts_with('\'') && pattern.ends_with('\'')))
        && pattern.len() >= 2
    {
        stripped = pattern[1..pattern.len() - 1].to_string();
        pattern = &stripped;
    }

    let pattern_segments: Vec<&str> = pattern.split('/').filter(|part| !part.is_empty()).collect();
    let name_segments: Vec<&str> = name.split('/').filter(|part| !part.is_empty()).collect();
    glob_segments_match(&pattern_segments, &name_segments, options.ignore_case)
}

/// Expands a glob pattern against all virtual filesystem paths.
pub fn glob_paths(
    fs: &VirtualFileSystem,
    cwd: &str,
    pattern: &str,
    options: GlobOptions,
) -> Vec<String> {
    let base = normalize_path(cwd);
    let absolute_pattern = if pattern.starts_with('/') {
        normalize_path(pattern)
    } else {
        normalize_path(&join_path(&base, pattern))
    };
    let relative_pattern = if pattern.starts_with('/') {
        pattern.trim_start_matches('/').to_string()
    } else {
        pattern.to_string()
    };

    let mut matches = BTreeSet::new();
    for path in fs.get_all_paths() {
        if path == "/" || path == base {
            continue;
        }
        let relative_to_cwd = path
            .strip_prefix(&format!("{base}/"))
            .unwrap_or_else(|| path.trim_start_matches('/'));
        let absolute_match = match_glob(&path, &absolute_pattern, options);
        let relative_match = match_glob(relative_to_cwd, &relative_pattern, options);
        if absolute_match || relative_match {
            let basename = relative_to_cwd
                .rsplit('/')
                .next()
                .unwrap_or(relative_to_cwd);
            if !options.include_hidden && basename.starts_with('.') {
                continue;
            }
            matches.insert(if pattern.starts_with('/') {
                path
            } else {
                relative_to_cwd.to_string()
            });
        }
    }
    matches.into_iter().collect()
}

fn glob_segments_match(pattern: &[&str], path: &[&str], ignore_case: bool) -> bool {
    match (pattern.split_first(), path.split_first()) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some((&"**", rest)), None) => glob_segments_match(rest, path, ignore_case),
        (Some((&"**", rest)), Some((_, path_rest))) => {
            glob_segments_match(rest, path, ignore_case)
                || glob_segments_match(pattern, path_rest, ignore_case)
        }
        (Some((pattern_head, rest)), Some((path_head, path_rest))) => {
            glob_segment_matches(pattern_head, path_head, ignore_case)
                && glob_segments_match(rest, path_rest, ignore_case)
        }
        (Some(_), None) => false,
    }
}

fn glob_segment_matches(pattern: &str, segment: &str, ignore_case: bool) -> bool {
    let pattern_chars: Vec<char> = if ignore_case {
        pattern.to_ascii_lowercase().chars().collect()
    } else {
        pattern.chars().collect()
    };
    let segment_chars: Vec<char> = if ignore_case {
        segment.to_ascii_lowercase().chars().collect()
    } else {
        segment.chars().collect()
    };
    glob_segment_match_at(&pattern_chars, &segment_chars, 0, 0)
}

fn glob_segment_match_at(pattern: &[char], segment: &[char], pi: usize, si: usize) -> bool {
    if pi == pattern.len() {
        return si == segment.len();
    }
    match pattern[pi] {
        '*' => {
            glob_segment_match_at(pattern, segment, pi + 1, si)
                || (si < segment.len() && glob_segment_match_at(pattern, segment, pi, si + 1))
        }
        '?' => si < segment.len() && glob_segment_match_at(pattern, segment, pi + 1, si + 1),
        '[' => {
            let Some((matched, next_pi)) = match_char_class(pattern, segment.get(si).copied(), pi)
            else {
                return si < segment.len()
                    && pattern[pi] == segment[si]
                    && glob_segment_match_at(pattern, segment, pi + 1, si + 1);
            };
            matched && glob_segment_match_at(pattern, segment, next_pi, si + 1)
        }
        literal => {
            si < segment.len()
                && literal == segment[si]
                && glob_segment_match_at(pattern, segment, pi + 1, si + 1)
        }
    }
}

fn match_char_class(
    pattern: &[char],
    candidate: Option<char>,
    start: usize,
) -> Option<(bool, usize)> {
    let candidate = candidate?;
    let mut end = start + 1;
    while end < pattern.len() && pattern[end] != ']' {
        end += 1;
    }
    if end >= pattern.len() {
        return None;
    }
    let mut index = start + 1;
    let negated = matches!(pattern.get(index), Some('!' | '^'));
    if negated {
        index += 1;
    }
    let mut matched = false;
    while index < end {
        if index + 2 < end && pattern[index + 1] == '-' {
            let start_char = pattern[index];
            let end_char = pattern[index + 2];
            if start_char <= candidate && candidate <= end_char {
                matched = true;
            }
            index += 3;
        } else {
            if pattern[index] == candidate {
                matched = true;
            }
            index += 1;
        }
    }
    Some((if negated { !matched } else { matched }, end + 1))
}
