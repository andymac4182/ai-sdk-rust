use std::{
    error::Error,
    fmt,
    time::{Duration, SystemTime},
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DurationInput<'a> {
    String(&'a str),
    Milliseconds(i128),
    Date(SystemTime),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurationParseError {
    message: String,
}

impl DurationParseError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for DurationParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for DurationParseError {}

pub fn parse_duration_to_date(param: DurationInput<'_>) -> Result<SystemTime, DurationParseError> {
    match param {
        DurationInput::String(value) => {
            let duration = parse_duration_string(value).ok_or_else(|| {
                DurationParseError::new(format!(
                    "Invalid duration: \"{value}\". Expected a valid duration string like \"1s\", \"1m\", \"1h\", etc."
                ))
            })?;
            Ok(SystemTime::now() + duration)
        }
        DurationInput::Milliseconds(value) => {
            if value < 0 {
                return Err(DurationParseError::new(format!(
                    "Invalid duration: {value}. Expected a non-negative finite number of milliseconds."
                )));
            }
            let millis = u64::try_from(value).map_err(|_| {
                DurationParseError::new(format!(
                    "Invalid duration: {value}. Expected a non-negative finite number of milliseconds."
                ))
            })?;
            Ok(SystemTime::now() + Duration::from_millis(millis))
        }
        DurationInput::Date(date) => Ok(date),
    }
}

fn parse_duration_string(value: &str) -> Option<Duration> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.starts_with('-') {
        return None;
    }

    let numeric_len = trimmed
        .char_indices()
        .take_while(|(_, character)| character.is_ascii_digit() || *character == '.')
        .map(|(index, character)| index + character.len_utf8())
        .last()?;
    let amount = trimmed[..numeric_len].parse::<f64>().ok()?;
    if !amount.is_finite() || amount < 0.0 {
        return None;
    }

    let unit = trimmed[numeric_len..].trim().to_ascii_lowercase();
    let multiplier = match unit.as_str() {
        "" | "ms" | "msec" | "msecs" | "millisecond" | "milliseconds" => 1.0,
        "s" | "sec" | "secs" | "second" | "seconds" => 1_000.0,
        "m" | "min" | "mins" | "minute" | "minutes" => 60_000.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3_600_000.0,
        "d" | "day" | "days" => 86_400_000.0,
        _ => return None,
    };

    let millis = amount * multiplier;
    if !millis.is_finite() || millis < 0.0 || millis > u64::MAX as f64 {
        return None;
    }

    Some(Duration::from_millis(millis.round() as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_time_cases() {
        let before = SystemTime::now();
        let parsed_string = parse_duration_to_date(DurationInput::String("5s")).unwrap();
        assert!(parsed_string > before);

        let before = SystemTime::now();
        let parsed_number = parse_duration_to_date(DurationInput::Milliseconds(1000)).unwrap();
        assert!(parsed_number > before);

        let future_date = SystemTime::now() + Duration::from_secs(5);
        assert_eq!(
            parse_duration_to_date(DurationInput::Date(future_date)).unwrap(),
            future_date
        );

        assert!(parse_duration_to_date(DurationInput::String("invalid")).is_err());
        assert!(parse_duration_to_date(DurationInput::Milliseconds(-1000)).is_err());
    }
}
