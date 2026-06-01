use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use serde_json::Value;

/// Chat statuses used by the Open Agents web surface and mirrored by the Rust
/// service when deciding whether a run is visibly active.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatUiStatus {
    Submitted,
    Streaming,
    Ready,
    Error,
}

/// Returns true for statuses that still have an in-flight user turn.
pub fn is_chat_in_flight(status: ChatUiStatus) -> bool {
    matches!(status, ChatUiStatus::Submitted | ChatUiStatus::Streaming)
}

/// Returns true when a UI-message part should be visible to a user.
pub fn has_renderable_assistant_part(part: &Value) -> bool {
    match part_type(part) {
        Some("text") => text_field(part, "text").is_some_and(|text| !text.is_empty()),
        Some("reasoning") => {
            text_field(part, "text").is_some_and(|text| !text.is_empty())
                || string_field(part, "state") == Some("streaming")
        }
        Some("dynamic-tool") => true,
        Some(part_type) if part_type.starts_with("tool-") => true,
        Some("data-commit") | Some("data-pr") => should_render_git_data_part(part),
        _ => false,
    }
}

/// Commit data with a skipped status is hidden; PR data and other commit
/// statuses remain visible.
pub fn should_render_git_data_part(part: &Value) -> bool {
    !(part_type(part) == Some("data-commit") && git_status(part) == Some("skipped"))
}

pub fn should_show_thinking_indicator(
    status: ChatUiStatus,
    has_assistant_renderable_content: bool,
    last_message_role: Option<&str>,
) -> bool {
    if !is_chat_in_flight(status) {
        return false;
    }
    if last_message_role != Some("assistant") {
        return true;
    }
    !has_assistant_renderable_content
}

pub fn should_use_chat_list_streaming_state(
    status: ChatUiStatus,
    has_chat_list_streaming: bool,
    user_stopped: bool,
    has_assistant_renderable_content: bool,
    last_message_role: Option<&str>,
) -> bool {
    if user_stopped || is_chat_in_flight(status) || !has_chat_list_streaming {
        return false;
    }
    if last_message_role != Some("assistant") {
        return true;
    }
    !has_assistant_renderable_content
}

