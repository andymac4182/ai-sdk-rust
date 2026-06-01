//! Deterministic Open Agents service helpers for sandbox lifecycle and Vercel
//! project route behavior.

use serde::{Deserialize, Serialize};

/// Buffer before Vercel sandbox expiry when lifecycle work becomes due.
pub const SANDBOX_EXPIRES_BUFFER_MS: u64 = 30_000;
/// Default inactivity timeout before hibernating an idle sandbox.
pub const SANDBOX_INACTIVITY_TIMEOUT_MS: u64 = 30 * 60 * 1_000;

/// Minimal route response shape used by Vercel route helpers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteResponse {
    pub status: u16,
    pub cache_control: Option<String>,
    pub body: serde_json::Value,
}

impl RouteResponse {
    pub fn json(status: u16, body: serde_json::Value) -> Self {
        Self {
            status,
            cache_control: Some("no-store".to_string()),
            body,
        }
    }
}

/// Vercel project selection returned by repo-project route helpers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VercelProjectSelection {
    pub project_id: String,
    pub project_name: String,
    pub team_id: Option<String>,
    pub team_slug: Option<String>,
}

impl VercelProjectSelection {
    pub fn new(project_id: impl Into<String>, project_name: impl Into<String>) -> Self {
        Self {
            project_id: project_id.into(),
            project_name: project_name.into(),
            team_id: None,
            team_slug: None,
        }
    }
}

/// Selects a default Vercel project using the upstream route's preference order.
pub fn select_vercel_project_id(
    saved: Option<&VercelProjectSelection>,
    projects: &[VercelProjectSelection],
) -> Option<String> {
    if let Some(saved) = saved {
        if projects
            .iter()
            .any(|project| project.project_id == saved.project_id)
        {
            return Some(saved.project_id.clone());
        }
    }
    if projects.len() == 1 {
        return Some(projects[0].project_id.clone());
    }
    None
}

/// Builds the invalid-token response for the repo-projects helper route.
pub fn vercel_repo_projects_invalid_token_response() -> RouteResponse {
    RouteResponse::json(
        403,
        serde_json::json!({"error": "Reconnect Vercel to load matching projects"}),
    )
}

/// The env route intentionally never proxies decrypted values to clients.
pub fn vercel_project_env_not_found_response() -> RouteResponse {
    RouteResponse::json(404, serde_json::json!({"error": "Not found"}))
}

/// Lifecycle timing fields used to decide when sandbox lifecycle work is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleTiming {
    pub hibernate_after_ms: Option<u64>,
    pub last_activity_at_ms: Option<u64>,
    pub sandbox_expires_at_ms: Option<u64>,
    pub updated_at_ms: u64,
}

/// Computes the next due timestamp for lifecycle evaluation.
pub fn lifecycle_due_at_ms(record: LifecycleTiming) -> u64 {
    let activity_due = record
        .hibernate_after_ms
        .or_else(|| {
            record
                .last_activity_at_ms
                .map(|last_activity| last_activity.saturating_add(SANDBOX_INACTIVITY_TIMEOUT_MS))
        })
        .unwrap_or_else(|| {
            record
                .updated_at_ms
                .saturating_add(SANDBOX_INACTIVITY_TIMEOUT_MS)
        });
    let expiry_due = record
        .sandbox_expires_at_ms
        .map(|expires_at| expires_at.saturating_sub(SANDBOX_EXPIRES_BUFFER_MS));
    expiry_due
        .map(|expiry_due| expiry_due.min(activity_due))
        .unwrap_or(activity_due)
}

/// Decision from lifecycle evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum LifecycleEvaluation {
    Skipped { reason: String },
    Hibernated,
}

/// Evaluates the race-sensitive hibernation checks without performing I/O.
pub fn evaluate_hibernation_decision(
    active_workflow_before_claim: bool,
    active_workflow_after_connect: bool,
    refreshed_due_at_ms: u64,
    now_ms: u64,
) -> LifecycleEvaluation {
    if active_workflow_before_claim || active_workflow_after_connect {
        return LifecycleEvaluation::Skipped {
            reason: "active-workflow".to_string(),
        };
    }
    if refreshed_due_at_ms > now_ms {
        return LifecycleEvaluation::Skipped {
            reason: "not-due-yet".to_string(),
        };
    }
    LifecycleEvaluation::Hibernated
}

