use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use time::OffsetDateTime;

use crate::types::{
    ActiveRunRepository, ChatMessageRecord, ChatReadRepository, ChatRecord, ChatRepository,
    CreateChatInput, CreateChatMessageInput, CreateIdempotencyRecordInput, CreateRunInput,
    CreateRunStepInput, CreateSessionInput, CreateShareInput, CreateSlackThreadMappingInput,
    CreateUsageRecordInput, IdempotencyRecord, IdempotencyRepository, InsertIfAbsent,
    MessageRepository, PersistenceError, PersistenceResult, RunRecord, RunRepository,
    RunStatusUpdate, RunStepRecord, SandboxStateRepository, SandboxStateUpdate, SessionRecord,
    SessionRepository, ShareRecord, ShareRepository, SlackThreadKey, SlackThreadMappingRecord,
    SlackThreadMappingRepository, UsageRecord, UsageRepository,
};

/// In-memory implementation of the Open Agents persistence contracts.
#[derive(Debug, Clone, Default)]
pub struct MemoryPersistenceStore {
    inner: Arc<Mutex<MemoryStoreInner>>,
}

#[derive(Debug, Default)]
struct MemoryStoreInner {
    sessions: HashMap<String, SessionRecord>,
    chats: HashMap<String, ChatRecord>,
    chat_ids_by_session: HashMap<String, Vec<String>>,
    messages: HashMap<String, ChatMessageRecord>,
    message_ids_by_chat: HashMap<String, Vec<String>>,
    runs: HashMap<String, RunRecord>,
    steps: HashMap<String, RunStepRecord>,
    step_ids_by_run: HashMap<String, Vec<String>>,
    step_ids_by_run_number: HashMap<(String, u32), String>,
    usage: HashMap<String, UsageRecord>,
    usage_ids_by_run: HashMap<String, Vec<String>>,
    slack_mappings: HashMap<SlackThreadKey, SlackThreadMappingRecord>,
    slack_mapping_keys_by_chat: HashMap<String, SlackThreadKey>,
    shares: HashMap<String, ShareRecord>,
    share_ids_by_chat: HashMap<String, String>,
    read_markers: HashMap<(String, String), crate::types::ChatReadRecord>,
    idempotency: HashMap<(String, String), IdempotencyRecord>,
    next_sequence: u64,
}

fn now_utc() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

fn conflict(message: impl Into<String>) -> PersistenceError {
    PersistenceError::Conflict(message.into())
}

fn invalid_reference(entity: &'static str, id: impl Into<String>) -> PersistenceError {
    PersistenceError::InvalidReference {
        entity,
        id: id.into(),
    }
}

impl MemoryStoreInner {
    fn next_sequence(&mut self) -> u64 {
        self.next_sequence += 1;
        self.next_sequence
    }
}

impl MemoryPersistenceStore {
    /// Construct an empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }

    fn with_inner<T>(
        &self,
        operation: impl FnOnce(&mut MemoryStoreInner) -> PersistenceResult<T>,
    ) -> PersistenceResult<T> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        operation(&mut inner)
    }
}

#[async_trait]
impl SessionRepository for MemoryPersistenceStore {
    async fn create_session_with_initial_chat(
        &self,
        session: CreateSessionInput,
        chat: CreateChatInput,
    ) -> PersistenceResult<(SessionRecord, ChatRecord)> {
        self.with_inner(|inner| {
            if session.id != chat.session_id {
                return Err(conflict("initial chat must reference the created session"));
            }
            if inner.sessions.contains_key(&session.id) {
                return Err(conflict(format!("session already exists: {}", session.id)));
            }
            if inner.chats.contains_key(&chat.id) {
                return Err(conflict(format!("chat already exists: {}", chat.id)));
            }

            let timestamp = now_utc();
            let session_record = SessionRecord {
                id: session.id,
                user_id: session.user_id,
                title: session.title,
                status: session.status,
                repo_owner: session.repo_owner,
                repo_name: session.repo_name,
                branch: session.branch,
                clone_url: session.clone_url,
                is_new_branch: session.is_new_branch,
                lifecycle_state: session.lifecycle_state,
                lifecycle_version: 0,
                lifecycle_run_id: None,
                sandbox_provisioning_run_id: None,
                lifecycle_error: None,
                sandbox_state: session.sandbox_state,
                metadata: session.metadata,
                created_at: timestamp,
                updated_at: timestamp,
            };
            let chat_record = ChatRecord {
                id: chat.id,
                session_id: chat.session_id,
                title: chat.title,
                model_id: chat.model_id,
                active_run_id: None,
                last_assistant_message_at: None,
                created_at: timestamp,
                updated_at: timestamp,
            };

            inner
                .chat_ids_by_session
                .entry(session_record.id.clone())
                .or_default()
                .push(chat_record.id.clone());
            inner
                .message_ids_by_chat
                .entry(chat_record.id.clone())
                .or_default();
            inner
                .sessions
                .insert(session_record.id.clone(), session_record.clone());
            inner
                .chats
                .insert(chat_record.id.clone(), chat_record.clone());

            Ok((session_record, chat_record))
        })
    }

