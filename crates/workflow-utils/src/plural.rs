pub fn pluralize<'a>(singular: &'a str, plural: &'a str, count: isize) -> &'a str {
    if count == 1 { singular } else { plural }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_pluralize_cases() {
        assert_eq!(pluralize("step", "steps", 1), "step");
        assert_eq!(pluralize("retry", "retries", 1), "retry");
        assert_eq!(pluralize("hook", "hooks", 1), "hook");

        assert_eq!(pluralize("step", "steps", 0), "steps");
        assert_eq!(pluralize("retry", "retries", 0), "retries");

        assert_eq!(pluralize("step", "steps", 2), "steps");
        assert_eq!(pluralize("retry", "retries", 3), "retries");
        assert_eq!(pluralize("hook", "hooks", 100), "hooks");

        assert_eq!(pluralize("has", "have", 1), "has");
        assert_eq!(pluralize("has", "have", 2), "have");
        assert_eq!(pluralize("has", "have", 0), "have");
    }
}
