use std::{collections::BTreeMap, env};

pub fn resolve_workflow_target_world(env: &BTreeMap<String, String>) -> String {
    if let Some(configured_world) = env.get("WORKFLOW_TARGET_WORLD") {
        if !configured_world.is_empty() {
            return configured_world.clone();
        }
    }

    if env
        .get("VERCEL_DEPLOYMENT_ID")
        .is_some_and(|deployment_id| !deployment_id.is_empty())
    {
        "vercel".to_owned()
    } else {
        "local".to_owned()
    }
}

pub fn resolve_workflow_target_world_from_env() -> String {
    resolve_workflow_target_world(&env::vars().collect())
}

pub fn is_vercel_world_target(target_world: &str) -> bool {
    matches!(target_world, "vercel" | "@workflow/world-vercel")
}

pub fn uses_vercel_world(env: &BTreeMap<String, String>) -> bool {
    is_vercel_world_target(&resolve_workflow_target_world(env))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_map(values: &[(&str, &str)]) -> BTreeMap<String, String> {
        values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn upstream_world_target_cases() {
        assert_eq!(
            resolve_workflow_target_world(&env_map(&[
                ("WORKFLOW_TARGET_WORLD", "@workflow/world-postgres"),
                ("VERCEL_DEPLOYMENT_ID", "deployment-id"),
            ])),
            "@workflow/world-postgres"
        );
        assert_eq!(
            resolve_workflow_target_world(&env_map(&[("VERCEL_DEPLOYMENT_ID", "deployment-id")])),
            "vercel"
        );
        assert_eq!(resolve_workflow_target_world(&env_map(&[])), "local");

        assert!(is_vercel_world_target("vercel"));
        assert!(is_vercel_world_target("@workflow/world-vercel"));
        assert!(!is_vercel_world_target("local"));
        assert!(!is_vercel_world_target("@workflow/world-postgres"));

        assert!(uses_vercel_world(&env_map(&[(
            "VERCEL_DEPLOYMENT_ID",
            "deployment-id"
        )])));
        assert!(!uses_vercel_world(&env_map(&[])));
    }
}