    async fn get_session(&self, session_id: &str) -> PersistenceResult<Option<SessionRecord>> {
        self.with_inner(|inner| Ok(inner.sessions.get(session_id).cloned()))
    }

    async fn update_session_lifecycle(
        &self,
        session_id: &str,
        update: crate::types::SessionLifecycleUpdate,
    ) -> PersistenceResult<Option<SessionRecord>> {
        self.with_inner(|inner| {
            let Some(session) = inner.sessions.get_mut(session_id) else {
                return Ok(None);
            };
            session.lifecycle_state = update.lifecycle_state;
            if let Some(status) = update.status {
                session.status = status;
            }
            session.lifecycle_error = update.lifecycle_error;
            session.lifecycle_version += 1;
            session.updated_at = now_utc();
            Ok(Some(session.clone()))
        })
    }

    async fn used_session_titles(&self, user_id: &str) -> PersistenceResult<Vec<String>> {
        self.with_inner(|inner| {
            let mut titles: Vec<_> = inner
                .sessions
                .values()
                .filter(|session| session.user_id == user_id)
                .map(|session| session.title.clone())
                .collect();
            titles.sort();
            titles.dedup();
            Ok(titles)
        })
    }
}

#[async_trait]
impl ChatRepository for MemoryPersistenceStore {
    async fn create_chat(&self, chat: CreateChatInput) -> PersistenceResult<ChatRecord> {
        self.with_inner(|inner| {
            if !inner.sessions.contains_key(&chat.session_id) {
                return Err(invalid_reference("session", chat.session_id));
            }
            if inner.chats.contains_key(&chat.id) {
                return Err(conflict(format!("chat already exists: {}", chat.id)));
            }

            let timestamp = now_utc();
            let record = ChatRecord {
                id: chat.id,
                session_id: chat.session_id,
                title: chat.title,
                model_id: chat.model_id,
                active_run_id: None,
                last_assistant_message_at: None,
                created_at: timestamp,
                updated_at: timestamp,
            };
            inner
                .chat_ids_by_session
                .entry(record.session_id.clone())
                .or_default()
                .push(record.id.clone());
            inner
                .message_ids_by_chat
                .entry(record.id.clone())
                .or_default();
            inner.chats.insert(record.id.clone(), record.clone());
            Ok(record)
        })
    }

    async fn get_chat(&self, chat_id: &str) -> PersistenceResult<Option<ChatRecord>> {
        self.with_inner(|inner| Ok(inner.chats.get(chat_id).cloned()))
    }

    async fn list_chats_by_session(&self, session_id: &str) -> PersistenceResult<Vec<ChatRecord>> {
        self.with_inner(|inner| {
            let records = inner
                .chat_ids_by_session
                .get(session_id)
                .into_iter()
                .flat_map(|ids| ids.iter())
                .filter_map(|id| inner.chats.get(id).cloned())
                .collect();
            Ok(records)
        })
    }

    async fn touch_chat(
        &self,
        chat_id: &str,
        assistant_activity_at: Option<OffsetDateTime>,
    ) -> PersistenceResult<Option<ChatRecord>> {
        self.with_inner(|inner| {
            let Some(chat) = inner.chats.get_mut(chat_id) else {
                return Ok(None);
            };
            let timestamp = assistant_activity_at.unwrap_or_else(now_utc);
            chat.updated_at = timestamp;
            if assistant_activity_at.is_some() {
                chat.last_assistant_message_at = assistant_activity_at;
            }
            Ok(Some(chat.clone()))
        })
    }

    async fn update_chat(
        &self,
        chat_id: &str,
        patch: crate::types::ChatPatch,
    ) -> PersistenceResult<Option<ChatRecord>> {
        self.with_inner(|inner| {
            let Some(chat) = inner.chats.get_mut(chat_id) else {
                return Ok(None);
            };
            if let Some(title) = patch.title {
                chat.title = title;
            }
            if let Some(model_id) = patch.model_id {
                chat.model_id = Some(model_id);
            }
            chat.updated_at = now_utc();
            Ok(Some(chat.clone()))
        })
    }

    async fn delete_chat(&self, chat_id: &str) -> PersistenceResult<bool> {
        self.with_inner(|inner| {
            let Some(chat) = inner.chats.remove(chat_id) else {
                return Ok(false);
            };
            if let Some(chat_ids) = inner.chat_ids_by_session.get_mut(&chat.session_id) {
                chat_ids.retain(|id| id != chat_id);
            }
            if let Some(message_ids) = inner.message_ids_by_chat.remove(chat_id) {
                for message_id in message_ids {
                    inner.messages.remove(&message_id);
                }
            }
            if let Some(share_id) = inner.share_ids_by_chat.remove(chat_id) {
                inner.shares.remove(&share_id);
            }
            inner
                .read_markers
                .retain(|key, _| key.1.as_str() != chat_id);
            Ok(true)
        })
    }

