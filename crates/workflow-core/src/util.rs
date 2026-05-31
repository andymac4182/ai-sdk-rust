#[must_use]
pub fn pluralize<'a>(singular: &'a str, plural: &'a str, count: usize) -> &'a str {
    if count == 1 { singular } else { plural }
}

/// Builds the workflow suspension log message used when queue work is enqueued.
#[must_use]
pub fn build_workflow_suspension_message(
    step_count: usize,
    hook_count: usize,
    wait_count: usize,
) -> Option<String> {
    if step_count == 0 && hook_count == 0 && wait_count == 0 {
        return None;
    }

    let mut parts = Vec::new();
    if step_count > 0 {
        parts.push(format!(
            "{step_count} {}",
            pluralize("step", "steps", step_count)
        ));
    }
    if hook_count > 0 {
        parts.push(format!(
            "{hook_count} {}",
            pluralize("hook", "hooks", hook_count)
        ));
    }
    if wait_count > 0 {
        parts.push(format!(
            "{wait_count} {}",
            pluralize("timer", "timers", wait_count)
        ));
    }

    let mut resume_parts = Vec::new();
    if step_count > 0 {
        resume_parts.push("steps are completed");
    }
    if hook_count > 0 {
        resume_parts.push("hooks are received");
    }
    if wait_count > 0 {
        resume_parts.push("timers have elapsed");
    }

    Some(format!(
        "{} to be enqueued\n  Workflow will suspend and resume when {}",
        parts.join(" and "),
        resume_parts.join(" and ")
    ))
}

/// Generate a user stream ID for a workflow run.
#[must_use]
pub fn get_workflow_run_stream_id(run_id: &str, namespace: Option<&str>) -> String {
    let stream_id = run_id.replacen("wrun_", "strm_", 1);
    let stream_id = format!("{stream_id}_user");
    match namespace {
        Some(namespace) if !namespace.is_empty() => {
            format!("{stream_id}_{}", base64_url_no_pad(namespace.as_bytes()))
        }
        _ => stream_id,
    }
}

#[must_use]
pub fn get_abort_stream_id(id: &str) -> String {
    format!("strm_{id}_system_abort")
}

pub fn get_abort_stream_id_from_token(hook_token: &str) -> Result<String, String> {
    let id = hook_token.strip_prefix("abrt_").ok_or_else(|| {
        format!("Invalid abort hook token format: expected \"abrt_\" prefix, got \"{hook_token}\"")
    })?;
    Ok(get_abort_stream_id(id))
}

fn base64_url_no_pad(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);

    let mut chunks = bytes.chunks_exact(3);
    for chunk in &mut chunks {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]);
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        out.push(ALPHABET[(n & 0x3f) as usize] as char);
    }

    match chunks.remainder() {
        [one] => {
            let n = u32::from(*one) << 16;
            out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
            out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        }
        [one, two] => {
            let n = (u32::from(*one) << 16) | (u32::from(*two) << 8);
            out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
            out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
            out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        }
        [] => {}
        _ => unreachable!("base64 chunks_exact(3) remainder is shorter than 3"),
    }

    out
}
