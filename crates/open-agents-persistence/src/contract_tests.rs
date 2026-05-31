use time::{Duration, OffsetDateTime};

use crate::types::{
    ActiveRunRepository, AgentType, ChatRepository, CreateChatInput, CreateChatMessageInput,
    CreateIdempotencyRecordInput, CreateRunInput, CreateRunStepInput, CreateSessionInput,
    CreateSlackThreadMappingInput, CreateUsageRecordInput, IdempotencyRepository, LifecycleState,
    MessageRepository, MessageRole, RunRepository, RunStatus, RunStatusUpdate,
    SandboxStateRepository, SandboxStateUpdate, SessionRepository, SlackThreadKey,
    SlackThreadMappingRepository, UsageRepository, UsageSource,
};

async fn seed_session<S>(store: &S)
where
    S: SessionRepository,
{
    let session = CreateSessionInput::new("session-1", "user-1", "Fix the thing");
    let chat = CreateChatInput::new("chat-1", "session-1", "Fix the thing");
    store
        .create_session_with_initial_chat(session, chat)
        .await
        .expect("session and chat are created");
}

pub(crate) async fn slack_thread_mapping_contract<S>(store: &S)
where
    S: SessionRepository + SlackThreadMappingRepository,
{
    seed_session(store).await;
    let key = SlackThreadKey::new("T1", "C1", "1712345678.000100");
    let created = store
        .create_slack_thread_mapping_if_absent(CreateSlackThreadMappingInput {
            key: key.clone(),
            session_id: "session-1".to_string(),
            chat_id: "chat-1".to_string(),
            user_id: "user-1".to_string(),
            root_message_ts: Some("1712345678.000100".to_string()),
            last_event_ts: Some("1712345678.000200".to_string()),
        })
        .await
        .expect("Slack mapping is inserted");
    assert!(created.inserted);
    assert_eq!(created.record.session_id, "session-1");
    assert_eq!(created.record.chat_id, "chat-1");

    let retry = store
        .create_slack_thread_mapping_if_absent(CreateSlackThreadMappingInput {
            key: key.clone(),
            session_id: "session-1".to_string(),
            chat_id: "chat-1".to_string(),
            user_id: "user-1".to_string(),
            root_message_ts: None,
            last_event_ts: None,
        })
        .await
        .expect("Slack mapping retry is idempotent");
    assert!(!retry.inserted);
    assert_eq!(
        retry.record.root_message_ts.as_deref(),
        Some("1712345678.000100")
    );

    let by_key = store
        .get_slack_thread_mapping(&key)
        .await
        .expect("lookup succeeds")
        .expect("mapping exists");
    assert_eq!(by_key.chat_id, "chat-1");

    let by_chat = store
        .get_slack_thread_mapping_by_chat("chat-1")
        .await
        .expect("chat lookup succeeds")
        .expect("mapping exists for chat");
    assert_eq!(by_chat.key, key);
    assert_eq!(by_chat.identity().session_id, "session-1");
    assert_eq!(by_chat.identity().chat_id, "chat-1");
}

pub(crate) async fn active_run_contract<S>(store: &S)
where
    S: SessionRepository + ChatRepository + ActiveRunRepository,
{
    seed_session(store).await;

    assert!(
        store
            .claim_chat_active_run("chat-1", "run-a")
            .await
            .expect("claim succeeds")
    );
    assert!(
        store
            .claim_chat_active_run("chat-1", "run-a")
            .await
            .expect("same run claim is idempotent")
    );
    assert!(
        !store
            .claim_chat_active_run("chat-1", "run-b")
            .await
            .expect("conflicting claim is rejected")
    );
    assert!(
        !store
            .compare_and_set_chat_active_run(
                "chat-1",
                Some("wrong-run".to_string()),
                Some("run-b".to_string()),
            )
            .await
            .expect("stale CAS returns false")
    );
    assert!(
        store
            .compare_and_set_chat_active_run("chat-1", Some("run-a".to_string()), None)
            .await
            .expect("owned run can be cleared")
    );
    assert!(
        store
            .claim_chat_active_run("chat-1", "run-b")
            .await
            .expect("new run can claim cleared slot")
    );

    let chat = store
        .get_chat("chat-1")
        .await
        .expect("chat lookup succeeds")
        .expect("chat exists");
    assert_eq!(chat.active_run_id.as_deref(), Some("run-b"));
}