    async fn chat_summaries_by_session(
        &self,
        session_id: &str,
    ) -> PersistenceResult<Vec<crate::types::ChatSummaryRecord>> {
        self.with_inner(|inner| {
            let summaries = inner
                .chat_ids_by_session
                .get(session_id)
                .into_iter()
                .flat_map(|ids| ids.iter())
                .filter_map(|id| inner.chats.get(id))
                .map(|chat| crate::types::ChatSummaryRecord {
                    id: chat.id.clone(),
                    title: chat.title.clone(),
                })
                .collect();
            Ok(summaries)
        })
    }
}

#[async_trait]
impl MessageRepository for MemoryPersistenceStore {
    async fn create_message_if_absent(
        &self,
        message: CreateChatMessageInput,
    ) -> PersistenceResult<InsertIfAbsent<ChatMessageRecord>> {
        self.with_inner(|inner| {
            if !inner.chats.contains_key(&message.chat_id) {
                return Err(invalid_reference("chat", message.chat_id));
            }
            if let Some(existing) = inner.messages.get(&message.id) {
                if existing.chat_id != message.chat_id {
                    return Err(conflict(format!(
                        "message already belongs to another chat: {}",
                        message.id
                    )));
                }
                return Ok(InsertIfAbsent {
                    record: existing.clone(),
                    inserted: false,
                });
            }

            let sequence = inner.next_sequence();
            let record = ChatMessageRecord {
                id: message.id,
                chat_id: message.chat_id,
                role: message.role,
                parts: message.parts,
                sequence,
                created_at: message.created_at.unwrap_or_else(now_utc),
            };
            inner
                .message_ids_by_chat
                .entry(record.chat_id.clone())
                .or_default()
                .push(record.id.clone());
            inner.messages.insert(record.id.clone(), record.clone());

            Ok(InsertIfAbsent {
                record,
                inserted: true,
            })
        })
    }

    async fn list_chat_messages(&self, chat_id: &str) -> PersistenceResult<Vec<ChatMessageRecord>> {
        self.with_inner(|inner| {
            let mut records: Vec<_> = inner
                .message_ids_by_chat
                .get(chat_id)
                .into_iter()
                .flat_map(|ids| ids.iter())
                .filter_map(|id| inner.messages.get(id).cloned())
                .collect();
            records.sort_by_key(|message| (message.created_at, message.sequence));
            Ok(records)
        })
    }

    async fn get_chat_message(
        &self,
        message_id: &str,
    ) -> PersistenceResult<Option<ChatMessageRecord>> {
        self.with_inner(|inner| Ok(inner.messages.get(message_id).cloned()))
    }

    async fn upsert_chat_message_scoped(
        &self,
        message: CreateChatMessageInput,
    ) -> PersistenceResult<crate::types::UpsertChatMessageResult> {
        self.with_inner(|inner| {
            if !inner.chats.contains_key(&message.chat_id) {
                return Err(invalid_reference("chat", message.chat_id));
            }
            if let Some(existing) = inner.messages.get_mut(&message.id) {
                if existing.chat_id == message.chat_id && existing.role == message.role {
                    existing.parts = message.parts;
                    return Ok(crate::types::UpsertChatMessageResult {
                        status: crate::types::UpsertChatMessageStatus::Updated,
                        message: Some(existing.clone()),
                    });
                }
                return Ok(crate::types::UpsertChatMessageResult {
                    status: crate::types::UpsertChatMessageStatus::Conflict,
                    message: None,
                });
            }

            let sequence = inner.next_sequence();
            let record = ChatMessageRecord {
                id: message.id,
                chat_id: message.chat_id,
                role: message.role,
                parts: message.parts,
                sequence,
                created_at: message.created_at.unwrap_or_else(now_utc),
            };
            inner
                .message_ids_by_chat
                .entry(record.chat_id.clone())
                .or_default()
                .push(record.id.clone());
            inner.messages.insert(record.id.clone(), record.clone());

            Ok(crate::types::UpsertChatMessageResult {
                status: crate::types::UpsertChatMessageStatus::Inserted,
                message: Some(record),
            })
        })
    }