pub fn should_keep_collapsed_reasoning_streaming(
    is_message_streaming: bool,
    has_streaming_reasoning_part: bool,
    has_renderable_content_after_group: bool,
) -> bool {
    if !is_message_streaming {
        return false;
    }
    if has_streaming_reasoning_part {
        return true;
    }
    !has_renderable_content_after_group
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitAction {
    Commit,
    PullRequest,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NavbarGitActionState {
    pub pending_action: Option<GitAction>,
    pub label: Option<&'static str>,
    pub latest_commit_part: Option<Value>,
    pub latest_pr_part: Option<Value>,
}

impl NavbarGitActionState {
    fn idle() -> Self {
        Self {
            pending_action: None,
            label: None,
            latest_commit_part: None,
            latest_pr_part: None,
        }
    }
}

/// Finds the latest assistant git data and returns the user-visible navbar
/// state for pending commit/PR finalization.
pub fn get_navbar_git_action_state(messages: &[Value]) -> NavbarGitActionState {
    let Some(message) = latest_assistant_git_message(messages) else {
        return NavbarGitActionState::idle();
    };
    let Some(parts) = message.get("parts").and_then(Value::as_array) else {
        return NavbarGitActionState::idle();
    };

    let mut latest_commit_part = None;
    let mut latest_pr_part = None;
    let mut pending_action = None;
    for part in parts.iter().rev() {
        match part_type(part) {
            Some("data-commit") => {
                latest_commit_part.get_or_insert_with(|| part.clone());
                if pending_action.is_none() && git_status(part) == Some("pending") {
                    pending_action = Some(GitAction::Commit);
                }
            }
            Some("data-pr") => {
                latest_pr_part.get_or_insert_with(|| part.clone());
                if pending_action.is_none() && git_status(part) == Some("pending") {
                    pending_action = Some(GitAction::PullRequest);
                }
            }
            _ => {}
        }
    }

    NavbarGitActionState {
        pending_action,
        label: pending_action.map(git_action_label),
        latest_commit_part,
        latest_pr_part,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitFinalizationState {
    pub is_finalizing: bool,
    pub label: Option<&'static str>,
}

pub fn get_git_finalization_state(
    status: ChatUiStatus,
    last_message_role: Option<&str>,
    last_message_parts: Option<&[Value]>,
) -> GitFinalizationState {
    if !is_chat_in_flight(status) || last_message_role != Some("assistant") {
        return GitFinalizationState {
            is_finalizing: false,
            label: None,
        };
    }
    let Some(parts) = last_message_parts else {
        return GitFinalizationState {
            is_finalizing: false,
            label: None,
        };
    };
    let git_parts = parts
        .iter()
        .filter(|part| matches!(part_type(part), Some("data-commit" | "data-pr")))
        .collect::<Vec<_>>();
    if git_parts.is_empty() {
        return GitFinalizationState {
            is_finalizing: false,
            label: None,
        };
    }
    if git_parts
        .iter()
        .any(|part| part_type(part) == Some("data-pr") && git_status(part) == Some("pending"))
    {
        return GitFinalizationState {
            is_finalizing: true,
            label: Some(git_action_label(GitAction::PullRequest)),
        };
    }
    if git_parts
        .iter()
        .any(|part| part_type(part) == Some("data-commit") && git_status(part) == Some("pending"))
    {
        return GitFinalizationState {
            is_finalizing: true,
            label: Some(git_action_label(GitAction::Commit)),
        };
    }
    GitFinalizationState {
        is_finalizing: true,
        label: Some("Finalizing git actions..."),
    }
}

pub fn should_refresh_after_ready_transition(
    previous_status: Option<ChatUiStatus>,
    status: ChatUiStatus,
    has_assistant_renderable_content: bool,
) -> bool {
    previous_status == Some(ChatUiStatus::Submitted)
        && status == ChatUiStatus::Ready
        && has_assistant_renderable_content
}

/// Removes exact duplicate OpenAI/Azure reasoning parts while preserving
/// distinct summaries for the same provider item id.
pub fn dedupe_message_reasoning(message: &Value) -> Cow<'_, Value> {
    let Some(parts) = message.get("parts").and_then(Value::as_array) else {
        return Cow::Borrowed(message);
    };

    let mut seen = BTreeSet::new();
    let mut has_duplicates = false;
    for part in parts {
        let Some(item_id) = reasoning_item_id(part) else {
            continue;
        };
        let key = reasoning_key(item_id, text_field(part, "text").unwrap_or(""));
        if !seen.insert(key) {
            has_duplicates = true;
            break;
        }
    }
    if !has_duplicates {
        return Cow::Borrowed(message);
    }

    let mut deduped = BTreeSet::new();
    let filtered_parts = parts
        .iter()
        .filter(|part| {
            let Some(item_id) = reasoning_item_id(part) else {
                return true;
            };
            let key = reasoning_key(item_id, text_field(part, "text").unwrap_or(""));
            deduped.insert(key)
        })
        .cloned()
        .collect::<Vec<_>>();

    let mut output = message.clone();
    if let Some(object) = output.as_object_mut() {
        object.insert("parts".to_string(), Value::Array(filtered_parts));
    }
    Cow::Owned(output)
}

#[derive(Clone, Default)]
pub struct WorkspaceStatusStore {
    inner: Arc<Mutex<WorkspaceStatusInner>>,
}

#[derive(Default)]
struct WorkspaceStatusInner {
    statuses: HashMap<String, Value>,
    listeners: HashMap<String, BTreeMap<usize, WorkspaceStatusListener>>,
    next_listener_id: usize,
}

type WorkspaceStatusListener = Arc<dyn Fn() + Send + Sync>;

impl WorkspaceStatusStore {
    pub fn set_chat_workspace_status(&self, chat_id: impl Into<String>, status: Value) {
        let chat_id = chat_id.into();
        let listeners = {
            let mut inner = self
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            inner.statuses.insert(chat_id.clone(), status);
            listeners_for_chat(&inner, &chat_id)
        };
        notify_listeners(listeners);
    }

    pub fn clear_chat_workspace_status(&self, chat_id: &str) {
        let listeners = {
            let mut inner = self
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let had_status = inner.statuses.remove(chat_id).is_some();
            if had_status {
                listeners_for_chat(&inner, chat_id)
            } else {
                Vec::new()
            }
        };
        notify_listeners(listeners);
    }

    pub fn get_chat_workspace_status_snapshot(&self, chat_id: &str) -> Option<Value> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .statuses
            .get(chat_id)
            .cloned()
    }

    pub fn subscribe_chat_workspace_status(
        &self,
        chat_id: impl Into<String>,
        listener: impl Fn() + Send + Sync + 'static,
    ) -> WorkspaceStatusSubscription {
        let chat_id = chat_id.into();
        let listener_id = {
            let mut inner = self
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let listener_id = inner.next_listener_id;
            inner.next_listener_id += 1;
            inner
                .listeners
                .entry(chat_id.clone())
                .or_default()
                .insert(listener_id, Arc::new(listener));
            listener_id
        };
        WorkspaceStatusSubscription {
            store: self.clone(),
            chat_id,
            listener_id,
            active: true,
        }
    }

    fn unsubscribe(&self, chat_id: &str, listener_id: usize) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(listeners) = inner.listeners.get_mut(chat_id) {
            listeners.remove(&listener_id);
            if listeners.is_empty() {
                inner.listeners.remove(chat_id);
            }
        }
    }
}

pub struct WorkspaceStatusSubscription {
    store: WorkspaceStatusStore,
    chat_id: String,
    listener_id: usize,
    active: bool,
}

impl WorkspaceStatusSubscription {
    pub fn unsubscribe(mut self) {
        if self.active {
            self.store.unsubscribe(&self.chat_id, self.listener_id);
            self.active = false;
        }
    }
}

impl Drop for WorkspaceStatusSubscription {
    fn drop(&mut self) {
        if self.active {
            self.store.unsubscribe(&self.chat_id, self.listener_id);
            self.active = false;
        }
    }
}

pub struct ChatRouteCleanupDependencies<'a> {
    pub abort_transport: &'a dyn Fn(&str),
    pub remove_instance: &'a dyn Fn(&str),
    pub clear_workspace_status: Option<&'a dyn Fn(&str)>,
    pub stop_stream: Option<&'a dyn Fn(&str)>,
}

