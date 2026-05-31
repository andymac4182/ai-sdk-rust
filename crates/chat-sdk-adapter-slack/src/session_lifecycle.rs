//! Slack thread-to-remote-session lifecycle helpers.
//!
//! Open Agents creates a durable session, initial chat, and sandbox
//! provisioning workflow from a conversation entry point, then reuses that
//! session for later messages in the same chat thread. This module keeps that
//! routing logic independent of any one database backend so service crates can
//! plug in Postgres, Redis, or an in-memory store for tests.

use std::collections::{HashMap, HashSet};

use chat_sdk_chat::types::ChannelVisibility;
use serde::{Deserialize, Serialize};

use crate::webhook::{SlackAppMentionPayload, SlackDirectMessagePayload, SlackEventBase};
use crate::{decode_thread_id, encode_thread_id};

/// Durable mapping key for a Slack thread-backed remote-agent session.
///
/// The platform thread id is still `slack:<channel_id>:<thread_ts>`, while
/// `team_id` and `enterprise_id` keep Enterprise Grid / Slack Connect installs
/// from colliding when the same Slack ids appear in different workspaces.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SlackThreadSessionKey {
    pub enterprise_id: Option<String>,
    pub team_id: Option<String>,
    pub channel_id: String,
    pub thread_ts: String,
}

impl SlackThreadSessionKey {
    pub fn new(
        channel_id: impl Into<String>,
        thread_ts: impl Into<String>,
        team_id: Option<String>,
        enterprise_id: Option<String>,
    ) -> Self {
        Self {
            enterprise_id,
            team_id,
            channel_id: channel_id.into(),
            thread_ts: thread_ts.into(),
        }
    }

    pub fn from_event_base(base: &SlackEventBase) -> Self {
        Self::new(
            base.channel_id.clone(),
            base.thread_ts.clone(),
            base.team_id.clone(),
            base.enterprise_id.clone(),
        )
    }

    pub fn from_thread_id(
        thread_id: &str,
        team_id: Option<String>,
        enterprise_id: Option<String>,
    ) -> Result<Self, SlackSessionLifecycleError> {
        let decoded =
            decode_thread_id(thread_id).ok_or(SlackSessionLifecycleError::InvalidThreadId)?;
        Ok(Self::new(
            decoded.channel_id,
            decoded.thread_ts,
            team_id,
            enterprise_id,
        ))
    }

    pub fn thread_id(&self) -> String {
        encode_thread_id(&self.channel_id, &self.thread_ts)
    }

    pub fn is_dm(&self) -> bool {
        self.channel_id.starts_with('D')
    }
}

/// Entry point that caused a Slack thread mapping to be created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlackSessionIngress {
    AppMention,
    DirectMessage,
}

/// Sandbox size class selected before provisioning.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlackResourceProfile {
    Small,
    #[default]
    Standard,
    Large,
}

impl SlackResourceProfile {
    fn parse(value: &str) -> Result<Self, SlackSessionLifecycleError> {
        match value.to_ascii_lowercase().as_str() {
            "small" => Ok(Self::Small),
            "standard" | "default" => Ok(Self::Standard),
            "large" => Ok(Self::Large),
            other => Err(SlackSessionLifecycleError::InvalidSettingsCommand(format!(
                "unknown resource profile {other:?}"
            ))),
        }
    }
}

/// Repo and run settings carried by a Slack-created remote-agent session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlackSessionSettings {
    pub repo_url: Option<String>,
    pub branch: Option<String>,
    pub resource_profile: SlackResourceProfile,
    pub auto_commit: bool,
    pub auto_push: bool,
    pub auto_pr: bool,
    pub global_skill_refs: Vec<String>,
}

impl Default for SlackSessionSettings {
    fn default() -> Self {
        Self {
            repo_url: None,
            branch: None,
            resource_profile: SlackResourceProfile::Standard,
            auto_commit: false,
            auto_push: false,
            auto_pr: false,
            global_skill_refs: Vec::new(),
        }
    }
}

impl SlackSessionSettings {
    pub fn apply_update(&mut self, update: SlackSessionSettingsUpdate) {
        if let Some(repo_url) = update.repo_url {
            self.repo_url = Some(repo_url);
        }
        if let Some(branch) = update.branch {
            self.branch = Some(branch);
        }
        if let Some(resource_profile) = update.resource_profile {
            self.resource_profile = resource_profile;
        }
        if let Some(auto_commit) = update.auto_commit {
            self.auto_commit = auto_commit;
        }
        if let Some(auto_push) = update.auto_push {
            self.auto_push = auto_push;
        }
        if let Some(auto_pr) = update.auto_pr {
            self.auto_pr = auto_pr;
        }
        if let Some(global_skill_refs) = update.global_skill_refs {
            self.global_skill_refs = global_skill_refs;
        }
    }
}

