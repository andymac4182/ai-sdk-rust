use time::OffsetDateTime;

pub const DEFAULT_TIMESTAMP_THRESHOLD_PAST_MS: i64 = 24 * 60 * 60 * 1000;
pub const DEFAULT_TIMESTAMP_THRESHOLD_FUTURE_MS: i64 = 5 * 60 * 1000;
#[deprecated(note = "Use DEFAULT_TIMESTAMP_THRESHOLD_PAST_MS instead.")]
pub const DEFAULT_TIMESTAMP_THRESHOLD_MS: i64 = DEFAULT_TIMESTAMP_THRESHOLD_PAST_MS;

const ULID_LEN: usize = 26;
const ULID_TIMESTAMP_LEN: usize = 10;

/// Extract a timestamp from a ULID string.
pub fn ulid_to_date(maybe_ulid: &str) -> Option<OffsetDateTime> {
    let millis = ulid_timestamp_millis(maybe_ulid)?;
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(millis) * 1_000_000).ok()
}

/// Validate that a possibly prefixed ULID timestamp is within the default clock-skew window.
pub fn validate_ulid_timestamp(prefixed_ulid: &str, prefix: &str) -> Option<String> {
    validate_ulid_timestamp_at(
        prefixed_ulid,
        prefix,
        OffsetDateTime::now_utc(),
        DEFAULT_TIMESTAMP_THRESHOLD_PAST_MS,
        DEFAULT_TIMESTAMP_THRESHOLD_FUTURE_MS,
    )
}

/// Validate that a possibly prefixed ULID timestamp is near a supplied server time.
pub fn validate_ulid_timestamp_at(
    prefixed_ulid: &str,
    prefix: &str,
    server_timestamp: OffsetDateTime,
    past_threshold_ms: i64,
    future_threshold_ms: i64,
) -> Option<String> {
    let raw = prefixed_ulid.strip_prefix(prefix).unwrap_or(prefixed_ulid);
    let Some(ulid_timestamp) = ulid_to_date(raw) else {
        return Some(format!(
            "Invalid runId: {prefixed_ulid:?} is not a valid ULID"
        ));
    };

    let diff_ms = (server_timestamp - ulid_timestamp).whole_milliseconds();
    if diff_ms > 0 && diff_ms <= i128::from(past_threshold_ms) {
        return None;
    }
    if diff_ms <= 0 && -diff_ms <= i128::from(future_threshold_ms) {
        return None;
    }

    let drift_seconds = diff_ms.unsigned_abs() / 1000;
    let direction = if diff_ms > 0 { "past" } else { "future" };
    let threshold_ms = if diff_ms > 0 {
        past_threshold_ms
    } else {
        future_threshold_ms
    };
    let threshold_seconds = threshold_ms / 1000;

    Some(format!(
        "Invalid runId timestamp: embedded timestamp is {drift_seconds}s in the {direction} (threshold: {threshold_seconds}s)"
    ))
}

fn ulid_timestamp_millis(raw: &str) -> Option<u64> {
    if raw.len() != ULID_LEN {
        return None;
    }

    let mut value = 0u64;
    for (index, byte) in raw.bytes().enumerate() {
        let decoded = u64::from(decode_crockford(byte)?);
        if index == 0 && decoded > 7 {
            return None;
        }
        if index < ULID_TIMESTAMP_LEN {
            value = (value << 5) | decoded;
        }
    }

    Some(value)
}

fn decode_crockford(byte: u8) -> Option<u8> {
    match byte.to_ascii_uppercase() {
        b'0' => Some(0),
        b'1' => Some(1),
        b'2' => Some(2),
        b'3' => Some(3),
        b'4' => Some(4),
        b'5' => Some(5),
        b'6' => Some(6),
        b'7' => Some(7),
        b'8' => Some(8),
        b'9' => Some(9),
        b'A' => Some(10),
        b'B' => Some(11),
        b'C' => Some(12),
        b'D' => Some(13),
        b'E' => Some(14),
        b'F' => Some(15),
        b'G' => Some(16),
        b'H' => Some(17),
        b'J' => Some(18),
        b'K' => Some(19),
        b'M' => Some(20),
        b'N' => Some(21),
        b'P' => Some(22),
        b'Q' => Some(23),
        b'R' => Some(24),
        b'S' => Some(25),
        b'T' => Some(26),
        b'V' => Some(27),
        b'W' => Some(28),
        b'X' => Some(29),
        b'Y' => Some(30),
        b'Z' => Some(31),
        _ => None,
    }
}