/// Performs local route teardown without sending a server-side stop signal.
pub fn cleanup_chat_route_on_unmount(
    chat_id: &str,
    dependencies: ChatRouteCleanupDependencies<'_>,
) {
    (dependencies.abort_transport)(chat_id);
    (dependencies.remove_instance)(chat_id);
    if let Some(clear_workspace_status) = dependencies.clear_workspace_status {
        clear_workspace_status(chat_id);
    }
    let _ = dependencies.stop_stream;
}

pub const MERGE_READINESS_TRANSIENT_MAX_POLLS: usize = 6;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeReadinessPollingState {
    pub can_merge: bool,
    pub reasons: Vec<String>,
    pub has_pr: bool,
    pub check_runs: usize,
    pub required_total: usize,
    pub pending: usize,
    pub failed: usize,
}

impl Default for MergeReadinessPollingState {
    fn default() -> Self {
        Self {
            can_merge: false,
            reasons: Vec::new(),
            has_pr: true,
            check_runs: 0,
            required_total: 0,
            pending: 0,
            failed: 0,
        }
    }
}

pub fn should_increment_merge_readiness_transient_poll_count(
    readiness: Option<&MergeReadinessPollingState>,
) -> bool {
    let Some(readiness) = readiness else {
        return false;
    };
    if readiness.can_merge || readiness.pending > 0 || readiness.failed > 0 {
        return false;
    }
    has_transient_merge_readiness_reason(readiness)
}