    async fn delete_chat_message_and_following(
        &self,
        chat_id: &str,
        message_id: &str,
    ) -> PersistenceResult<crate::types::DeleteChatMessageResult> {
        self.with_inner(|inner| {
            let Some(message_ids) = inner.message_ids_by_chat.get(chat_id).cloned() else {
                return Ok(crate::types::DeleteChatMessageResult::NotFound);
            };
            let Some(start_index) = message_ids.iter().position(|id| id == message_id) else {
                return Ok(crate::types::DeleteChatMessageResult::NotFound);
            };
            let Some(target) = inner.messages.get(message_id) else {
                return Ok(crate::types::DeleteChatMessageResult::NotFound);
            };
            if target.role != crate::types::MessageRole::User {
                return Ok(crate::types::DeleteChatMessageResult::NotUserMessage);
            }

            let deleted_message_ids = message_ids[start_index..].to_vec();
            for deleted_id in &deleted_message_ids {
                inner.messages.remove(deleted_id);
            }
            if let Some(stored_ids) = inner.message_ids_by_chat.get_mut(chat_id) {
                stored_ids.truncate(start_index);
            }
            let last_assistant_message_at = inner
                .message_ids_by_chat
                .get(chat_id)
                .into_iter()
                .flat_map(|ids| ids.iter())
                .filter_map(|id| inner.messages.get(id))
                .filter(|message| message.role == crate::types::MessageRole::Assistant)
                .map(|message| message.created_at)
                .max();
            if let Some(chat) = inner.chats.get_mut(chat_id) {
                chat.last_assistant_message_at = last_assistant_message_at;
                chat.updated_at = now_utc();
            }

            Ok(crate::types::DeleteChatMessageResult::Deleted {
                deleted_message_ids,
            })
        })
    }

    async fn fork_chat_through_message(
        &self,
        input: crate::types::ForkChatInput,
    ) -> PersistenceResult<crate::types::ForkChatResult> {
        self.with_inner(|inner| {
            let Some(source_chat) = inner.chats.get(&input.source_chat_id).cloned() else {
                return Ok(crate::types::ForkChatResult::MessageNotFound);
            };
            let Some(source_session) = inner.sessions.get(&source_chat.session_id) else {
                return Err(invalid_reference("session", source_chat.session_id));
            };
            if source_session.user_id != input.user_id {
                return Err(conflict("fork user must own source session"));
            }
            if !inner.sessions.contains_key(&input.forked_chat.session_id) {
                return Err(invalid_reference("session", input.forked_chat.session_id));
            }
            if inner.chats.contains_key(&input.forked_chat.id) {
                return Err(conflict(format!(
                    "chat already exists: {}",
                    input.forked_chat.id
                )));
            }

            let message_ids = inner
                .message_ids_by_chat
                .get(&input.source_chat_id)
                .cloned()
                .unwrap_or_default();
            let Some(through_index) = message_ids
                .iter()
                .position(|message_id| message_id == &input.through_message_id)
            else {
                return Ok(crate::types::ForkChatResult::MessageNotFound);
            };
            let through_message = inner
                .messages
                .get(&input.through_message_id)
                .expect("message index points at an existing message");
            if through_message.role != crate::types::MessageRole::Assistant {
                return Ok(crate::types::ForkChatResult::NotAssistantMessage);
            }
            let through_message_created_at = through_message.created_at;

            let timestamp = now_utc();
            let record = ChatRecord {
                id: input.forked_chat.id,
                session_id: input.forked_chat.session_id,
                title: input.forked_chat.title,
                model_id: input.forked_chat.model_id,
                active_run_id: None,
                last_assistant_message_at: Some(through_message_created_at),
                created_at: timestamp,
                updated_at: timestamp,
            };
            inner
                .chat_ids_by_session
                .entry(record.session_id.clone())
                .or_default()
                .push(record.id.clone());
            inner
                .message_ids_by_chat
                .entry(record.id.clone())
                .or_default();
            inner.chats.insert(record.id.clone(), record.clone());

            let messages_to_copy = message_ids.into_iter().take(through_index + 1);
            for (copy_index, message_id) in messages_to_copy.enumerate() {
                let source = inner
                    .messages
                    .get(&message_id)
                    .expect("message index points at an existing message")
                    .clone();
                let forked_message_id = format!("{}:forked:{}", record.id, copy_index + 1);
                let forked_message = ChatMessageRecord {
                    id: forked_message_id.clone(),
                    chat_id: record.id.clone(),
                    role: source.role,
                    parts: rewrite_message_json_id(source.parts, &forked_message_id),
                    sequence: inner.next_sequence(),
                    created_at: source.created_at,
                };
                inner
                    .message_ids_by_chat
                    .entry(record.id.clone())
                    .or_default()
                    .push(forked_message.id.clone());
                inner
                    .messages
                    .insert(forked_message.id.clone(), forked_message);
            }

            let read_timestamp = now_utc();
            inner.read_markers.insert(
                (input.user_id.clone(), record.id.clone()),
                crate::types::ChatReadRecord {
                    user_id: input.user_id,
                    chat_id: record.id.clone(),
                    last_read_at: read_timestamp,
                    created_at: read_timestamp,
                    updated_at: read_timestamp,
                },
            );

            Ok(crate::types::ForkChatResult::Created { chat: record })
        })
    }
}