pub(crate) async fn message_contract<S>(store: &S)
where
    S: SessionRepository + MessageRepository,
{
    seed_session(store).await;
    let base = OffsetDateTime::now_utc();

    let second = store
        .create_message_if_absent(CreateChatMessageInput {
            id: "message-2".to_string(),
            chat_id: "chat-1".to_string(),
            role: MessageRole::Assistant,
            parts: serde_json::json!({"id":"message-2","parts":[{"type":"text","text":"two"}]}),
            created_at: Some(base + Duration::seconds(2)),
        })
        .await
        .expect("second message inserts");
    assert!(second.inserted);

    let first = store
        .create_message_if_absent(CreateChatMessageInput {
            id: "message-1".to_string(),
            chat_id: "chat-1".to_string(),
            role: MessageRole::User,
            parts: serde_json::json!({"id":"message-1","parts":[{"type":"text","text":"one"}]}),
            created_at: Some(base + Duration::seconds(1)),
        })
        .await
        .expect("first message inserts");
    assert!(first.inserted);

    let duplicate = store
        .create_message_if_absent(CreateChatMessageInput {
            id: "message-1".to_string(),
            chat_id: "chat-1".to_string(),
            role: MessageRole::User,
            parts: serde_json::json!({"changed":true}),
            created_at: Some(base + Duration::seconds(3)),
        })
        .await
        .expect("duplicate message is idempotent");
    assert!(!duplicate.inserted);
    assert_eq!(duplicate.record.parts, first.record.parts);

    let messages = store
        .list_chat_messages("chat-1")
        .await
        .expect("message list succeeds");
    let ids: Vec<_> = messages.iter().map(|message| message.id.as_str()).collect();
    assert_eq!(ids, vec!["message-1", "message-2"]);
}

pub(crate) async fn idempotency_contract<S>(store: &S)
where
    S: IdempotencyRepository,
{
    let created = store
        .create_idempotency_record_if_absent(CreateIdempotencyRecordInput {
            scope: "slack-event".to_string(),
            key: "event-1".to_string(),
            request_hash: Some("hash-a".to_string()),
            response: Some(serde_json::json!({"status":"accepted"})),
            expires_at: None,
        })
        .await
        .expect("idempotency record inserts");
    assert!(created.inserted);

    let retry = store
        .create_idempotency_record_if_absent(CreateIdempotencyRecordInput {
            scope: "slack-event".to_string(),
            key: "event-1".to_string(),
            request_hash: Some("hash-b".to_string()),
            response: Some(serde_json::json!({"status":"changed"})),
            expires_at: None,
        })
        .await
        .expect("retry returns existing record");
    assert!(!retry.inserted);
    assert_eq!(retry.record.request_hash.as_deref(), Some("hash-a"));
    assert_eq!(
        retry.record.response,
        Some(serde_json::json!({"status":"accepted"}))
    );

    let fetched = store
        .get_idempotency_record("slack-event", "event-1")
        .await
        .expect("lookup succeeds")
        .expect("record exists");
    assert_eq!(fetched.key, "event-1");
}

pub(crate) async fn sandbox_lifecycle_contract<S>(store: &S)
where
    S: SessionRepository + SandboxStateRepository,
{
    seed_session(store).await;

    assert!(
        store
            .claim_session_lifecycle_run("session-1", "lifecycle-run-1")
            .await
            .expect("lifecycle lease is claimed")
    );
    assert!(
        !store
            .claim_session_lifecycle_run("session-1", "lifecycle-run-2")
            .await
            .expect("second lifecycle lease is rejected")
    );
    assert!(
        store
            .clear_session_lifecycle_run_if_owned("session-1", "lifecycle-run-1")
            .await
            .expect("owned lifecycle lease is cleared")
    );
    assert!(
        store
            .claim_sandbox_provisioning_run("session-1", "sandbox-run-1")
            .await
            .expect("sandbox provisioning lease is claimed")
    );

    let updated = store
        .update_sandbox_state(
            "session-1",
            SandboxStateUpdate {
                lifecycle_state: LifecycleState::Running,
                sandbox_state: Some(crate::types::SandboxState {
                    provider: "vercel".to_string(),
                    sandbox_id: None,
                    sandbox_name: Some("open-agents-session-1".to_string()),
                    working_directory: Some("/workspace".to_string()),
                    current_branch: Some("codex/open-agents".to_string()),
                    environment_details: Some("ready".to_string()),
                    raw: serde_json::json!({"type":"vercel"}),
                }),
                lifecycle_error: None,
            },
        )
        .await
        .expect("sandbox state update succeeds")
        .expect("session exists");
    assert_eq!(updated.lifecycle_state, LifecycleState::Running);
    assert_eq!(updated.lifecycle_version, 1);
    assert_eq!(
        updated
            .sandbox_state
            .as_ref()
            .and_then(|state| state.sandbox_name.as_deref()),
        Some("open-agents-session-1")
    );

    let failed = store
        .update_sandbox_state(
            "session-1",
            SandboxStateUpdate {
                lifecycle_state: LifecycleState::Failed,
                sandbox_state: updated.sandbox_state,
                lifecycle_error: Some("sandbox stopped".to_string()),
            },
        )
        .await
        .expect("failure state update succeeds")
        .expect("session exists");
    assert_eq!(failed.lifecycle_state, LifecycleState::Failed);
    assert_eq!(failed.lifecycle_version, 2);
    assert_eq!(failed.lifecycle_error.as_deref(), Some("sandbox stopped"));
}