pub fn should_poll_merge_readiness(
    readiness: Option<&MergeReadinessPollingState>,
    transient_poll_count: usize,
) -> bool {
    let Some(readiness) = readiness else {
        return false;
    };
    if !readiness.has_pr {
        return false;
    }
    if readiness.pending > 0 {
        return true;
    }
    if readiness.can_merge
        || readiness.failed > 0
        || !has_transient_merge_readiness_reason(readiness)
    {
        return false;
    }
    transient_poll_count < MERGE_READINESS_TRANSIENT_MAX_POLLS
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CancelableStreamError {
    Undefined,
    Named { name: String, message: String },
    Other(String),
}

impl CancelableStreamError {
    pub fn named(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Named {
            name: name.into(),
            message: message.into(),
        }
    }
}

pub fn is_abort_like_stream_error(error: &CancelableStreamError) -> bool {
    match error {
        CancelableStreamError::Undefined => true,
        CancelableStreamError::Named { name, message } => {
            name == "AbortError"
                || name == "ResponseAborted"
                || (message.to_ascii_lowercase().contains("status code 404")
                    && message.to_ascii_lowercase().contains("not ok"))
        }
        CancelableStreamError::Other(_) => false,
    }
}

#[derive(Clone, Debug)]
pub struct CancelableReadableStream<T> {
    source: VecDeque<Result<T, CancelableStreamError>>,
    cancelled: bool,
    completed: bool,
}

impl<T> CancelableReadableStream<T> {
    pub fn new(source: impl IntoIterator<Item = Result<T, CancelableStreamError>>) -> Self {
        Self {
            source: source.into_iter().collect(),
            cancelled: false,
            completed: false,
        }
    }

    pub fn read_next(&mut self) -> Result<Option<T>, CancelableStreamError> {
        if self.cancelled || self.completed {
            return Ok(None);
        }
        match self.source.pop_front() {
            Some(Ok(chunk)) => Ok(Some(chunk)),
            Some(Err(error)) if self.cancelled || is_abort_like_stream_error(&error) => {
                self.completed = true;
                Ok(None)
            }
            Some(Err(error)) => {
                self.completed = true;
                Err(error)
            }
            None => {
                self.completed = true;
                Ok(None)
            }
        }
    }

    pub fn cancel(&mut self) {
        self.cancelled = true;
        self.source.clear();
    }
}

fn latest_assistant_git_message(messages: &[Value]) -> Option<&Value> {
    messages.iter().rev().find(|message| {
        string_field(message, "role") == Some("assistant")
            && message
                .get("parts")
                .and_then(Value::as_array)
                .is_some_and(|parts| {
                    parts
                        .iter()
                        .any(|part| matches!(part_type(part), Some("data-commit" | "data-pr")))
                })
    })
}

fn git_action_label(action: GitAction) -> &'static str {
    match action {
        GitAction::Commit => "Creating commit...",
        GitAction::PullRequest => "Creating pull request...",
    }
}

fn has_transient_merge_readiness_reason(readiness: &MergeReadinessPollingState) -> bool {
    readiness.reasons.iter().any(|reason| {
        matches!(
            reason.as_str(),
            "GitHub is still calculating mergeability"
                | "Required checks are still pending"
                | "Required checks are still in progress"
                | "Branch protection requirements are not yet satisfied"
        )
    })
}

fn listeners_for_chat(inner: &WorkspaceStatusInner, chat_id: &str) -> Vec<WorkspaceStatusListener> {
    inner
        .listeners
        .get(chat_id)
        .map(|listeners| listeners.values().cloned().collect())
        .unwrap_or_default()
}

fn notify_listeners(listeners: Vec<WorkspaceStatusListener>) {
    for listener in listeners {
        listener();
    }
}

fn reasoning_item_id(part: &Value) -> Option<&str> {
    if part_type(part) != Some("reasoning") {
        return None;
    }
    part.pointer("/providerMetadata/openai/itemId")
        .or_else(|| part.pointer("/providerMetadata/azure/itemId"))
        .and_then(Value::as_str)
}

fn reasoning_key(item_id: &str, text: &str) -> String {
    format!("{item_id}\0{text}")
}

