pub fn sanitize_error_message(message: &str) -> String {
    sanitize_error_message_with_options(message, false)
}

/// Aggressively sanitizes host/bootstrap error text.
pub fn sanitize_host_error_message(message: &str) -> String {
    sanitize_error_message_with_options(message, true)
}

fn sanitize_error_message_with_options(message: &str, aggressive: bool) -> String {
    if message.is_empty() {
        return String::new();
    }
    let mut kept_lines = Vec::new();
    for (index, line) in message.lines().enumerate() {
        if index > 0 && line.trim_start().starts_with("at ") {
            break;
        }
        kept_lines.push(line);
    }
    let mut sanitized = kept_lines.join("\n");

    if aggressive {
        sanitized = replace_prefixed_token(&sanitized, "file://", true);
        sanitized = replace_unc_paths(&sanitized);
    }
    sanitized = replace_node_internal_paths(&sanitized);
    for prefix in host_path_prefixes(aggressive) {
        sanitized = replace_prefixed_token(&sanitized, prefix, false);
    }
    sanitized = replace_windows_drive_paths(&sanitized);
    sanitized
}

fn host_path_prefixes(aggressive: bool) -> Vec<&'static str> {
    let mut prefixes = vec![
        "/Users/",
        "/home/",
        "/private/",
        "/var/",
        "/opt/",
        "/Library/",
        "/System/",
        "/usr/",
        "/etc/",
        "/tmp/",
        "/nix/",
        "/snap/",
    ];
    if aggressive {
        prefixes.extend(["/workspace/", "/root/", "/srv/", "/mnt/", "/app/"]);
    }
    prefixes
}

fn replace_prefixed_token(input: &str, prefix: &str, include_colon: bool) -> String {
    let mut output = String::with_capacity(input.len());
    let mut index = 0;
    while let Some(offset) = input[index..].find(prefix) {
        let start = index + offset;
        output.push_str(&input[index..start]);
        output.push_str("<path>");
        let mut end = start + prefix.len();
        while end < input.len() {
            let character = input[end..].chars().next().expect("valid char boundary");
            if character.is_whitespace()
                || matches!(character, '\'' | '"' | ',' | ')' | '}' | ']')
                || (!include_colon && character == ':')
            {
                break;
            }
            end += character.len_utf8();
        }
        index = end;
    }
    output.push_str(&input[index..]);
    output
}

fn replace_windows_drive_paths(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if index + 2 < input.len()
            && bytes[index].is_ascii_uppercase()
            && bytes[index + 1] == b':'
            && bytes[index + 2] == b'\\'
        {
            output.push_str("<path>");
            index += 3;
            while index < input.len() {
                let character = input[index..].chars().next().expect("valid char boundary");
                if character.is_whitespace()
                    || matches!(character, '\'' | '"' | ',' | ')' | '}' | ']')
                {
                    break;
                }
                index += character.len_utf8();
            }
        } else {
            let character = input[index..].chars().next().expect("valid char boundary");
            output.push(character);
            index += character.len_utf8();
        }
    }
    output
}

fn replace_node_internal_paths(input: &str) -> String {
    let prefix = "node:internal/";
    let mut output = String::with_capacity(input.len());
    let mut index = 0;
    while let Some(offset) = input[index..].find(prefix) {
        let start = index + offset;
        output.push_str(&input[index..start]);
        output.push_str("<internal>");
        let mut end = start + prefix.len();
        while end < input.len() {
            let character = input[end..].chars().next().expect("valid char boundary");
            if character.is_whitespace()
                || matches!(character, '\'' | '"' | ',' | ')' | '}' | ']' | ':')
            {
                break;
            }
            end += character.len_utf8();
        }
        index = end;
    }
    output.push_str(&input[index..]);
    output
}

fn replace_unc_paths(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut index = 0;
    while let Some(offset) = input[index..].find("\\\\") {
        let start = index + offset;
        output.push_str(&input[index..start]);
        output.push_str("<path>");
        let mut end = start + 2;
        while end < input.len() {
            let character = input[end..].chars().next().expect("valid char boundary");
            if character.is_whitespace() || matches!(character, '\'' | '"' | ',' | ')' | '}' | ']')
            {
                break;
            }
            end += character.len_utf8();
        }
        index = end;
    }
    output.push_str(&input[index..]);
    output
}