fn rewrite_message_json_id(mut parts: serde_json::Value, new_id: &str) -> serde_json::Value {
    if let serde_json::Value::Object(object) = &mut parts {
        object.insert(
            "id".to_string(),
            serde_json::Value::String(new_id.to_string()),
        );
    }
    parts
}

#[async_trait]
impl ActiveRunRepository for MemoryPersistenceStore {
    async fn claim_chat_active_run(&self, chat_id: &str, run_id: &str) -> PersistenceResult<bool> {
        self.with_inner(|inner| {
            let Some(chat) = inner.chats.get_mut(chat_id) else {
                return Ok(false);
            };
            if chat
                .active_run_id
                .as_deref()
                .is_some_and(|active_run_id| active_run_id != run_id)
            {
                return Ok(false);
            }
            chat.active_run_id = Some(run_id.to_string());
            chat.updated_at = now_utc();
            Ok(true)
        })
    }

    async fn compare_and_set_chat_active_run(
        &self,
        chat_id: &str,
        expected_run_id: Option<String>,
        next_run_id: Option<String>,
    ) -> PersistenceResult<bool> {
        self.with_inner(|inner| {
            let Some(chat) = inner.chats.get_mut(chat_id) else {
                return Ok(false);
            };
            if chat.active_run_id != expected_run_id {
                return Ok(false);
            }
            chat.active_run_id = next_run_id;
            chat.updated_at = now_utc();
            Ok(true)
        })
    }
}

#[async_trait]
impl RunRepository for MemoryPersistenceStore {
    async fn create_run(&self, run: CreateRunInput) -> PersistenceResult<RunRecord> {
        self.with_inner(|inner| {
            if inner.runs.contains_key(&run.id) {
                return Err(conflict(format!("run already exists: {}", run.id)));
            }
            let Some(chat) = inner.chats.get(&run.chat_id) else {
                return Err(invalid_reference("chat", run.chat_id));
            };
            if chat.session_id != run.session_id {
                return Err(conflict("run session must match chat session"));
            }
            let Some(session) = inner.sessions.get(&run.session_id) else {
                return Err(invalid_reference("session", run.session_id));
            };
            if session.user_id != run.user_id {
                return Err(conflict("run user must match session user"));
            }

            let timestamp = now_utc();
            let record = RunRecord {
                id: run.id,
                session_id: run.session_id,
                chat_id: run.chat_id,
                user_id: run.user_id,
                model_id: run.model_id,
                status: run.status,
                idempotency_key: run.idempotency_key,
                started_at: run.started_at.unwrap_or(timestamp),
                finished_at: None,
                error: None,
                created_at: timestamp,
                updated_at: timestamp,
            };
            inner.runs.insert(record.id.clone(), record.clone());
            Ok(record)
        })
    }

    async fn get_run(&self, run_id: &str) -> PersistenceResult<Option<RunRecord>> {
        self.with_inner(|inner| Ok(inner.runs.get(run_id).cloned()))
    }

    async fn update_run_status(
        &self,
        run_id: &str,
        update: RunStatusUpdate,
    ) -> PersistenceResult<Option<RunRecord>> {
        self.with_inner(|inner| {
            let Some(run) = inner.runs.get_mut(run_id) else {
                return Ok(None);
            };
            run.status = update.status;
            run.error = update.error;
            run.finished_at = update
                .finished_at
                .or_else(|| update.status.is_terminal().then(now_utc));
            run.updated_at = now_utc();
            Ok(Some(run.clone()))
        })
    }

    async fn create_run_step_if_absent(
        &self,
        step: CreateRunStepInput,
    ) -> PersistenceResult<InsertIfAbsent<RunStepRecord>> {
        self.with_inner(|inner| {
            if !inner.runs.contains_key(&step.run_id) {
                return Err(invalid_reference("run", step.run_id));
            }
            let run_number_key = (step.run_id.clone(), step.step_number);
            if let Some(existing_id) = inner.step_ids_by_run_number.get(&run_number_key) {
                let record = inner
                    .steps
                    .get(existing_id)
                    .expect("run step index points at an existing step")
                    .clone();
                return Ok(InsertIfAbsent {
                    record,
                    inserted: false,
                });
            }
            if inner.steps.contains_key(&step.id) {
                return Err(conflict(format!("run step already exists: {}", step.id)));
            }

            let record = RunStepRecord {
                id: step.id,
                run_id: step.run_id,
                step_number: step.step_number,
                started_at: step.started_at,
                finished_at: step.finished_at,
                duration_ms: step.duration_ms,
                finish_reason: step.finish_reason,
                raw_finish_reason: step.raw_finish_reason,
                created_at: now_utc(),
            };
            inner
                .step_ids_by_run
                .entry(record.run_id.clone())
                .or_default()
                .push(record.id.clone());
            inner.step_ids_by_run_number.insert(
                (record.run_id.clone(), record.step_number),
                record.id.clone(),
            );
            inner.steps.insert(record.id.clone(), record.clone());
            Ok(InsertIfAbsent {
                record,
                inserted: true,
            })
        })
    }