fn git_status(part: &Value) -> Option<&str> {
    part.get("data")
        .and_then(|data| data.get("status"))
        .and_then(Value::as_str)
}

fn part_type(part: &Value) -> Option<&str> {
    string_field(part, "type")
}

fn text_field<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value.get(field).and_then(Value::as_str)
}

fn string_field<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value.get(field).and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use serde_json::json;

    use super::*;

    fn message(role: &str, parts: Vec<Value>) -> Value {
        json!({
            "id": format!("{role}-message"),
            "role": role,
            "parts": parts,
        })
    }

    fn commit(status: &str) -> Value {
        json!({
            "type": "data-commit",
            "data": { "status": status }
        })
    }

    fn pr(status: &str) -> Value {
        json!({
            "type": "data-pr",
            "data": { "status": status }
        })
    }

    fn reasoning(item_id: &str, text: &str) -> Value {
        json!({
            "type": "reasoning",
            "text": text,
            "providerMetadata": {
                "openai": { "itemId": item_id }
            }
        })
    }

    #[test]
    fn chat_streaming_state_matches_upstream_inflight_rendering_git_and_refresh_cases() {
        assert!(is_chat_in_flight(ChatUiStatus::Submitted));
        assert!(is_chat_in_flight(ChatUiStatus::Streaming));
        assert!(!is_chat_in_flight(ChatUiStatus::Ready));

        assert!(has_renderable_assistant_part(
            &json!({"type": "text", "text": "hello"})
        ));
        assert!(!has_renderable_assistant_part(
            &json!({"type": "text", "text": ""})
        ));
        assert!(has_renderable_assistant_part(
            &json!({"type": "reasoning", "text": "", "state": "streaming"})
        ));
        assert!(has_renderable_assistant_part(&json!({"type": "tool-bash"})));
        assert!(!has_renderable_assistant_part(&commit("skipped")));
        assert!(has_renderable_assistant_part(&commit("pending")));

        assert!(!should_show_thinking_indicator(
            ChatUiStatus::Submitted,
            true,
            Some("assistant")
        ));
        assert!(should_show_thinking_indicator(
            ChatUiStatus::Streaming,
            false,
            Some("assistant")
        ));
        assert!(should_show_thinking_indicator(
            ChatUiStatus::Submitted,
            false,
            Some("user")
        ));

        assert!(should_use_chat_list_streaming_state(
            ChatUiStatus::Ready,
            true,
            false,
            false,
            Some("assistant")
        ));
        assert!(!should_use_chat_list_streaming_state(
            ChatUiStatus::Ready,
            true,
            true,
            false,
            Some("assistant")
        ));
        assert!(!should_use_chat_list_streaming_state(
            ChatUiStatus::Streaming,
            true,
            false,
            false,
            Some("assistant")
        ));

        let messages = vec![message("assistant", vec![commit("pending"), pr("pending")])];
        let nav = get_navbar_git_action_state(&messages);
        assert_eq!(nav.pending_action, Some(GitAction::PullRequest));
        assert_eq!(nav.label, Some("Creating pull request..."));
        assert!(nav.latest_commit_part.is_some());
        assert!(nav.latest_pr_part.is_some());

        let messages = vec![
            message("assistant", vec![commit("pending")]),
            message("assistant", vec![commit("success")]),
        ];
        let nav = get_navbar_git_action_state(&messages);
        assert_eq!(nav.pending_action, None);
        assert_eq!(nav.label, None);

        let parts = vec![commit("success")];
        assert_eq!(
            get_git_finalization_state(ChatUiStatus::Streaming, Some("assistant"), Some(&parts)),
            GitFinalizationState {
                is_finalizing: true,
                label: Some("Finalizing git actions...")
            }
        );
        assert_eq!(
            get_git_finalization_state(ChatUiStatus::Ready, Some("assistant"), Some(&parts)),
            GitFinalizationState {
                is_finalizing: false,
                label: None
            }
        );

        assert!(should_keep_collapsed_reasoning_streaming(true, true, true));
        assert!(should_keep_collapsed_reasoning_streaming(
            true, false, false
        ));
        assert!(!should_keep_collapsed_reasoning_streaming(
            false, true, false
        ));
        assert!(should_refresh_after_ready_transition(
            Some(ChatUiStatus::Submitted),
            ChatUiStatus::Ready,
            true
        ));
        assert!(!should_refresh_after_ready_transition(
            Some(ChatUiStatus::Streaming),
            ChatUiStatus::Ready,
            true
        ));
    }

    #[test]
    fn dedupe_message_reasoning_matches_openai_azure_and_immutability_cases() {
        let no_reasoning = message("assistant", vec![json!({"type": "text", "text": "done"})]);
        assert!(matches!(
            dedupe_message_reasoning(&no_reasoning),
            Cow::Borrowed(_)
        ));

        let unique = message(
            "assistant",
            vec![reasoning("item-1", "a"), reasoning("item-2", "a")],
        );
        assert!(matches!(
            dedupe_message_reasoning(&unique),
            Cow::Borrowed(_)
        ));

        let multi_summary = message(
            "assistant",
            vec![reasoning("item-1", "a"), reasoning("item-1", "b")],
        );
        assert!(matches!(
            dedupe_message_reasoning(&multi_summary),
            Cow::Borrowed(_)
        ));

        let duplicate = message(
            "assistant",
            vec![
                reasoning("item-1", "a"),
                json!({"type": "text", "text": "kept"}),
                reasoning("item-1", "a"),
                json!({
                    "type": "reasoning",
                    "text": "azure",
                    "providerMetadata": { "azure": { "itemId": "azure-1" } }
                }),
                json!({
                    "type": "reasoning",
                    "text": "azure",
                    "providerMetadata": { "azure": { "itemId": "azure-1" } }
                }),
            ],
        );
        let original = duplicate.clone();
        let deduped = dedupe_message_reasoning(&duplicate).into_owned();
        assert_eq!(duplicate, original);
        let parts = deduped
            .get("parts")
            .and_then(Value::as_array)
            .expect("parts array");
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[1], json!({"type": "text", "text": "kept"}));
    }

    #[test]
    fn workspace_status_store_tracks_latest_status_and_subscribers() {
        let store = WorkspaceStatusStore::default();
        let notifications = Arc::new(AtomicUsize::new(0));
        let notifications_for_listener = Arc::clone(&notifications);
        let subscription = store.subscribe_chat_workspace_status("chat-1", move || {
            notifications_for_listener.fetch_add(1, Ordering::SeqCst);
        });

        store.set_chat_workspace_status("chat-1", json!({"status": "setting-up"}));
        assert_eq!(
            store.get_chat_workspace_status_snapshot("chat-1"),
            Some(json!({"status": "setting-up"}))
        );
        assert_eq!(notifications.load(Ordering::SeqCst), 1);

        store.set_chat_workspace_status("chat-1", json!({"status": "ready"}));
        assert_eq!(
            store.get_chat_workspace_status_snapshot("chat-1"),
            Some(json!({"status": "ready"}))
        );
        assert_eq!(notifications.load(Ordering::SeqCst), 2);

        subscription.unsubscribe();
        store.clear_chat_workspace_status("chat-1");
        assert_eq!(store.get_chat_workspace_status_snapshot("chat-1"), None);
        assert_eq!(notifications.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn chat_route_cleanup_clears_local_state_without_stopping_active_run() {
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let stop_calls = Arc::new(AtomicUsize::new(0));
        let abort_calls = Arc::clone(&calls);
        let remove_calls = Arc::clone(&calls);
        let clear_calls = Arc::clone(&calls);
        let stop_calls_for_dep = Arc::clone(&stop_calls);

        let abort = move |chat_id: &str| {
            abort_calls.lock().unwrap().push(format!("abort:{chat_id}"));
        };
        let remove = move |chat_id: &str| {
            remove_calls
                .lock()
                .unwrap()
                .push(format!("remove:{chat_id}"));
        };
        let clear = move |chat_id: &str| {
            clear_calls.lock().unwrap().push(format!("clear:{chat_id}"));
        };
        let stop = move |_chat_id: &str| {
            stop_calls_for_dep.fetch_add(1, Ordering::SeqCst);
        };

        cleanup_chat_route_on_unmount(
            "chat-1",
            ChatRouteCleanupDependencies {
                abort_transport: &abort,
                remove_instance: &remove,
                clear_workspace_status: Some(&clear),
                stop_stream: Some(&stop),
            },
        );

        assert_eq!(
            calls.lock().unwrap().as_slice(),
            ["abort:chat-1", "remove:chat-1", "clear:chat-1"]
        );
        assert_eq!(stop_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn merge_readiness_polling_matches_pending_warmup_transient_and_blocked_cases() {
        let pending = MergeReadinessPollingState {
            required_total: 2,
            pending: 1,
            ..MergeReadinessPollingState::default()
        };
        assert!(should_poll_merge_readiness(
            Some(&pending),
            MERGE_READINESS_TRANSIENT_MAX_POLLS
        ));

        let warmup = MergeReadinessPollingState {
            reasons: vec!["Branch protection requirements are not yet satisfied".to_string()],
            ..MergeReadinessPollingState::default()
        };
        assert!(should_poll_merge_readiness(Some(&warmup), 0));

        let stale = MergeReadinessPollingState {
            reasons: vec!["Branch protection requirements are not yet satisfied".to_string()],
            check_runs: 1,
            required_total: 1,
            ..MergeReadinessPollingState::default()
        };
        assert!(should_poll_merge_readiness(Some(&stale), 0));
        assert!(should_increment_merge_readiness_transient_poll_count(Some(
            &stale
        )));

        let exhausted = MergeReadinessPollingState {
            reasons: vec!["GitHub is still calculating mergeability".to_string()],
            ..MergeReadinessPollingState::default()
        };
        assert!(!should_poll_merge_readiness(
            Some(&exhausted),
            MERGE_READINESS_TRANSIENT_MAX_POLLS
        ));

        let failing = MergeReadinessPollingState {
            reasons: vec!["Branch protection requirements are not yet satisfied".to_string()],
            failed: 1,
            ..MergeReadinessPollingState::default()
        };
        assert!(!should_poll_merge_readiness(Some(&failing), 0));
        assert!(!should_increment_merge_readiness_transient_poll_count(
            Some(&pending)
        ));

        let blocked = MergeReadinessPollingState {
            reasons: vec!["Pull request has merge conflicts".to_string()],
            ..MergeReadinessPollingState::default()
        };
        assert!(!should_poll_merge_readiness(Some(&blocked), 0));
        assert!(!should_poll_merge_readiness(None, 0));
    }

    #[test]
    fn cancelable_readable_stream_semantics_match_forwarding_abort_and_idempotent_cancel_cases() {
        let mut stream = CancelableReadableStream::new([Ok("a"), Ok("b")]);
        assert_eq!(stream.read_next(), Ok(Some("a")));
        assert_eq!(stream.read_next(), Ok(Some("b")));
        assert_eq!(stream.read_next(), Ok(None));
        stream.cancel();
        assert_eq!(stream.read_next(), Ok(None));

        let mut empty = CancelableReadableStream::<&str>::new([]);
        assert_eq!(empty.read_next(), Ok(None));

        let mut cancelled = CancelableReadableStream::new([Ok("a")]);
        cancelled.cancel();
        cancelled.cancel();
        assert_eq!(cancelled.read_next(), Ok(None));

        for error in [
            CancelableStreamError::named("AbortError", "aborted"),
            CancelableStreamError::named("ResponseAborted", "response aborted"),
            CancelableStreamError::Undefined,
            CancelableStreamError::named("Error", "status code 404 not ok"),
        ] {
            let mut stream = CancelableReadableStream::<&str>::new([Err(error)]);
            assert_eq!(stream.read_next(), Ok(None));
        }

        let mut stream = CancelableReadableStream::<&str>::new([Err(
            CancelableStreamError::Other("boom".to_string()),
        )]);
        assert_eq!(
            stream.read_next(),
            Err(CancelableStreamError::Other("boom".to_string()))
        );
    }
}