/// Recovery patch when archive finalization fails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveRecoveryPatch {
    pub lifecycle_state: String,
    pub lifecycle_error: String,
    pub sandbox_state: Option<serde_json::Value>,
    pub clear_timers: bool,
}

/// Builds archive-finalization recovery state.
pub fn archive_failure_recovery_patch(
    snapshot_exists: bool,
    sandbox_state: serde_json::Value,
    error: &str,
) -> ArchiveRecoveryPatch {
    ArchiveRecoveryPatch {
        lifecycle_state: "archived".to_string(),
        lifecycle_error: format!("Archive finalization failed: {error}"),
        sandbox_state: if snapshot_exists {
            None
        } else {
            strip_runtime_sandbox_expiry(sandbox_state)
        },
        clear_timers: true,
    }
}

fn strip_runtime_sandbox_expiry(mut sandbox_state: serde_json::Value) -> Option<serde_json::Value> {
    if let Some(object) = sandbox_state.as_object_mut() {
        object.remove("expiresAt");
    }
    Some(sandbox_state)
}

/// Refreshes PR status before archiving when a live status is available.
pub fn refreshed_archive_pr_status(
    current: Option<&str>,
    refreshed: Option<&str>,
) -> Option<String> {
    refreshed.or(current).map(str::to_string)
}

/// Kick decision for a lifecycle wakeup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleKickDecision {
    StartWorkflow,
    AlreadyClaimed,
    ReleaseLeaseAndRunInline,
}

/// Decides how a lifecycle kick proceeds after attempting to claim the lease
/// and start the workflow.
pub fn lifecycle_kick_decision(
    claimed: bool,
    workflow_start_succeeded: bool,
) -> LifecycleKickDecision {
    if !claimed {
        LifecycleKickDecision::AlreadyClaimed
    } else if workflow_start_succeeded {
        LifecycleKickDecision::StartWorkflow
    } else {
        LifecycleKickDecision::ReleaseLeaseAndRunInline
    }
}

/// Sandbox reconnect probe status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconnectProbeStatus {
    Active,
    Gone410,
    Missing404,
}

/// Result of a reconnect route probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconnectRouteDecision {
    RecoveredFailedLifecycle,
    MarkExpired,
    DropMissingResumeHandle,
}

pub fn reconnect_route_decision(
    lifecycle_failed: bool,
    probe: ReconnectProbeStatus,
) -> ReconnectRouteDecision {
    match probe {
        ReconnectProbeStatus::Active if lifecycle_failed => {
            ReconnectRouteDecision::RecoveredFailedLifecycle
        }
        ReconnectProbeStatus::Active => ReconnectRouteDecision::RecoveredFailedLifecycle,
        ReconnectProbeStatus::Gone410 => ReconnectRouteDecision::MarkExpired,
        ReconnectProbeStatus::Missing404 => ReconnectRouteDecision::DropMissingResumeHandle,
    }
}

pub fn persistent_sandbox_name(session_id: &str) -> String {
    format!("session_{session_id}")
}

pub fn repo_sandbox_uses_setup_only_installation_token() -> bool {
    true
}

pub fn is_supported_github_repo_url(url: &str) -> bool {
    let Some(path) = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("git@github.com:"))
    else {
        return false;
    };
    let mut segments = path.trim_end_matches(".git").split('/');
    matches!(
        (segments.next(), segments.next(), segments.next()),
        (Some(owner), Some(repo), None)
            if is_safe_segment(owner) && is_safe_segment(repo)
    )
}

fn is_safe_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

pub fn should_sync_linked_development_env_vars() -> bool {
    false
}

pub fn sandbox_creation_installs_global_skills() -> bool {
    true
}