    async fn list_run_steps(&self, run_id: &str) -> PersistenceResult<Vec<RunStepRecord>> {
        self.with_inner(|inner| {
            let mut records: Vec<_> = inner
                .step_ids_by_run
                .get(run_id)
                .into_iter()
                .flat_map(|ids| ids.iter())
                .filter_map(|id| inner.steps.get(id).cloned())
                .collect();
            records.sort_by_key(|step| step.step_number);
            Ok(records)
        })
    }
}

#[async_trait]
impl UsageRepository for MemoryPersistenceStore {
    async fn create_usage_record(
        &self,
        usage: CreateUsageRecordInput,
    ) -> PersistenceResult<UsageRecord> {
        self.with_inner(|inner| {
            if inner.usage.contains_key(&usage.id) {
                return Err(conflict(format!(
                    "usage record already exists: {}",
                    usage.id
                )));
            }
            if let Some(run_id) = &usage.run_id {
                if !inner.runs.contains_key(run_id) {
                    return Err(invalid_reference("run", run_id.clone()));
                }
            }
            let record = UsageRecord {
                id: usage.id,
                user_id: usage.user_id,
                run_id: usage.run_id,
                source: usage.source,
                agent_type: usage.agent_type,
                provider: usage.provider,
                model_id: usage.model_id,
                input_tokens: usage.input_tokens,
                cached_input_tokens: usage.cached_input_tokens,
                output_tokens: usage.output_tokens,
                tool_call_count: usage.tool_call_count,
                created_at: now_utc(),
            };
            if let Some(run_id) = &record.run_id {
                inner
                    .usage_ids_by_run
                    .entry(run_id.clone())
                    .or_default()
                    .push(record.id.clone());
            }
            inner.usage.insert(record.id.clone(), record.clone());
            Ok(record)
        })
    }

    async fn list_usage_by_run(&self, run_id: &str) -> PersistenceResult<Vec<UsageRecord>> {
        self.with_inner(|inner| {
            let records = inner
                .usage_ids_by_run
                .get(run_id)
                .into_iter()
                .flat_map(|ids| ids.iter())
                .filter_map(|id| inner.usage.get(id).cloned())
                .collect();
            Ok(records)
        })
    }
}

#[async_trait]
impl SlackThreadMappingRepository for MemoryPersistenceStore {
    async fn create_slack_thread_mapping_if_absent(
        &self,
        mapping: CreateSlackThreadMappingInput,
    ) -> PersistenceResult<InsertIfAbsent<SlackThreadMappingRecord>> {
        self.with_inner(|inner| {
            if let Some(existing) = inner.slack_mappings.get(&mapping.key) {
                return Ok(InsertIfAbsent {
                    record: existing.clone(),
                    inserted: false,
                });
            }
            let Some(chat) = inner.chats.get(&mapping.chat_id) else {
                return Err(invalid_reference("chat", mapping.chat_id));
            };
            if chat.session_id != mapping.session_id {
                return Err(conflict("Slack mapping session must match chat session"));
            }
            let Some(session) = inner.sessions.get(&mapping.session_id) else {
                return Err(invalid_reference("session", mapping.session_id));
            };
            if session.user_id != mapping.user_id {
                return Err(conflict("Slack mapping user must match session user"));
            }

            let timestamp = now_utc();
            let record = SlackThreadMappingRecord {
                key: mapping.key,
                session_id: mapping.session_id,
                chat_id: mapping.chat_id,
                user_id: mapping.user_id,
                root_message_ts: mapping.root_message_ts,
                last_event_ts: mapping.last_event_ts,
                created_at: timestamp,
                updated_at: timestamp,
            };
            inner
                .slack_mapping_keys_by_chat
                .insert(record.chat_id.clone(), record.key.clone());
            inner
                .slack_mappings
                .insert(record.key.clone(), record.clone());

            Ok(InsertIfAbsent {
                record,
                inserted: true,
            })
        })
    }

    async fn get_slack_thread_mapping(
        &self,
        key: &SlackThreadKey,
    ) -> PersistenceResult<Option<SlackThreadMappingRecord>> {
        self.with_inner(|inner| Ok(inner.slack_mappings.get(key).cloned()))
    }

    async fn get_slack_thread_mapping_by_chat(
        &self,
        chat_id: &str,
    ) -> PersistenceResult<Option<SlackThreadMappingRecord>> {
        self.with_inner(|inner| {
            let record = inner
                .slack_mapping_keys_by_chat
                .get(chat_id)
                .and_then(|key| inner.slack_mappings.get(key))
                .cloned();
            Ok(record)
        })
    }
}

#[async_trait]
impl ShareRepository for MemoryPersistenceStore {
    async fn get_share_by_chat_id(&self, chat_id: &str) -> PersistenceResult<Option<ShareRecord>> {
        self.with_inner(|inner| {
            let record = inner
                .share_ids_by_chat
                .get(chat_id)
                .and_then(|share_id| inner.shares.get(share_id))
                .cloned();
            Ok(record)
        })
    }