/// Default settings applied to newly created Slack sessions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SlackSessionDefaults {
    pub settings: SlackSessionSettings,
}

impl SlackSessionDefaults {
    fn materialize(&self, key: &SlackThreadSessionKey) -> SlackSessionSettings {
        let mut settings = self.settings.clone();
        if settings.branch.is_none() {
            settings.branch = Some(default_branch_name(key));
        }
        settings
    }
}

/// Partial settings update parsed from a Slack settings command or supplied by
/// a service-specific command surface.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SlackSessionSettingsUpdate {
    pub repo_url: Option<String>,
    pub branch: Option<String>,
    pub resource_profile: Option<SlackResourceProfile>,
    pub auto_commit: Option<bool>,
    pub auto_push: Option<bool>,
    pub auto_pr: Option<bool>,
    pub global_skill_refs: Option<Vec<String>>,
}

impl SlackSessionSettingsUpdate {
    pub fn parse_command(text: &str) -> Result<Option<Self>, SlackSessionLifecycleError> {
        let text = strip_leading_mentions(text.trim());
        let args = text
            .strip_prefix("settings ")
            .or_else(|| text.strip_prefix("/agent settings "))
            .or_else(|| text.strip_prefix("agent settings "));
        let Some(args) = args else {
            return Ok(None);
        };

        let mut update = Self::default();
        for token in args.split_whitespace() {
            let (key, value) = token.split_once('=').ok_or_else(|| {
                SlackSessionLifecycleError::InvalidSettingsCommand(format!(
                    "settings token {token:?} must be key=value",
                ))
            })?;
            match normalize_settings_key(key).as_str() {
                "repo_url" => update.repo_url = Some(value.to_string()),
                "branch" => update.branch = Some(value.to_string()),
                "resource_profile" => {
                    update.resource_profile = Some(SlackResourceProfile::parse(value)?);
                }
                "auto_commit" => update.auto_commit = Some(parse_bool_setting(key, value)?),
                "auto_push" => update.auto_push = Some(parse_bool_setting(key, value)?),
                "auto_pr" => update.auto_pr = Some(parse_bool_setting(key, value)?),
                "skills" => {
                    update.global_skill_refs = Some(
                        value
                            .split(',')
                            .filter(|part| !part.is_empty())
                            .map(str::to_string)
                            .collect(),
                    );
                }
                other => {
                    return Err(SlackSessionLifecycleError::InvalidSettingsCommand(format!(
                        "unknown settings key {other:?}"
                    )));
                }
            }
        }
        Ok(Some(update))
    }
}

/// Slack channel policy used before creating or resuming a remote session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackChannelPolicy {
    pub allow_public_channels: bool,
    pub allow_private_channels: bool,
    pub allow_direct_messages: bool,
    pub allow_slack_connect: bool,
    pub allow_unknown_channels: bool,
    pub allowed_channel_ids: HashSet<String>,
    pub denied_channel_ids: HashSet<String>,
}

impl Default for SlackChannelPolicy {
    fn default() -> Self {
        Self {
            allow_public_channels: true,
            allow_private_channels: true,
            allow_direct_messages: true,
            allow_slack_connect: false,
            allow_unknown_channels: false,
            allowed_channel_ids: HashSet::new(),
            denied_channel_ids: HashSet::new(),
        }
    }
}