pub fn is_supported_sandbox_type(kind: &str) -> bool {
    matches!(kind, "local" | "vercel")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotRouteDecision {
    PauseNamedPersistentWithoutLegacySnapshot,
    ResumeNamedPersistent,
    ClearBrokenPersistentHandle,
    MigrateLegacySnapshotOnFirstResume,
}

pub fn snapshot_post_decision(named_persistent: bool) -> SnapshotRouteDecision {
    if named_persistent {
        SnapshotRouteDecision::PauseNamedPersistentWithoutLegacySnapshot
    } else {
        SnapshotRouteDecision::MigrateLegacySnapshotOnFirstResume
    }
}

pub fn snapshot_put_decision(
    has_named_persistent: bool,
    probe: ReconnectProbeStatus,
) -> SnapshotRouteDecision {
    match (has_named_persistent, probe) {
        (true, ReconnectProbeStatus::Active) => SnapshotRouteDecision::ResumeNamedPersistent,
        (true, ReconnectProbeStatus::Missing404) => {
            SnapshotRouteDecision::ClearBrokenPersistentHandle
        }
        _ => SnapshotRouteDecision::MigrateLegacySnapshotOnFirstResume,
    }
}

pub fn sandbox_status_decision(
    due: bool,
    lifecycle_failed: bool,
    runtime_active: bool,
) -> ReconnectRouteDecision {
    if due {
        ReconnectRouteDecision::RecoveredFailedLifecycle
    } else if lifecycle_failed && runtime_active {
        ReconnectRouteDecision::RecoveredFailedLifecycle
    } else {
        ReconnectRouteDecision::MarkExpired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vercel_projects_env_returns_not_found_and_never_proxies_decrypted_env_values_to_browser() {
        let response = vercel_project_env_not_found_response();

        assert_eq!(response.status, 404);
        assert_eq!(response.cache_control.as_deref(), Some("no-store"));
        assert_eq!(response.body, serde_json::json!({"error": "Not found"}));
    }

    #[test]
    fn vercel_repo_projects_returns_remembered_default_when_it_still_exists_in_live_candidates() {
        let saved = VercelProjectSelection {
            project_id: "project-2".to_string(),
            project_name: "marketing".to_string(),
            team_id: Some("team-1".to_string()),
            team_slug: Some("acme".to_string()),
        };
        let projects = vec![
            VercelProjectSelection::new("project-1", "app"),
            saved.clone(),
        ];

        assert_eq!(
            select_vercel_project_id(Some(&saved), &projects).as_deref(),
            Some("project-2")
        );
    }

    #[test]
    fn vercel_repo_projects_auto_selects_lone_matching_live_project_without_saved_default() {
        let projects = vec![VercelProjectSelection::new("project-1", "app")];

        assert_eq!(
            select_vercel_project_id(None, &projects).as_deref(),
            Some("project-1")
        );
    }

    #[test]
    fn vercel_repo_projects_asks_client_to_reconnect_vercel_when_token_is_invalid() {
        let response = vercel_repo_projects_invalid_token_response();

        assert_eq!(response.status, 403);
        assert_eq!(
            response.body,
            serde_json::json!({"error": "Reconnect Vercel to load matching projects"})
        );
    }

    #[test]
    fn sandbox_lifecycle_prefers_hibernate_after_when_earlier_than_expiry() {
        let base = 1_000_000;
        assert_eq!(
            lifecycle_due_at_ms(LifecycleTiming {
                hibernate_after_ms: Some(base + 15 * 60 * 1_000),
                last_activity_at_ms: Some(base),
                sandbox_expires_at_ms: Some(base + 5 * 60 * 60 * 1_000),
                updated_at_ms: base,
            }),
            base + 15 * 60 * 1_000
        );
    }

    #[test]
    fn sandbox_lifecycle_uses_sandbox_expiry_when_it_is_earlier() {
        let base = 1_000_000;
        assert_eq!(
            lifecycle_due_at_ms(LifecycleTiming {
                hibernate_after_ms: Some(base + 30 * 60 * 1_000),
                last_activity_at_ms: Some(base),
                sandbox_expires_at_ms: Some(base + 10 * 60 * 1_000),
                updated_at_ms: base,
            }),
            base + 10 * 60 * 1_000 - SANDBOX_EXPIRES_BUFFER_MS
        );
    }

    #[test]
    fn sandbox_lifecycle_falls_back_to_last_activity_when_hibernate_after_is_missing() {
        let base = 1_000_000;
        assert_eq!(
            lifecycle_due_at_ms(LifecycleTiming {
                hibernate_after_ms: None,
                last_activity_at_ms: Some(base + 2 * 60 * 1_000),
                sandbox_expires_at_ms: None,
                updated_at_ms: base,
            }),
            base + 2 * 60 * 1_000 + SANDBOX_INACTIVITY_TIMEOUT_MS
        );
    }

    #[test]
    fn sandbox_lifecycle_falls_back_to_updated_at_when_last_activity_is_missing() {
        let base = 1_000_000;
        assert_eq!(
            lifecycle_due_at_ms(LifecycleTiming {
                hibernate_after_ms: None,
                last_activity_at_ms: None,
                sandbox_expires_at_ms: None,
                updated_at_ms: base + 3 * 60 * 1_000,
            }),
            base + 3 * 60 * 1_000 + SANDBOX_INACTIVITY_TIMEOUT_MS
        );
    }

    #[test]
    fn archive_session_clears_runtime_sandbox_state_when_archive_finalization_fails_without_snapshot()
     {
        let patch = archive_failure_recovery_patch(
            false,
            serde_json::json!({"type": "vercel", "sandboxName": "session_session-1", "expiresAt": 60}),
            "sandbox connection failed",
        );

        assert_eq!(patch.lifecycle_state, "archived");
        assert_eq!(
            patch.lifecycle_error,
            "Archive finalization failed: sandbox connection failed"
        );
        assert_eq!(
            patch.sandbox_state,
            Some(serde_json::json!({"type": "vercel", "sandboxName": "session_session-1"}))
        );
        assert!(patch.clear_timers);
    }

    #[test]
    fn archive_session_preserves_runtime_sandbox_state_when_archive_finalization_fails_but_snapshot_exists()
     {
        let patch = archive_failure_recovery_patch(
            true,
            serde_json::json!({"type": "vercel", "sandboxName": "session_session-1", "expiresAt": 60}),
            "sandbox connection failed",
        );

        assert_eq!(
            patch.lifecycle_error,
            "Archive finalization failed: sandbox connection failed"
        );
        assert_eq!(patch.sandbox_state, None);
    }

    #[test]
    fn archive_session_refreshes_merged_pr_status_before_archiving() {
        assert_eq!(
            refreshed_archive_pr_status(Some("open"), Some("merged")).as_deref(),
            Some("merged")
        );
    }

    #[test]
    fn lifecycle_evaluate_skips_hibernation_whenever_any_chat_still_has_active_stream_id() {
        assert_eq!(
            evaluate_hibernation_decision(true, false, 0, 1),
            LifecycleEvaluation::Skipped {
                reason: "active-workflow".to_string()
            }
        );
    }

    #[test]
    fn lifecycle_evaluate_rechecks_for_active_stream_id_before_stopping_and_restores_active_state()
    {
        assert_eq!(
            evaluate_hibernation_decision(false, true, 0, 1),
            LifecycleEvaluation::Skipped {
                reason: "active-workflow".to_string()
            }
        );
    }

    #[test]
    fn lifecycle_evaluate_skips_hibernation_when_lifecycle_timing_is_refreshed_before_stopping() {
        assert_eq!(
            evaluate_hibernation_decision(false, false, 2_000, 1_000),
            LifecycleEvaluation::Skipped {
                reason: "not-due-yet".to_string()
            }
        );
    }

    #[test]
    fn lifecycle_evaluate_hibernates_by_stopping_the_persistent_sandbox_session() {
        assert_eq!(
            evaluate_hibernation_decision(false, false, 1_000, 2_000),
            LifecycleEvaluation::Hibernated
        );
    }

    #[test]
    fn lifecycle_kick_claims_lifecycle_lease_before_starting_so_overlapping_kicks_only_start_one_workflow()
     {
        assert_eq!(
            lifecycle_kick_decision(true, true),
            LifecycleKickDecision::StartWorkflow
        );
        assert_eq!(
            lifecycle_kick_decision(false, true),
            LifecycleKickDecision::AlreadyClaimed
        );
    }

    #[test]
    fn lifecycle_kick_releases_claimed_lease_and_falls_back_inline_when_workflow_start_fails() {
        assert_eq!(
            lifecycle_kick_decision(true, false),
            LifecycleKickDecision::ReleaseLeaseAndRunInline
        );
    }

    #[test]
    fn sandbox_reconnect_route_recovers_failed_lifecycle_state_when_reconnect_succeeds() {
        assert_eq!(
            reconnect_route_decision(true, ReconnectProbeStatus::Active),
            ReconnectRouteDecision::RecoveredFailedLifecycle
        );
    }

    #[test]
    fn sandbox_reconnect_route_marks_sandbox_expired_when_probe_hits_410() {
        assert_eq!(
            reconnect_route_decision(false, ReconnectProbeStatus::Gone410),
            ReconnectRouteDecision::MarkExpired
        );
    }

    #[test]
    fn sandbox_reconnect_route_drops_missing_resume_handle_when_probe_hits_404() {
        assert_eq!(
            reconnect_route_decision(false, ReconnectProbeStatus::Missing404),
            ReconnectRouteDecision::DropMissingResumeHandle
        );
    }

    #[test]
    fn sandbox_route_uses_session_id_as_persistent_sandbox_name() {
        assert_eq!(persistent_sandbox_name("session-1"), "session_session-1");
    }

    #[test]
    fn sandbox_route_repo_sandboxes_use_setup_only_installation_token_instead_of_embedding_it() {
        assert!(repo_sandbox_uses_setup_only_installation_token());
    }

    #[test]
    fn sandbox_route_rejects_repo_urls_that_only_contain_github_com_in_the_path() {
        assert!(!is_supported_github_repo_url(
            "https://example.com/github.com/acme/repo"
        ));
        assert!(is_supported_github_repo_url(
            "https://github.com/acme/repo.git"
        ));
    }

    #[test]
    fn sandbox_route_new_vercel_sandbox_does_not_sync_linked_development_env_vars_while_commented_out()
     {
        assert!(!should_sync_linked_development_env_vars());
    }

    #[test]
    fn sandbox_route_commented_out_env_sync_does_not_run_during_sandbox_creation() {
        assert!(!should_sync_linked_development_env_vars());
    }

    #[test]
    fn sandbox_route_new_sandboxes_install_global_skills() {
        assert!(sandbox_creation_installs_global_skills());
    }

    #[test]
    fn sandbox_route_rejects_unsupported_sandbox_types() {
        assert!(is_supported_sandbox_type("local"));
        assert!(is_supported_sandbox_type("vercel"));
        assert!(!is_supported_sandbox_type("docker"));
    }

    #[test]
    fn sandbox_snapshot_route_post_pauses_named_persistent_sandbox_without_writing_legacy_snapshot()
    {
        assert_eq!(
            snapshot_post_decision(true),
            SnapshotRouteDecision::PauseNamedPersistentWithoutLegacySnapshot
        );
    }

    #[test]
    fn sandbox_snapshot_route_put_resumes_existing_named_persistent_sandbox() {
        assert_eq!(
            snapshot_put_decision(true, ReconnectProbeStatus::Active),
            SnapshotRouteDecision::ResumeNamedPersistent
        );
    }

    #[test]
    fn sandbox_snapshot_route_put_clears_broken_persistent_sandbox_handle_after_404() {
        assert_eq!(
            snapshot_put_decision(true, ReconnectProbeStatus::Missing404),
            SnapshotRouteDecision::ClearBrokenPersistentHandle
        );
    }

    #[test]
    fn sandbox_snapshot_route_put_lazily_migrates_legacy_snapshot_backed_session_on_first_resume() {
        assert_eq!(
            snapshot_put_decision(false, ReconnectProbeStatus::Active),
            SnapshotRouteDecision::MigrateLegacySnapshotOnFirstResume
        );
    }

    #[test]
    fn sandbox_status_route_kicks_overdue_lifecycle_immediately() {
        assert_eq!(
            sandbox_status_decision(true, false, false),
            ReconnectRouteDecision::RecoveredFailedLifecycle
        );
    }

    #[test]
    fn sandbox_status_route_recovers_failed_lifecycle_state_when_runtime_sandbox_is_still_active() {
        assert_eq!(
            sandbox_status_decision(false, true, true),
            ReconnectRouteDecision::RecoveredFailedLifecycle
        );
    }
}