    async fn create_share_if_absent(
        &self,
        share: CreateShareInput,
    ) -> PersistenceResult<InsertIfAbsent<ShareRecord>> {
        self.with_inner(|inner| {
            if !inner.chats.contains_key(&share.chat_id) {
                return Err(invalid_reference("chat", share.chat_id));
            }
            if let Some(existing_id) = inner.share_ids_by_chat.get(&share.chat_id) {
                let record = inner
                    .shares
                    .get(existing_id)
                    .expect("share index points at an existing share")
                    .clone();
                return Ok(InsertIfAbsent {
                    record,
                    inserted: false,
                });
            }
            if inner.shares.contains_key(&share.id) {
                return Err(conflict(format!("share already exists: {}", share.id)));
            }
            let record = ShareRecord {
                id: share.id,
                chat_id: share.chat_id,
                created_at: now_utc(),
            };
            inner
                .share_ids_by_chat
                .insert(record.chat_id.clone(), record.id.clone());
            inner.shares.insert(record.id.clone(), record.clone());
            Ok(InsertIfAbsent {
                record,
                inserted: true,
            })
        })
    }

    async fn delete_share_by_chat_id(&self, chat_id: &str) -> PersistenceResult<bool> {
        self.with_inner(|inner| {
            let Some(share_id) = inner.share_ids_by_chat.remove(chat_id) else {
                return Ok(false);
            };
            Ok(inner.shares.remove(&share_id).is_some())
        })
    }
}

#[async_trait]
impl ChatReadRepository for MemoryPersistenceStore {
    async fn mark_chat_read(
        &self,
        user_id: &str,
        chat_id: &str,
    ) -> PersistenceResult<crate::types::ChatReadRecord> {
        self.with_inner(|inner| {
            if !inner.chats.contains_key(chat_id) {
                return Err(invalid_reference("chat", chat_id));
            }
            let key = (user_id.to_string(), chat_id.to_string());
            let timestamp = now_utc();
            let record = inner
                .read_markers
                .entry(key)
                .and_modify(|record| {
                    record.last_read_at = timestamp;
                    record.updated_at = timestamp;
                })
                .or_insert_with(|| crate::types::ChatReadRecord {
                    user_id: user_id.to_string(),
                    chat_id: chat_id.to_string(),
                    last_read_at: timestamp,
                    created_at: timestamp,
                    updated_at: timestamp,
                })
                .clone();
            Ok(record)
        })
    }

    async fn get_chat_read(
        &self,
        user_id: &str,
        chat_id: &str,
    ) -> PersistenceResult<Option<crate::types::ChatReadRecord>> {
        self.with_inner(|inner| {
            Ok(inner
                .read_markers
                .get(&(user_id.to_string(), chat_id.to_string()))
                .cloned())
        })
    }
}

#[async_trait]
impl SandboxStateRepository for MemoryPersistenceStore {
    async fn update_sandbox_state(
        &self,
        session_id: &str,
        update: SandboxStateUpdate,
    ) -> PersistenceResult<Option<SessionRecord>> {
        self.with_inner(|inner| {
            let Some(session) = inner.sessions.get_mut(session_id) else {
                return Ok(None);
            };
            session.sandbox_state = update.sandbox_state;
            session.lifecycle_state = update.lifecycle_state;
            session.lifecycle_error = update.lifecycle_error;
            session.lifecycle_version += 1;
            session.updated_at = now_utc();
            Ok(Some(session.clone()))
        })
    }

    async fn claim_session_lifecycle_run(
        &self,
        session_id: &str,
        run_id: &str,
    ) -> PersistenceResult<bool> {
        self.with_inner(|inner| {
            let Some(session) = inner.sessions.get_mut(session_id) else {
                return Ok(false);
            };
            if session.lifecycle_run_id.is_some() {
                return Ok(false);
            }
            session.lifecycle_run_id = Some(run_id.to_string());
            session.updated_at = now_utc();
            Ok(true)
        })
    }

    async fn clear_session_lifecycle_run_if_owned(
        &self,
        session_id: &str,
        run_id: &str,
    ) -> PersistenceResult<bool> {
        self.with_inner(|inner| {
            let Some(session) = inner.sessions.get_mut(session_id) else {
                return Ok(false);
            };
            if session.lifecycle_run_id.as_deref() != Some(run_id) {
                return Ok(false);
            }
            session.lifecycle_run_id = None;
            session.updated_at = now_utc();
            Ok(true)
        })
    }

    async fn claim_sandbox_provisioning_run(
        &self,
        session_id: &str,
        run_id: &str,
    ) -> PersistenceResult<bool> {
        self.with_inner(|inner| {
            let Some(session) = inner.sessions.get_mut(session_id) else {
                return Ok(false);
            };
            if session.sandbox_provisioning_run_id.is_some() {
                return Ok(false);
            }
            session.sandbox_provisioning_run_id = Some(run_id.to_string());
            session.updated_at = now_utc();
            Ok(true)
        })
    }