pub(crate) async fn run_usage_contract<S>(store: &S)
where
    S: SessionRepository + RunRepository + UsageRepository + ActiveRunRepository,
{
    seed_session(store).await;
    let run = store
        .create_run(CreateRunInput {
            id: "run-1".to_string(),
            session_id: "session-1".to_string(),
            chat_id: "chat-1".to_string(),
            user_id: "user-1".to_string(),
            model_id: Some("gpt-5.5".to_string()),
            status: RunStatus::Running,
            idempotency_key: Some("slack-event-1".to_string()),
            started_at: None,
        })
        .await
        .expect("run inserts");
    assert_eq!(run.status, RunStatus::Running);
    assert_eq!(run.identity().run_id.as_deref(), Some("run-1"));

    assert!(
        store
            .claim_chat_active_run("chat-1", "run-1")
            .await
            .expect("active run is claimed")
    );

    let step = store
        .create_run_step_if_absent(CreateRunStepInput {
            id: "run-1-step-1".to_string(),
            run_id: "run-1".to_string(),
            step_number: 1,
            started_at: OffsetDateTime::now_utc(),
            finished_at: Some(OffsetDateTime::now_utc()),
            duration_ms: Some(42),
            finish_reason: Some("tool-calls".to_string()),
            raw_finish_reason: Some("tool-calls".to_string()),
        })
        .await
        .expect("run step inserts");
    assert!(step.inserted);

    let duplicate_step = store
        .create_run_step_if_absent(CreateRunStepInput {
            id: "run-1-step-1-retry".to_string(),
            run_id: "run-1".to_string(),
            step_number: 1,
            started_at: OffsetDateTime::now_utc(),
            finished_at: None,
            duration_ms: None,
            finish_reason: None,
            raw_finish_reason: None,
        })
        .await
        .expect("run step retry is idempotent");
    assert!(!duplicate_step.inserted);
    assert_eq!(duplicate_step.record.id, "run-1-step-1");

    let usage = store
        .create_usage_record(CreateUsageRecordInput {
            id: "usage-1".to_string(),
            user_id: "user-1".to_string(),
            run_id: Some("run-1".to_string()),
            source: UsageSource::Slack,
            agent_type: AgentType::Main,
            provider: Some("openai".to_string()),
            model_id: Some("gpt-5.5".to_string()),
            input_tokens: 100,
            cached_input_tokens: 10,
            output_tokens: 25,
            tool_call_count: 2,
        })
        .await
        .expect("usage inserts");
    assert_eq!(usage.input_tokens, 100);

    let finished = store
        .update_run_status(
            "run-1",
            RunStatusUpdate {
                status: RunStatus::Finished,
                finished_at: None,
                error: None,
            },
        )
        .await
        .expect("run update succeeds")
        .expect("run exists");
    assert_eq!(finished.status, RunStatus::Finished);
    assert!(finished.finished_at.is_some());

    assert!(
        store
            .compare_and_set_chat_active_run("chat-1", Some("run-1".to_string()), None)
            .await
            .expect("active run clears")
    );

    let usage_rows = store
        .list_usage_by_run("run-1")
        .await
        .expect("usage lookup succeeds");
    assert_eq!(usage_rows.len(), 1);
    assert_eq!(usage_rows[0].id, "usage-1");

    let steps = store
        .list_run_steps("run-1")
        .await
        .expect("step lookup succeeds");
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].step_number, 1);
}
