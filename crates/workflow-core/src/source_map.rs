fn is_base64_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=')
}

fn extract_inline_source_map_base64(source: &str) -> Option<&str> {
    const SOURCE_MAP_PREFIX: &str = "//# sourceMappingURL=data:application/json";
    const BASE64_MARKER: &str = ";base64,";

    let mut scan_from = 0;
    while scan_from < source.len() {
        let prefix_idx = source[scan_from..].find(SOURCE_MAP_PREFIX)? + scan_from;
        let marker_search_start = prefix_idx + SOURCE_MAP_PREFIX.len();
        let Some(base64_start) = source[marker_search_start..]
            .find(BASE64_MARKER)
            .map(|offset| marker_search_start + offset)
        else {
            scan_from = marker_search_start;
            continue;
        };

        if let Some(comma_before) = source[prefix_idx..]
            .find(',')
            .map(|offset| prefix_idx + offset)
        {
            if comma_before < base64_start {
                scan_from = base64_start + BASE64_MARKER.len();
                continue;
            }
        }

        let value_start = base64_start + BASE64_MARKER.len();
        let value_end = source.as_bytes()[value_start..]
            .iter()
            .position(|byte| !is_base64_char(*byte))
            .map_or(source.len(), |offset| value_start + offset);

        if value_end > value_start {
            return Some(&source[value_start..value_end]);
        }
        scan_from = value_end;
    }

    None
}

/// Remaps an error stack using an inline source map when one is present.
///
/// This bucket ports the streaming inline-map extraction and failure behavior;
/// full VLQ mapping is intentionally left for the runtime/source-map bucket.
#[must_use]
pub fn remap_error_stack(stack: &str, _filename: &str, workflow_code: &str) -> String {
    let Some(base64) = extract_inline_source_map_base64(workflow_code) else {
        return stack.to_string();
    };

    if decode_base64(base64)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_none()
    {
        return stack.to_string();
    }

    stack.to_string()
}

fn decode_base64(input: &str) -> Result<Vec<u8>, ()> {
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0u8;

    for byte in input.bytes() {
        if byte == b'=' {
            break;
        }
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(()),
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push(((buffer >> bits) & 0xff) as u8);
        }
    }

    Ok(output)
}