impl SlackChannelPolicy {
    pub fn check(&self, request: &SlackSessionRequest) -> Result<(), SlackSessionLifecycleError> {
        if self.denied_channel_ids.contains(&request.key.channel_id) {
            return Err(SlackSessionLifecycleError::PermissionDenied(
                SlackPermissionFailure::new(request, SlackPermissionDeniedReason::ChannelDenied),
            ));
        }
        if !self.allowed_channel_ids.is_empty()
            && !self.allowed_channel_ids.contains(&request.key.channel_id)
        {
            return Err(SlackSessionLifecycleError::PermissionDenied(
                SlackPermissionFailure::new(
                    request,
                    SlackPermissionDeniedReason::ChannelNotAllowed,
                ),
            ));
        }
        if request.is_slack_connect && !self.allow_slack_connect {
            return Err(SlackSessionLifecycleError::PermissionDenied(
                SlackPermissionFailure::new(request, SlackPermissionDeniedReason::SlackConnect),
            ));
        }
        if request.key.is_dm() {
            return if self.allow_direct_messages {
                Ok(())
            } else {
                Err(SlackSessionLifecycleError::PermissionDenied(
                    SlackPermissionFailure::new(
                        request,
                        SlackPermissionDeniedReason::DirectMessage,
                    ),
                ))
            };
        }
        match request.channel_visibility {
            ChannelVisibility::Workspace if self.allow_public_channels => Ok(()),
            ChannelVisibility::Workspace => Err(SlackSessionLifecycleError::PermissionDenied(
                SlackPermissionFailure::new(request, SlackPermissionDeniedReason::PublicChannel),
            )),
            ChannelVisibility::Private if self.allow_private_channels => Ok(()),
            ChannelVisibility::Private => Err(SlackSessionLifecycleError::PermissionDenied(
                SlackPermissionFailure::new(request, SlackPermissionDeniedReason::PrivateChannel),
            )),
            ChannelVisibility::External if self.allow_slack_connect => Ok(()),
            ChannelVisibility::External => Err(SlackSessionLifecycleError::PermissionDenied(
                SlackPermissionFailure::new(request, SlackPermissionDeniedReason::SlackConnect),
            )),
            ChannelVisibility::Unknown if self.allow_unknown_channels => Ok(()),
            ChannelVisibility::Unknown => Err(SlackSessionLifecycleError::PermissionDenied(
                SlackPermissionFailure::new(request, SlackPermissionDeniedReason::UnknownChannel),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlackPermissionDeniedReason {
    ChannelDenied,
    ChannelNotAllowed,
    DirectMessage,
    PrivateChannel,
    PublicChannel,
    SlackConnect,
    UnknownChannel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackPermissionFailure {
    pub channel_id: String,
    pub thread_id: String,
    pub reason: SlackPermissionDeniedReason,
}

impl SlackPermissionFailure {
    fn new(request: &SlackSessionRequest, reason: SlackPermissionDeniedReason) -> Self {
        Self {
            channel_id: request.key.channel_id.clone(),
            thread_id: request.key.thread_id(),
            reason,
        }
    }
}

/// Incoming Slack message normalized for lifecycle routing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackSessionRequest {
    pub key: SlackThreadSessionKey,
    pub ingress: SlackSessionIngress,
    pub user_id: Option<String>,
    pub text: String,
    pub channel_visibility: ChannelVisibility,
    pub is_slack_connect: bool,
}

impl SlackSessionRequest {
    pub fn from_app_mention(payload: &SlackAppMentionPayload) -> Self {
        Self::from_event_base(&payload.base, SlackSessionIngress::AppMention)
    }

    pub fn from_direct_message(payload: &SlackDirectMessagePayload) -> Self {
        Self::from_event_base(&payload.base, SlackSessionIngress::DirectMessage)
    }

    pub fn from_event_base(base: &SlackEventBase, ingress: SlackSessionIngress) -> Self {
        let is_slack_connect = base.is_ext_shared_channel.unwrap_or(false);
        Self {
            key: SlackThreadSessionKey::from_event_base(base),
            ingress,
            user_id: base.user_id.clone(),
            text: base.text.clone(),
            channel_visibility: derive_channel_visibility(&base.channel_id, is_slack_connect),
            is_slack_connect,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlackRemoteAgentSessionStatus {
    Active,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlackSandboxLifecycleState {
    Pending,
    Provisioning,
    Ready,
    Failed,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlackSkillLoadState {
    NotRequested,
    PendingProvisioning,
    Loaded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlackSandboxSession {
    pub lifecycle_state: SlackSandboxLifecycleState,
    pub sandbox_id: Option<String>,
    pub working_directory: Option<String>,
    pub current_branch: Option<String>,
    pub failure_message: Option<String>,
    pub failure_reported: bool,
    pub skill_load_state: SlackSkillLoadState,
    pub loaded_skill_refs: Vec<String>,
}

impl SlackSandboxSession {
    fn pending(settings: &SlackSessionSettings) -> Self {
        let skill_load_state = if settings.global_skill_refs.is_empty() {
            SlackSkillLoadState::NotRequested
        } else {
            SlackSkillLoadState::PendingProvisioning
        };
        Self {
            lifecycle_state: SlackSandboxLifecycleState::Pending,
            sandbox_id: None,
            working_directory: None,
            current_branch: None,
            failure_message: None,
            failure_reported: false,
            skill_load_state,
            loaded_skill_refs: Vec::new(),
        }
    }
}

/// Durable session/chat row pair associated with one Slack thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlackRemoteAgentSession {
    pub session_id: String,
    pub chat_id: String,
    pub thread_key: SlackThreadSessionKey,
    pub thread_id: String,
    pub ingress: SlackSessionIngress,
    pub status: SlackRemoteAgentSessionStatus,
    pub created_by_user_id: Option<String>,
    pub last_user_id: Option<String>,
    pub channel_visibility: ChannelVisibility,
    pub is_slack_connect: bool,
    pub settings: SlackSessionSettings,
    pub settings_revision: u64,
    pub sandbox: SlackSandboxSession,
}

/// Store interface for Slack thread-to-session mappings.
pub trait SlackSessionLifecycleStore {
    fn get_by_thread_key(&self, key: &SlackThreadSessionKey) -> Option<SlackRemoteAgentSession>;
    fn insert_session(
        &mut self,
        session: SlackRemoteAgentSession,
    ) -> Result<(), SlackSessionLifecycleError>;
    fn update_session(
        &mut self,
        session: SlackRemoteAgentSession,
    ) -> Result<(), SlackSessionLifecycleError>;
}

/// Test and single-process store implementation.
#[derive(Debug, Clone, Default)]
pub struct InMemorySlackSessionLifecycleStore {
    sessions_by_key: HashMap<SlackThreadSessionKey, SlackRemoteAgentSession>,
    keys_by_session_id: HashMap<String, SlackThreadSessionKey>,
}

impl InMemorySlackSessionLifecycleStore {
    pub fn len(&self) -> usize {
        self.sessions_by_key.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions_by_key.is_empty()
    }
}

impl SlackSessionLifecycleStore for InMemorySlackSessionLifecycleStore {
    fn get_by_thread_key(&self, key: &SlackThreadSessionKey) -> Option<SlackRemoteAgentSession> {
        self.sessions_by_key.get(key).cloned()
    }

    fn insert_session(
        &mut self,
        session: SlackRemoteAgentSession,
    ) -> Result<(), SlackSessionLifecycleError> {
        if self.sessions_by_key.contains_key(&session.thread_key) {
            return Err(SlackSessionLifecycleError::DuplicateMapping);
        }
        self.keys_by_session_id
            .insert(session.session_id.clone(), session.thread_key.clone());
        self.sessions_by_key
            .insert(session.thread_key.clone(), session);
        Ok(())
    }

    fn update_session(
        &mut self,
        session: SlackRemoteAgentSession,
    ) -> Result<(), SlackSessionLifecycleError> {
        if !self.sessions_by_key.contains_key(&session.thread_key) {
            return Err(SlackSessionLifecycleError::SessionNotFound);
        }
        self.keys_by_session_id
            .insert(session.session_id.clone(), session.thread_key.clone());
        self.sessions_by_key
            .insert(session.thread_key.clone(), session);
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlackSessionLifecycleAction {
    Created,
    Reused,
    SettingsUpdated {
        previous: SlackSessionSettings,
        update: SlackSessionSettingsUpdate,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackSessionLifecycleOutcome {
    pub action: SlackSessionLifecycleAction,
    pub session: SlackRemoteAgentSession,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackSandboxReady {
    pub sandbox_id: String,
    pub working_directory: String,
    pub current_branch: Option<String>,
    pub did_setup_workspace: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackProvisioningFailureReport {
    pub session_id: String,
    pub message: String,
    pub should_notify: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlackSessionLifecycleError {
    DuplicateMapping,
    InvalidSettingsCommand(String),
    InvalidThreadId,
    PermissionDenied(SlackPermissionFailure),
    SessionNotFound,
}

impl std::fmt::Display for SlackSessionLifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateMapping => f.write_str("Slack thread is already mapped to a session"),
            Self::InvalidSettingsCommand(message) => {
                write!(f, "Invalid settings command: {message}")
            }
            Self::InvalidThreadId => f.write_str("Slack thread id is invalid"),
            Self::PermissionDenied(failure) => {
                write!(
                    f,
                    "Slack channel {} is not allowed for remote-agent sessions: {:?}",
                    failure.channel_id, failure.reason
                )
            }
            Self::SessionNotFound => f.write_str("Slack session mapping was not found"),
        }
    }
}

impl std::error::Error for SlackSessionLifecycleError {}

/// Lifecycle coordinator that turns Slack events into durable session records.
#[derive(Debug, Clone)]
pub struct SlackSessionLifecycle<S> {
    store: S,
    policy: SlackChannelPolicy,
    defaults: SlackSessionDefaults,
    next_id: u64,
}

impl<S> SlackSessionLifecycle<S>
where
    S: SlackSessionLifecycleStore,
{
    pub fn new(store: S, policy: SlackChannelPolicy, defaults: SlackSessionDefaults) -> Self {
        Self {
            store,
            policy,
            defaults,
            next_id: 1,
        }
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    pub fn handle_app_mention(
        &mut self,
        payload: &SlackAppMentionPayload,
    ) -> Result<SlackSessionLifecycleOutcome, SlackSessionLifecycleError> {
        self.handle_request(SlackSessionRequest::from_app_mention(payload))
    }

    pub fn handle_direct_message(
        &mut self,
        payload: &SlackDirectMessagePayload,
    ) -> Result<SlackSessionLifecycleOutcome, SlackSessionLifecycleError> {
        self.handle_request(SlackSessionRequest::from_direct_message(payload))
    }

    pub fn handle_request(
        &mut self,
        request: SlackSessionRequest,
    ) -> Result<SlackSessionLifecycleOutcome, SlackSessionLifecycleError> {
        self.policy.check(&request)?;

        if let Some(mut session) = self.store.get_by_thread_key(&request.key) {
            session.last_user_id = request.user_id.clone();
            session.channel_visibility = request.channel_visibility;
            session.is_slack_connect = request.is_slack_connect;

            if let Some(update) = SlackSessionSettingsUpdate::parse_command(&request.text)? {
                let previous = session.settings.clone();
                session.settings.apply_update(update.clone());
                session.settings_revision += 1;
                if session.sandbox.lifecycle_state == SlackSandboxLifecycleState::Pending {
                    session.sandbox = SlackSandboxSession::pending(&session.settings);
                }
                self.store.update_session(session.clone())?;
                return Ok(SlackSessionLifecycleOutcome {
                    action: SlackSessionLifecycleAction::SettingsUpdated { previous, update },
                    session,
                });
            }

            self.store.update_session(session.clone())?;
            return Ok(SlackSessionLifecycleOutcome {
                action: SlackSessionLifecycleAction::Reused,
                session,
            });
        }

        let session = self.create_session_from_request(&request);
        self.store.insert_session(session.clone())?;
        Ok(SlackSessionLifecycleOutcome {
            action: SlackSessionLifecycleAction::Created,
            session,
        })
    }

    pub fn mark_provisioning(
        &mut self,
        key: &SlackThreadSessionKey,
    ) -> Result<SlackRemoteAgentSession, SlackSessionLifecycleError> {
        let mut session = self
            .store
            .get_by_thread_key(key)
            .ok_or(SlackSessionLifecycleError::SessionNotFound)?;
        session.sandbox.lifecycle_state = SlackSandboxLifecycleState::Provisioning;
        session.sandbox.failure_message = None;
        self.store.update_session(session.clone())?;
        Ok(session)
    }

    pub fn mark_ready(
        &mut self,
        key: &SlackThreadSessionKey,
        ready: SlackSandboxReady,
    ) -> Result<SlackRemoteAgentSession, SlackSessionLifecycleError> {
        let mut session = self
            .store
            .get_by_thread_key(key)
            .ok_or(SlackSessionLifecycleError::SessionNotFound)?;
        session.sandbox.lifecycle_state = SlackSandboxLifecycleState::Ready;
        session.sandbox.sandbox_id = Some(ready.sandbox_id);
        session.sandbox.working_directory = Some(ready.working_directory);
        session.sandbox.current_branch = ready
            .current_branch
            .or_else(|| session.settings.branch.clone());
        session.sandbox.failure_message = None;
        session.sandbox.failure_reported = false;
        if !session.settings.global_skill_refs.is_empty() && ready.did_setup_workspace {
            session.sandbox.skill_load_state = SlackSkillLoadState::Loaded;
            session.sandbox.loaded_skill_refs = session.settings.global_skill_refs.clone();
        } else if session.settings.global_skill_refs.is_empty() {
            session.sandbox.skill_load_state = SlackSkillLoadState::NotRequested;
        }
        self.store.update_session(session.clone())?;
        Ok(session)
    }

    pub fn mark_provisioning_failed(
        &mut self,
        key: &SlackThreadSessionKey,
        message: impl Into<String>,
    ) -> Result<SlackProvisioningFailureReport, SlackSessionLifecycleError> {
        let message = message.into();
        let mut session = self
            .store
            .get_by_thread_key(key)
            .ok_or(SlackSessionLifecycleError::SessionNotFound)?;
        let should_notify = !session.sandbox.failure_reported;
        session.sandbox.lifecycle_state = SlackSandboxLifecycleState::Failed;
        session.sandbox.failure_message = Some(message.clone());
        session.sandbox.failure_reported = true;
        if session.sandbox.skill_load_state == SlackSkillLoadState::PendingProvisioning {
            session.sandbox.skill_load_state = SlackSkillLoadState::Failed;
        }
        self.store.update_session(session.clone())?;
        Ok(SlackProvisioningFailureReport {
            session_id: session.session_id,
            message,
            should_notify,
        })
    }

    pub fn mark_stopped(
        &mut self,
        key: &SlackThreadSessionKey,
    ) -> Result<SlackRemoteAgentSession, SlackSessionLifecycleError> {
        let mut session = self
            .store
            .get_by_thread_key(key)
            .ok_or(SlackSessionLifecycleError::SessionNotFound)?;
        session.sandbox.lifecycle_state = SlackSandboxLifecycleState::Stopped;
        self.store.update_session(session.clone())?;
        Ok(session)
    }

    fn create_session_from_request(
        &mut self,
        request: &SlackSessionRequest,
    ) -> SlackRemoteAgentSession {
        let settings = self.defaults.materialize(&request.key);
        let session_id = self.next_session_id();
        let chat_id = self.next_chat_id();
        SlackRemoteAgentSession {
            session_id,
            chat_id,
            thread_key: request.key.clone(),
            thread_id: request.key.thread_id(),
            ingress: request.ingress,
            status: SlackRemoteAgentSessionStatus::Active,
            created_by_user_id: request.user_id.clone(),
            last_user_id: request.user_id.clone(),
            channel_visibility: request.channel_visibility,
            is_slack_connect: request.is_slack_connect,
            sandbox: SlackSandboxSession::pending(&settings),
            settings,
            settings_revision: 0,
        }
    }

    fn next_session_id(&mut self) -> String {
        let id = self.next_id;
        self.next_id += 1;
        format!("slack-session-{id}")
    }

    fn next_chat_id(&self) -> String {
        format!("slack-chat-{}", self.next_id - 1)
    }
}

fn derive_channel_visibility(channel_id: &str, is_slack_connect: bool) -> ChannelVisibility {
    if is_slack_connect {
        return ChannelVisibility::External;
    }
    match channel_id.chars().next() {
        Some('D') | Some('G') => ChannelVisibility::Private,
        Some('C') => ChannelVisibility::Workspace,
        _ => ChannelVisibility::Unknown,
    }
}

fn default_branch_name(key: &SlackThreadSessionKey) -> String {
    let normalized_thread = key
        .thread_ts
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>();
    format!("codex/slack-{}-{normalized_thread}", key.channel_id)
}

fn normalize_settings_key(key: &str) -> String {
    match key.to_ascii_lowercase().as_str() {
        "repo" | "clone_url" | "clone-url" => "repo_url".to_string(),
        "resources" | "resource" | "profile" => "resource_profile".to_string(),
        "autocommit" | "auto-commit" => "auto_commit".to_string(),
        "autopush" | "auto-push" => "auto_push".to_string(),
        "autopr" | "auto-pr" | "pr" => "auto_pr".to_string(),
        other => other.replace('-', "_"),
    }
}

fn parse_bool_setting(key: &str, value: &str) -> Result<bool, SlackSessionLifecycleError> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Ok(true),
        "false" | "no" | "off" | "0" => Ok(false),
        _ => Err(SlackSessionLifecycleError::InvalidSettingsCommand(format!(
            "{key} must be a boolean"
        ))),
    }
}

fn strip_leading_mentions(mut text: &str) -> &str {
    loop {
        let trimmed = text.trim_start();
        let Some(rest) = trimmed.strip_prefix("<@") else {
            return trimmed;
        };
        let Some(end) = rest.find('>') else {
            return trimmed;
        };
        text = &rest[end + 1..];
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chat_sdk_chat::thread::Thread;
    use chat_sdk_chat::types::Adapter;

    use super::*;
    use crate::SlackAdapter;
    use crate::SlackAdapterOptions;
    use crate::webhook::{SlackParseOptions, SlackWebhookPayload, parse_slack_webhook_body};

    fn lifecycle() -> SlackSessionLifecycle<InMemorySlackSessionLifecycleStore> {
        SlackSessionLifecycle::new(
            InMemorySlackSessionLifecycleStore::default(),
            SlackChannelPolicy::default(),
            SlackSessionDefaults::default(),
        )
    }

    fn app_mention_fixture(
        channel: &str,
        ts: &str,
        thread_ts: Option<&str>,
        text: &str,
        is_ext_shared_channel: bool,
    ) -> SlackAppMentionPayload {
        let mut event = serde_json::json!({
            "type": "app_mention",
            "channel": channel,
            "user": "U111",
            "text": text,
            "ts": ts,
        });
        if let Some(thread_ts) = thread_ts {
            event["thread_ts"] = serde_json::Value::String(thread_ts.to_string());
        }
        let envelope = serde_json::json!({
            "type": "event_callback",
            "team_id": "T111",
            "event_id": "Ev111",
            "event_time": 1710000000,
            "is_ext_shared_channel": is_ext_shared_channel,
            "event": event,
        });
        match parse_slack_webhook_body(&envelope.to_string(), &SlackParseOptions::default())
            .expect("fixture parses")
        {
            SlackWebhookPayload::AppMention(payload) => payload,
            other => panic!("expected app_mention, got {}", other.kind()),
        }
    }

    fn dm_fixture(channel: &str, ts: &str, text: &str) -> SlackDirectMessagePayload {
        let envelope = serde_json::json!({
            "type": "event_callback",
            "team_id": "T111",
            "event_id": "Ev222",
            "event_time": 1710000001,
            "event": {
                "type": "message",
                "channel_type": "im",
                "channel": channel,
                "user": "U222",
                "text": text,
                "ts": ts,
            },
        });
        match parse_slack_webhook_body(&envelope.to_string(), &SlackParseOptions::default())
            .expect("fixture parses")
        {
            SlackWebhookPayload::DirectMessage(payload) => payload,
            other => panic!("expected direct_message, got {}", other.kind()),
        }
    }

    #[test]
    fn fixture_first_app_mention_creates_session() {
        let mut lifecycle = lifecycle();
        let payload =
            app_mention_fixture("C123", "1710000000.000100", None, "<@UBOT> start", false);

        let outcome = lifecycle.handle_app_mention(&payload).unwrap();

        assert_eq!(outcome.action, SlackSessionLifecycleAction::Created);
        assert_eq!(outcome.session.thread_id, "slack:C123:1710000000.000100");
        assert_eq!(outcome.session.thread_key.team_id.as_deref(), Some("T111"));
        assert_eq!(outcome.session.ingress, SlackSessionIngress::AppMention);
        assert_eq!(
            outcome.session.channel_visibility,
            ChannelVisibility::Workspace
        );
        assert_eq!(
            outcome.session.sandbox.lifecycle_state,
            SlackSandboxLifecycleState::Pending
        );
        assert_eq!(lifecycle.store().len(), 1);
    }

    #[test]
    fn fixture_later_reply_reuses_existing_session() {
        let mut lifecycle = lifecycle();
        let first = app_mention_fixture("C123", "1710000000.000100", None, "start", false);
        let first_outcome = lifecycle.handle_app_mention(&first).unwrap();
        let reply = app_mention_fixture(
            "C123",
            "1710000000.000200",
            Some("1710000000.000100"),
            "continue",
            false,
        );

        let reused = lifecycle.handle_app_mention(&reply).unwrap();

        assert_eq!(reused.action, SlackSessionLifecycleAction::Reused);
        assert_eq!(reused.session.session_id, first_outcome.session.session_id);
        assert_eq!(reused.session.chat_id, first_outcome.session.chat_id);
        assert_eq!(lifecycle.store().len(), 1);
    }

    #[test]
    fn fixture_direct_message_creates_private_session() {
        let mut lifecycle = lifecycle();
        let payload = dm_fixture("D123", "1710000001.000100", "work privately");

        let outcome = lifecycle.handle_direct_message(&payload).unwrap();

        assert_eq!(outcome.action, SlackSessionLifecycleAction::Created);
        assert_eq!(outcome.session.ingress, SlackSessionIngress::DirectMessage);
        assert_eq!(outcome.session.thread_id, "slack:D123:1710000001.000100");
        assert_eq!(
            outcome.session.channel_visibility,
            ChannelVisibility::Private
        );
        assert!(outcome.session.thread_key.is_dm());
    }

    #[test]
    fn fixture_permission_failure_blocks_unauthorized_channel() {
        let mut policy = SlackChannelPolicy::default();
        policy.allowed_channel_ids.insert("C_ALLOWED".to_string());
        let mut lifecycle = SlackSessionLifecycle::new(
            InMemorySlackSessionLifecycleStore::default(),
            policy,
            SlackSessionDefaults::default(),
        );
        let payload = app_mention_fixture("C_BLOCKED", "1710000002.000100", None, "start", false);

        let err = lifecycle.handle_app_mention(&payload).unwrap_err();

        match err {
            SlackSessionLifecycleError::PermissionDenied(failure) => {
                assert_eq!(failure.channel_id, "C_BLOCKED");
                assert_eq!(
                    failure.reason,
                    SlackPermissionDeniedReason::ChannelNotAllowed
                );
            }
            other => panic!("expected permission failure, got {other:?}"),
        }
        assert!(lifecycle.store().is_empty());
    }

    #[test]
    fn fixture_settings_update_changes_future_runs() {
        let mut lifecycle = lifecycle();
        let first = app_mention_fixture("C123", "1710000003.000100", None, "start", false);
        lifecycle.handle_app_mention(&first).unwrap();
        let update = app_mention_fixture(
            "C123",
            "1710000003.000200",
            Some("1710000003.000100"),
            "<@UBOT> settings repo=https://github.com/acme/service.git branch=codex/custom resources=large auto_commit=true auto_push=true auto_pr=true",
            false,
        );

        let updated = lifecycle.handle_app_mention(&update).unwrap();

        match updated.action {
            SlackSessionLifecycleAction::SettingsUpdated { previous, update } => {
                assert_eq!(previous.repo_url, None);
                assert_eq!(
                    update.repo_url.as_deref(),
                    Some("https://github.com/acme/service.git")
                );
            }
            other => panic!("expected settings update, got {other:?}"),
        }
        assert_eq!(
            updated.session.settings.repo_url.as_deref(),
            Some("https://github.com/acme/service.git")
        );
        assert_eq!(
            updated.session.settings.branch.as_deref(),
            Some("codex/custom")
        );
        assert_eq!(
            updated.session.settings.resource_profile,
            SlackResourceProfile::Large
        );
        assert!(updated.session.settings.auto_commit);
        assert!(updated.session.settings.auto_push);
        assert!(updated.session.settings.auto_pr);

        let future = app_mention_fixture(
            "C123",
            "1710000003.000300",
            Some("1710000003.000100"),
            "next run",
            false,
        );
        let reused = lifecycle.handle_app_mention(&future).unwrap();
        assert_eq!(reused.action, SlackSessionLifecycleAction::Reused);
        assert_eq!(
            reused.session.settings.repo_url.as_deref(),
            Some("https://github.com/acme/service.git")
        );
        assert_eq!(reused.session.settings_revision, 1);
    }

    #[test]
    fn fixture_provisioning_failure_reporting_happens_once() {
        let mut lifecycle = lifecycle();
        let payload = app_mention_fixture("C123", "1710000004.000100", None, "start", false);
        let created = lifecycle.handle_app_mention(&payload).unwrap();
        lifecycle
            .mark_provisioning(&created.session.thread_key)
            .unwrap();

        let first = lifecycle
            .mark_provisioning_failed(&created.session.thread_key, "sandbox quota exceeded")
            .unwrap();
        let second = lifecycle
            .mark_provisioning_failed(&created.session.thread_key, "sandbox quota exceeded")
            .unwrap();

        assert!(first.should_notify);
        assert!(!second.should_notify);
        let stored = lifecycle
            .store()
            .get_by_thread_key(&created.session.thread_key)
            .unwrap();
        assert_eq!(
            stored.sandbox.lifecycle_state,
            SlackSandboxLifecycleState::Failed
        );
        assert_eq!(
            stored.sandbox.failure_message.as_deref(),
            Some("sandbox quota exceeded")
        );
    }

    #[test]
    fn slack_connect_channels_are_blocked_by_default() {
        let mut lifecycle = lifecycle();
        let payload = app_mention_fixture("CEXT", "1710000005.000100", None, "start", true);

        let err = lifecycle.handle_app_mention(&payload).unwrap_err();

        match err {
            SlackSessionLifecycleError::PermissionDenied(failure) => {
                assert_eq!(failure.reason, SlackPermissionDeniedReason::SlackConnect);
            }
            other => panic!("expected Slack Connect permission failure, got {other:?}"),
        }
    }

    #[test]
    fn ready_transition_loads_skills_after_provisioning() {
        let defaults = SlackSessionDefaults {
            settings: SlackSessionSettings {
                global_skill_refs: vec!["global:rust".to_string(), "repo:codex".to_string()],
                ..SlackSessionSettings::default()
            },
        };
        let mut lifecycle = SlackSessionLifecycle::new(
            InMemorySlackSessionLifecycleStore::default(),
            SlackChannelPolicy::default(),
            defaults,
        );
        let payload = app_mention_fixture("C123", "1710000006.000100", None, "start", false);
        let created = lifecycle.handle_app_mention(&payload).unwrap();
        lifecycle
            .mark_provisioning(&created.session.thread_key)
            .unwrap();

        let ready = lifecycle
            .mark_ready(
                &created.session.thread_key,
                SlackSandboxReady {
                    sandbox_id: "sbx_123".to_string(),
                    working_directory: "/workspace/service".to_string(),
                    current_branch: Some("codex/custom".to_string()),
                    did_setup_workspace: true,
                },
            )
            .unwrap();

        assert_eq!(
            ready.sandbox.lifecycle_state,
            SlackSandboxLifecycleState::Ready
        );
        assert_eq!(ready.sandbox.skill_load_state, SlackSkillLoadState::Loaded);
        assert_eq!(
            ready.sandbox.loaded_skill_refs,
            vec!["global:rust".to_string(), "repo:codex".to_string()]
        );
    }

    #[test]
    fn fixture_thread_id_encoding_and_chat_thread_reconstruction() {
        let thread_id = encode_thread_id("C123", "1710000007.000100");
        let adapter: Arc<dyn Adapter> = Arc::new(SlackAdapter::new(SlackAdapterOptions::new(
            "xoxb-test",
            "signing-secret",
        )));
        let thread = Thread::new(adapter.clone(), thread_id.clone())
            .with_channel_id("slack:C123")
            .with_channel_visibility(ChannelVisibility::Workspace);
        let json = thread.to_json();

        let restored = Thread::from_json(&json, adapter);

        assert_eq!(thread_id, "slack:C123:1710000007.000100");
        assert_eq!(restored.thread_id(), thread_id);
        assert_eq!(restored.channel_id(), Some("slack:C123"));
        assert_eq!(restored.channel_visibility(), ChannelVisibility::Workspace);
    }
}
