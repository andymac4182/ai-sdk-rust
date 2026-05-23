//! Pure utility helpers for the Linear adapter. 1:1 port of the
//! pure-function subset of `packages/adapter-linear/src/utils.ts`:
//!
//! - [`get_user_name_from_profile_url`] - extract the
//!   `<slug>/profiles/<name>` segment from a Linear profile URL.
//! - [`calculate_expiry`] - compute an absolute expiry timestamp
//!   (epoch ms) from an `expires_in` duration in seconds.
//!
//! Deferred (depend on card / format infrastructure not yet ported):
//! - `renderMessageToLinearMarkdown` (needs `extractCard` +
//!   `cardToLinearMarkdown` + `convertEmojiPlaceholders` wiring)
//! - `assertAgentSessionThread` (needs the agent-session thread-id
//!   variant in the Rust decode helper)

/// Extract the user display name from a Linear profile URL. 1:1
/// port of upstream `getUserNameFromProfileUrl(url)`. Returns
/// the empty string when the URL does not contain a
/// `/profiles/<name>` segment.
///
/// Regex used upstream: `^https:\/\/linear\.app\/\S+\/profiles\/([^\/?#]+)`.
/// The Rust port doesn't pull in the `regex` crate (which isn't a
/// workspace dep yet); it parses the URL with `split` / `find`
/// matching exactly the same shape.
pub fn get_user_name_from_profile_url(url: &str) -> String {
    let prefix = "https://linear.app/";
    let Some(after_root) = url.strip_prefix(prefix) else {
        return String::new();
    };
    // After the workspace slug, "/profiles/" must appear.
    let Some(profiles_at) = after_root.find("/profiles/") else {
        return String::new();
    };
    // The slug must be non-empty (upstream uses `\S+`).
    let slug = &after_root[..profiles_at];
    if slug.is_empty() {
        return String::new();
    }
    let after_profiles = &after_root[profiles_at + "/profiles/".len()..];
    // Take characters up to the first `/`, `?`, or `#`.
    let end = after_profiles
        .find(|c: char| c == '/' || c == '?' || c == '#')
        .unwrap_or(after_profiles.len());
    after_profiles[..end].to_string()
}

/// Calculate an absolute expiry timestamp (Unix epoch
/// milliseconds) given an optional `expires_in` duration in
/// seconds. 1:1 port of upstream `calculateExpiry(expiresIn)`.
/// Returns `None` when `expires_in` is `None`.
pub fn calculate_expiry(expires_in: Option<u64>) -> Option<u128> {
    let secs = expires_in?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    Some(now_ms + (secs as u128) * 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- getUserNameFromProfileUrl (3 upstream cases) ----------

    #[test]
    fn extracts_the_profile_name_for_any_workspace_slug() {
        assert_eq!(
            get_user_name_from_profile_url("https://linear.app/acme-workspace/profiles/Bob"),
            "Bob"
        );
    }

    #[test]
    fn ignores_trailing_slash_query_and_hash() {
        assert_eq!(
            get_user_name_from_profile_url(
                "https://linear.app/acme-workspace/profiles/bob-bob/?foo=bar#details"
            ),
            "bob-bob"
        );
    }

    #[test]
    fn falls_back_to_empty_when_url_does_not_contain_a_profile_path() {
        assert_eq!(
            get_user_name_from_profile_url("https://linear.app/acme-workspace/issues/ABC-1"),
            ""
        );
    }

    // ---------- additive Rust-side coverage (not in upstream) ----------

    #[test]
    fn rejects_urls_with_a_non_linear_root() {
        assert_eq!(
            get_user_name_from_profile_url("https://example.com/foo/profiles/Bob"),
            ""
        );
    }

    #[test]
    fn rejects_urls_with_an_empty_workspace_slug() {
        // The upstream regex `\S+` requires at least one non-space
        // workspace slug char before `/profiles/`.
        assert_eq!(
            get_user_name_from_profile_url("https://linear.app//profiles/Bob"),
            ""
        );
    }

    #[test]
    fn calculate_expiry_returns_none_for_none_input() {
        assert_eq!(calculate_expiry(None), None);
    }

    #[test]
    fn calculate_expiry_adds_seconds_in_milliseconds() {
        let before = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let expiry = calculate_expiry(Some(3600)).unwrap();
        let after = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        // expiry must be in [before + 1h, after + 1h] inclusive
        assert!(expiry >= before + 3_600_000);
        assert!(expiry <= after + 3_600_000);
    }
}