    async fn clear_sandbox_provisioning_run_if_owned(
        &self,
        session_id: &str,
        run_id: &str,
    ) -> PersistenceResult<bool> {
        self.with_inner(|inner| {
            let Some(session) = inner.sessions.get_mut(session_id) else {
                return Ok(false);
            };
            if session.sandbox_provisioning_run_id.as_deref() != Some(run_id) {
                return Ok(false);
            }
            session.sandbox_provisioning_run_id = None;
            session.updated_at = now_utc();
            Ok(true)
        })
    }
}

#[async_trait]
impl IdempotencyRepository for MemoryPersistenceStore {
    async fn create_idempotency_record_if_absent(
        &self,
        record: CreateIdempotencyRecordInput,
    ) -> PersistenceResult<InsertIfAbsent<IdempotencyRecord>> {
        self.with_inner(|inner| {
            let key = (record.scope.clone(), record.key.clone());
            if let Some(existing) = inner.idempotency.get(&key) {
                return Ok(InsertIfAbsent {
                    record: existing.clone(),
                    inserted: false,
                });
            }
            let record = IdempotencyRecord {
                scope: record.scope,
                key: record.key,
                request_hash: record.request_hash,
                response: record.response,
                created_at: now_utc(),
                expires_at: record.expires_at,
            };
            inner.idempotency.insert(key, record.clone());
            Ok(InsertIfAbsent {
                record,
                inserted: true,
            })
        })
    }

    async fn get_idempotency_record(
        &self,
        scope: &str,
        key: &str,
    ) -> PersistenceResult<Option<IdempotencyRecord>> {
        self.with_inner(|inner| {
            Ok(inner
                .idempotency
                .get(&(scope.to_string(), key.to_string()))
                .cloned())
        })
    }
}

#[cfg(test)]
mod tests {
    use futures_executor::block_on;

    use super::MemoryPersistenceStore;
    use crate::contract_tests;

    #[test]
    fn slack_thread_mapping_contract() {
        block_on(contract_tests::slack_thread_mapping_contract(
            &MemoryPersistenceStore::new(),
        ));
    }

    #[test]
    fn active_run_contract() {
        block_on(contract_tests::active_run_contract(
            &MemoryPersistenceStore::new(),
        ));
    }

    #[test]
    fn message_contract() {
        block_on(contract_tests::message_contract(
            &MemoryPersistenceStore::new(),
        ));
    }

    #[test]
    fn session_context_guard_contract() {
        block_on(contract_tests::session_context_guard_contract(
            &MemoryPersistenceStore::new(),
        ));
    }

    #[test]
    fn session_chats_route_lists_creates_updates_and_deletes_chats() {
        block_on(
            contract_tests::session_chats_route_lists_creates_updates_and_deletes_chats(
                &MemoryPersistenceStore::new(),
            ),
        );
    }

    #[test]
    fn session_chat_messages_route_scoped_upsert_and_delete_contract() {
        block_on(
            contract_tests::session_chat_messages_route_scoped_upsert_and_delete_contract(
                &MemoryPersistenceStore::new(),
            ),
        );
    }

    #[test]
    fn session_chat_fork_route_copies_messages_through_selected_assistant() {
        block_on(
            contract_tests::session_chat_fork_route_copies_messages_through_selected_assistant(
                &MemoryPersistenceStore::new(),
            ),
        );
    }

    #[test]
    fn session_chat_read_route_marks_authenticated_owned_chat_read() {
        block_on(
            contract_tests::session_chat_read_route_marks_authenticated_owned_chat_read(
                &MemoryPersistenceStore::new(),
            ),
        );
    }

    #[test]
    fn session_chat_share_route_creates_reuses_and_revokes_share() {
        block_on(
            contract_tests::session_chat_share_route_creates_reuses_and_revokes_share(
                &MemoryPersistenceStore::new(),
            ),
        );
    }

    #[test]
    fn db_sessions_normalizes_legacy_sandbox_state_and_deduplicates_titles() {
        block_on(
            contract_tests::db_sessions_normalizes_legacy_sandbox_state_and_deduplicates_titles(
                &MemoryPersistenceStore::new(),
            ),
        );
    }

    #[test]
    fn idempotency_contract() {
        block_on(contract_tests::idempotency_contract(
            &MemoryPersistenceStore::new(),
        ));
    }

    #[test]
    fn sandbox_lifecycle_contract() {
        block_on(contract_tests::sandbox_lifecycle_contract(
            &MemoryPersistenceStore::new(),
        ));
    }

    #[test]
    fn run_usage_contract() {
        block_on(contract_tests::run_usage_contract(
            &MemoryPersistenceStore::new(),
        ));
    }
}
