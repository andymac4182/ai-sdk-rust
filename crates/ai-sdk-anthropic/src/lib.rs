//! Package-owned inventory checkpoint for upstream `@ai-sdk/anthropic`.
//!
//! Runtime provider behavior is intentionally not claimed yet. The current
//! AI-01 slice establishes the one-to-one crate boundary and tracks the
//! current upstream test corpus so child rows can port it without losing rows.

/// Upstream package covered by this crate.
pub const UPSTREAM_PACKAGE: &str = "@ai-sdk/anthropic";

/// Upstream package directory in `vercel/ai`.
pub const UPSTREAM_PACKAGE_DIR: &str = "packages/anthropic";

/// Upstream commit used for the checked-in AI-01 inventory.
pub const UPSTREAM_COMMIT: &str = "ab6d66482d31afe15f4973a51c5f7cfa09c92ea6";

/// Checked-in row-level inventory document for this package.
pub const INVENTORY_DOCUMENT: &str = "docs/ai-foundational-provider-inventory.md";

/// Current upstream test files under `packages/anthropic/src`.
pub const UPSTREAM_TEST_FILES: usize = 13;

/// Current detected upstream `it`/`test` cases under `packages/anthropic/src`.
pub const UPSTREAM_TEST_CASES: usize = 424;

/// Current explicit TypeScript type-system exceptions.
pub const TYPE_SYSTEM_IMPOSSIBLE_CASES: usize = 6;

/// Current explicit JavaScript runtime exceptions.
pub const JS_ONLY_DOCUMENTED_CASES: usize = 0;

/// Current portable cases still requiring named Rust tests.
pub const PORTABLE_UNMAPPED_CASES: usize =
    UPSTREAM_TEST_CASES - TYPE_SYSTEM_IMPOSSIBLE_CASES - JS_ONLY_DOCUMENTED_CASES;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_foundational_inventory_tracks_current_upstream_cases() {
        assert_eq!(UPSTREAM_PACKAGE, "@ai-sdk/anthropic");
        assert_eq!(UPSTREAM_PACKAGE_DIR, "packages/anthropic");
        assert_eq!(UPSTREAM_COMMIT, "ab6d66482d31afe15f4973a51c5f7cfa09c92ea6");
        assert_eq!(UPSTREAM_TEST_FILES, 13);
        assert_eq!(UPSTREAM_TEST_CASES, 424);
        assert_eq!(TYPE_SYSTEM_IMPOSSIBLE_CASES, 6);
        assert_eq!(JS_ONLY_DOCUMENTED_CASES, 0);
        assert_eq!(PORTABLE_UNMAPPED_CASES, 418);
        assert_eq!(
            INVENTORY_DOCUMENT,
            "docs/ai-foundational-provider-inventory.md"
        );
    }

    #[test]
    fn anthropic_foundational_inventory_has_no_ported_cases_yet() {
        assert_eq!(PORTABLE_UNMAPPED_CASES + TYPE_SYSTEM_IMPOSSIBLE_CASES, 424);
    }
}
