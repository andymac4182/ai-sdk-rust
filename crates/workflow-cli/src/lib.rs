//! Portable CLI formatting helpers for the standalone Workflow SDK port.
//!
//! This crate maps to upstream `packages/cli`. JavaScript command execution,
//! update checks, Vercel API access, and terminal rendering stay documented as
//! host tooling; the portable Rust surface starts with inspect output helpers.

#![forbid(unsafe_code)]

use std::time::SystemTime;

/// Upstream repository used for this crate boundary.
pub const UPSTREAM_REPOSITORY: &str = "github.com/vercel/workflow";

/// Upstream ref fetched into the local source mirror.
pub const UPSTREAM_REF: &str = "main";

/// Upstream package owned by this crate.
pub const UPSTREAM_PACKAGE: &str = "@workflow/cli";

/// Upstream package version inventoried for this crate.
pub const UPSTREAM_VERSION: &str = "5.0.0-beta.10";

/// Rust crate version compiled into the library.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Placeholder shown when inspect data is past its retention deadline.
pub const EXPIRED_DATA_MESSAGE: &str = "<data expired>";

/// Returns true when a run-level expiration timestamp is in the past.
pub fn has_expired_data(expired_at: Option<SystemTime>, now: SystemTime) -> bool {
    expired_at.is_some_and(|expired_at| expired_at < now)
}

/// Format an inspect table cell for the portable expired-data behavior.
pub fn format_table_value(
    property: &str,
    value: impl ToString,
    expired_at: Option<SystemTime>,
    now: SystemTime,
) -> String {
    if matches!(property, "input" | "output" | "error") && has_expired_data(expired_at, now) {
        EXPIRED_DATA_MESSAGE.to_string()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn now() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_735_689_600)
    }

    #[test]
    fn cli_has_expired_data_returns_false_when_expired_at_is_undefined() {
        assert!(!has_expired_data(None, now()));
    }

    #[test]
    fn cli_has_expired_data_returns_false_when_expired_at_is_in_the_future() {
        assert!(!has_expired_data(
            Some(now() + Duration::from_secs(24 * 60 * 60)),
            now()
        ));
    }

    #[test]
    fn cli_has_expired_data_returns_true_when_expired_at_is_in_the_past() {
        assert!(has_expired_data(
            Some(now() - Duration::from_secs(24 * 60 * 60)),
            now()
        ));
    }

    #[test]
    fn cli_format_table_value_returns_input_value_when_expired_at_is_in_the_future() {
        let formatted = format_table_value(
            "input",
            "hello",
            Some(now() + Duration::from_secs(24 * 60 * 60)),
            now(),
        );
        assert!(!formatted.contains("expired"));
    }

    #[test]
    fn cli_format_table_value_returns_expired_message_when_expired_at_is_in_the_past() {
        let formatted = format_table_value(
            "output",
            "hello",
            Some(now() - Duration::from_secs(24 * 60 * 60)),
            now(),
        );
        assert!(formatted.contains("data expired"));
    }

    #[test]
    fn cli_format_table_value_returns_input_value_when_expired_at_is_not_present() {
        let formatted = format_table_value("input", "hello", None, now());
        assert!(!formatted.contains("expired"));
    }
}
