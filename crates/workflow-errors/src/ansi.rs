#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Explain<'a> {
    pub text: &'a str,
    pub explain: &'a str,
}

pub fn help(message: &str) -> String {
    format!("help: {message}")
}

pub fn hint(message: &str) -> String {
    format!("hint: {message}")
}

pub fn note(messages: &[&str]) -> String {
    format!("note: {}", messages.join("\n"))
}

pub fn docs(url: &str) -> String {
    format!("docs: {url}")
}

pub fn code(str: &str) -> String {
    format!("`{str}`")
}

pub fn dim(str: &str) -> String {
    str.to_owned()
}

pub fn bold(str: &str) -> String {
    str.to_owned()
}

pub fn red(str: &str) -> String {
    str.to_owned()
}

pub fn magenta(str: &str) -> String {
    str.to_owned()
}

pub fn frame(title: &str, contents: &[&str]) -> String {
    let mut result = vec![title.to_owned()];
    for (index, content) in contents.iter().enumerate() {
        let lines = content.split('\n').collect::<Vec<_>>();
        let is_last_content = index == contents.len() - 1;
        let first_line_prefix = if is_last_content {
            "╰▶ "
        } else {
            "├▶ "
        };
        let continuation_prefix = if is_last_content { "   " } else { "│  " };

        for (line_index, line) in lines.iter().enumerate() {
            let prefix = if line_index == 0 {
                first_line_prefix
            } else {
                continuation_prefix
            };
            result.push(format!("{prefix}{line}"));
        }
    }
    result.join("\n")
}

pub fn inline(text: &[&str], values: &[Explain<'_>]) -> String {
    let mut result_lines = Vec::new();
    let mut current_line = String::new();
    let mut current_line_visual_len = 0_usize;
    let mut pending_markers = Vec::new();

    for (index, segment) in text.iter().enumerate() {
        let lines = segment.split('\n').collect::<Vec<_>>();
        for (line_index, line) in lines.iter().enumerate() {
            if line_index > 0 {
                flush_line(
                    &mut result_lines,
                    &mut current_line,
                    &mut current_line_visual_len,
                    &mut pending_markers,
                );
            }
            current_line.push_str(line);
            current_line_visual_len += line.len();
        }

        if let Some(value) = values.get(index) {
            let start_col = current_line_visual_len;
            current_line.push_str(value.text);
            current_line_visual_len += value.text.len();
            pending_markers.push(Marker {
                start_col,
                end_col: current_line_visual_len,
                explain: value.explain.to_owned(),
            });
        }
    }

    if !current_line.is_empty() || !pending_markers.is_empty() {
        flush_line(
            &mut result_lines,
            &mut current_line,
            &mut current_line_visual_len,
            &mut pending_markers,
        );
    }

    result_lines.join("\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Marker {
    start_col: usize,
    end_col: usize,
    explain: String,
}

fn flush_line(
    result_lines: &mut Vec<String>,
    current_line: &mut String,
    current_line_visual_len: &mut usize,
    pending_markers: &mut Vec<Marker>,
) {
    result_lines.push(std::mem::take(current_line));
    if pending_markers.is_empty() {
        *current_line_visual_len = 0;
        return;
    }

    let marker_mids = pending_markers
        .iter()
        .map(get_marker_midpoint)
        .collect::<Vec<_>>();
    result_lines.push(build_underline(pending_markers));

    for (index, marker) in pending_markers.iter().enumerate() {
        result_lines.push(build_explanation_line(
            marker,
            marker_mids[index],
            &marker_mids[(index + 1)..],
            pending_markers.len() == 1,
        ));
    }

    pending_markers.clear();
    *current_line_visual_len = 0;
}

fn get_marker_midpoint(marker: &Marker) -> usize {
    let text_len = marker.end_col - marker.start_col;
    marker.start_col + (text_len / 2)
}

fn build_underline(markers: &[Marker]) -> String {
    let mut parts = String::new();
    let mut pos = 0;
    for marker in markers {
        let text_len = (marker.end_col - marker.start_col).max(1);
        let midpoint = text_len / 2;
        if marker.start_col > pos {
            parts.push_str(&" ".repeat(marker.start_col - pos));
            pos = marker.start_col;
        }
        parts.push_str(&"─".repeat(midpoint));
        parts.push('┬');
        parts.push_str(&"─".repeat(text_len.saturating_sub(midpoint + 1)));
        pos += text_len;
    }
    parts
}

fn build_explanation_line(
    marker: &Marker,
    mid_col: usize,
    remaining_mids: &[usize],
    is_only_marker: bool,
) -> String {
    let mut line = String::from("╰");
    let mut pos = mid_col + 1;
    for next_mid in remaining_mids {
        while pos < *next_mid {
            line.push('─');
            pos += 1;
        }
        line.push('┼');
        pos += 1;
    }

    let arrow = if is_only_marker { "▶ " } else { "─▶ " };
    line.push_str(arrow);
    line.push_str(&marker.explain);

    format!("{}{}", " ".repeat(mid_col), line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_ansi_cases() {
        assert_eq!(frame("something went wrong", &[]), "something went wrong");
        assert_eq!(
            frame("something went wrong", &["here is why"]),
            "something went wrong\n╰▶ here is why"
        );
        assert_eq!(
            frame("something went wrong", &["first reason", "second reason"]),
            "something went wrong\n├▶ first reason\n╰▶ second reason"
        );
        assert_eq!(
            frame("title", &["first\nwith two lines", "last\nalso two lines"]),
            "title\n├▶ first\n│  with two lines\n╰▶ last\n   also two lines"
        );
        assert_eq!(code("fn()"), "`fn()`");
        assert_eq!(hint("try reloading"), "hint: try reloading");
        assert_eq!(
            note(&["read more:", "https://example.com"]),
            "note: read more:\nhttps://example.com"
        );
        assert_eq!(
            help("run `wf inspect run run_123`"),
            "help: run `wf inspect run run_123`"
        );
        assert_eq!(
            docs("https://workflow-sdk.dev/docs/api-reference/workflow/sleep"),
            "docs: https://workflow-sdk.dev/docs/api-reference/workflow/sleep"
        );

        let out = inline(
            &["function ", "()"],
            &[Explain {
                text: "hello",
                explain: "name not allowed",
            }],
        );
        assert_eq!(
            out,
            "function hello()\n         ──┬──\n           ╰▶ name not allowed"
        );

        let out = inline(
            &["const ", " = 1\nconst y = 2"],
            &[Explain {
                text: "x",
                explain: "unused",
            }],
        );
        assert_eq!(out, "const x = 1\n      ┬\n      ╰▶ unused\nconst y = 2");
    }
}
