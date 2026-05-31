use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use time::OffsetDateTime;

use crate::types::{
    ActiveRunRepository, ChatMessageRecord, ChatRecord, ChatRepository, CreateChatInput,
    CreateChatMessageInput, CreateIdempotencyRecordInput, CreateRunInput, CreateRunStepInput,
    CreateSessionInput, CreateSlackThreadMappingInput, CreateUsageRecordInput, IdempotencyRecord,
    IdempotencyRepository, InsertIfAbsent, MessageRepository, PersistenceError, PersistenceResult,
    RunRecord, RunRepository, RunStatusUpdate, RunStepRecord, SandboxStateRepository,
    SandboxStateUpdate, SessionRecord, SessionRepository, SlackThreadKey, SlackThreadMappingRecord,
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
