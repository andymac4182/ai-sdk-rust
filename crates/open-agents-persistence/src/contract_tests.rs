use time::{Duration, OffsetDateTime};

use crate::types::{
    ActiveRunRepository, AgentType, ChatPatch, ChatReadRepository, ChatRepository, CreateChatInput,
    CreateChatMessageInput, CreateIdempotencyRecordInput, CreateRunInput, CreateRunStepInput,
    CreateSessionInput, CreateShareInput, CreateSlackThreadMappingInput, CreateUsageRecordInput,
    DeleteChatMessageResult, ForkChatInput, ForkChatResult, IdempotencyRepository, LifecycleState,
    MessageRepository, MessageRole, RunRepository, RunStatus, RunStatusUpdate,
    SandboxStateRepository, SandboxStateUpdate, SessionRepository, ShareRepository, SlackThreadKey,
    SlackThreadMappingRepository, UpsertChatMessageStatus, UsageRepository, UsageSource,
    normalize_legacy_sandbox_state,
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

pub(crate) async fn session_context_guard_contract<S>(store: &S)
where
    S: SessionRepository + ChatRepository,
{
    seed_session(store).await;

    let missing_session = store
        .get_session("missing-session")
        .await
        .expect("session lookup succeeds");
    assert!(missing_session.is_none());

    let session = store
        .get_session("session-1")
        .await
        .expect("session lookup succeeds")
        .expect("session exists");
    assert_eq!(session.user_id, "user-1");
    assert_ne!(session.user_id, "other-user");

    let missing_chat = store
        .get_chat("missing-chat")
        .await
        .expect("chat lookup succeeds");
    assert!(missing_chat.is_none());

    let chat = store
        .get_chat("chat-1")
        .await
        .expect("chat lookup succeeds")
        .expect("chat exists");
    assert_eq!(chat.session_id, "session-1");
    assert_ne!(chat.session_id, "session-2");
}

pub(crate) async fn session_chats_route_lists_creates_updates_and_deletes_chats<S>(store: &S)
where
    S: SessionRepository + ChatRepository,
{
    seed_session(store).await;

    let summaries = store
        .chat_summaries_by_session("session-1")
        .await
        .expect("chat summaries load");
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].id, "chat-1");

    let requested = store
        .create_chat(CreateChatInput {
            id: "chat-requested".to_string(),
            session_id: "session-1".to_string(),
            title: "New chat".to_string(),
            model_id: Some("model-default".to_string()),
        })
        .await
        .expect("requested chat inserts");
    assert_eq!(requested.id, "chat-requested");

    let duplicate = store
        .create_chat(CreateChatInput::new(
            "chat-requested",
            "session-1",
            "Duplicate",
        ))
        .await
        .expect_err("duplicate chat id conflicts");
    assert!(duplicate.to_string().contains("chat already exists"));

    let updated = store
        .update_chat(
            "chat-requested",
            ChatPatch {
                title: Some("New title".to_string()),
                model_id: Some("model-2".to_string()),
            },
        )
        .await
        .expect("chat update succeeds")
        .expect("chat exists");
    assert_eq!(updated.title, "New title");
    assert_eq!(updated.model_id.as_deref(), Some("model-2"));

    let missing_update = store
        .update_chat(
            "missing-chat",
            ChatPatch {
                title: Some("New".to_string()),
                model_id: None,
            },
        )
        .await
        .expect("missing update is not an error");
    assert!(missing_update.is_none());

    let only_chat_delete_allowed = store
        .chat_summaries_by_session("session-1")
        .await
        .expect("chat summaries load")
        .len()
        > 1;
    assert!(only_chat_delete_allowed);
    assert!(
        store
            .delete_chat("chat-requested")
            .await
            .expect("chat delete succeeds")
    );
    assert!(
        store
            .get_chat("chat-requested")
            .await
            .expect("chat lookup succeeds")
            .is_none()
    );
}

pub(crate) async fn session_chat_messages_route_scoped_upsert_and_delete_contract<S>(store: &S)
where
    S: SessionRepository + ChatRepository + MessageRepository,
{
    seed_session(store).await;
    let base = OffsetDateTime::now_utc();

    let invalid_role = CreateChatMessageInput {
        id: "user-payload".to_string(),
        chat_id: "chat-1".to_string(),
        role: MessageRole::User,
        parts: serde_json::json!({"id":"user-payload","role":"user","parts":[]}),
        created_at: Some(base),
    };
    let inserted_user = store
        .upsert_chat_message_scoped(invalid_role)
        .await
        .expect("user message can be stored by the persistence layer");
    assert_eq!(inserted_user.status, UpsertChatMessageStatus::Inserted);

    let assistant = CreateChatMessageInput {
        id: "assistant-1".to_string(),
        chat_id: "chat-1".to_string(),
        role: MessageRole::Assistant,
        parts: serde_json::json!({"id":"assistant-1","role":"assistant","parts":[{"type":"data-commit"}]}),
        created_at: Some(base + Duration::seconds(1)),
    };
    let inserted = store
        .upsert_chat_message_scoped(assistant.clone())
        .await
        .expect("assistant message inserts");
    assert_eq!(inserted.status, UpsertChatMessageStatus::Inserted);

    let updated = store
        .upsert_chat_message_scoped(CreateChatMessageInput {
            parts: serde_json::json!({"id":"assistant-1","role":"assistant","parts":[{"type":"data-pr"}]}),
            ..assistant.clone()
        })
        .await
        .expect("same chat and role can update");
    assert_eq!(updated.status, UpsertChatMessageStatus::Updated);

    let other_session = CreateSessionInput::new("session-2", "user-1", "Other session");
    let other_chat = CreateChatInput::new("chat-2", "session-2", "Other chat");
    store
        .create_session_with_initial_chat(other_session, other_chat)
        .await
        .expect("second session inserts");
    let conflict = store
        .upsert_chat_message_scoped(CreateChatMessageInput {
            chat_id: "chat-2".to_string(),
            ..assistant
        })
        .await
        .expect("conflicting upsert returns status");
    assert_eq!(conflict.status, UpsertChatMessageStatus::Conflict);

    let assistant_delete = store
        .delete_chat_message_and_following("chat-1", "assistant-1")
        .await
        .expect("assistant delete is classified");
    assert_eq!(assistant_delete, DeleteChatMessageResult::NotUserMessage);

    let missing_delete = store
        .delete_chat_message_and_following("chat-1", "missing")
        .await
        .expect("missing delete is classified");
    assert_eq!(missing_delete, DeleteChatMessageResult::NotFound);

    store
        .create_message_if_absent(CreateChatMessageInput {
            id: "assistant-2".to_string(),
            chat_id: "chat-1".to_string(),
            role: MessageRole::Assistant,
            parts: serde_json::json!({"id":"assistant-2","role":"assistant","parts":[]}),
            created_at: Some(base + Duration::seconds(2)),
        })
        .await
        .expect("following assistant inserts");
    let deleted = store
        .delete_chat_message_and_following("chat-1", "user-payload")
        .await
        .expect("user delete succeeds");
    assert_eq!(
        deleted,
        DeleteChatMessageResult::Deleted {
            deleted_message_ids: vec![
                "user-payload".to_string(),
                "assistant-1".to_string(),
                "assistant-2".to_string()
            ],
        }
    );
}

pub(crate) async fn session_chat_fork_route_copies_messages_through_selected_assistant<S>(store: &S)
where
    S: SessionRepository + ChatRepository + MessageRepository + ChatReadRepository,
{
    seed_session(store).await;
    let base = OffsetDateTime::now_utc();
    for (id, role, seconds) in [
        ("message-1", MessageRole::User, 1),
        ("message-2", MessageRole::Assistant, 2),
        ("message-3", MessageRole::Assistant, 3),
    ] {
        store
            .create_message_if_absent(CreateChatMessageInput {
                id: id.to_string(),
                chat_id: "chat-1".to_string(),
                role,
                parts: serde_json::json!({"id":id,"role":format!("{role:?}"),"parts":[]}),
                created_at: Some(base + Duration::seconds(seconds)),
            })
            .await
            .expect("source message inserts");
    }

    let missing = store
        .fork_chat_through_message(ForkChatInput {
            user_id: "user-1".to_string(),
            source_chat_id: "chat-1".to_string(),
            through_message_id: "missing".to_string(),
            forked_chat: CreateChatInput::new("fork-missing", "session-1", "Fork"),
        })
        .await
        .expect("missing fork is classified");
    assert_eq!(missing, ForkChatResult::MessageNotFound);

    let user_message = store
        .fork_chat_through_message(ForkChatInput {
            user_id: "user-1".to_string(),
            source_chat_id: "chat-1".to_string(),
            through_message_id: "message-1".to_string(),
            forked_chat: CreateChatInput::new("fork-user", "session-1", "Fork"),
        })
        .await
        .expect("non-assistant fork is classified");
    assert_eq!(user_message, ForkChatResult::NotAssistantMessage);

    let created = store
        .fork_chat_through_message(ForkChatInput {
            user_id: "user-1".to_string(),
            source_chat_id: "chat-1".to_string(),
            through_message_id: "message-2".to_string(),
            forked_chat: CreateChatInput {
                id: "fork-chat-1".to_string(),
                session_id: "session-1".to_string(),
                title: "Fork of Original chat".to_string(),
                model_id: Some("model-1".to_string()),
            },
        })
        .await
        .expect("fork succeeds");
    let ForkChatResult::Created { chat } = created else {
        panic!("expected created fork");
    };
    assert_eq!(chat.id, "fork-chat-1");
    assert_eq!(
        chat.last_assistant_message_at,
        Some(base + Duration::seconds(2))
    );

    let forked_messages = store
        .list_chat_messages("fork-chat-1")
        .await
        .expect("forked messages list");
    assert_eq!(forked_messages.len(), 2);
    assert_eq!(forked_messages[0].role, MessageRole::User);
    assert_eq!(forked_messages[1].role, MessageRole::Assistant);
    assert!(
        store
            .get_chat_read("user-1", "fork-chat-1")
            .await
            .expect("read marker lookup succeeds")
            .is_some()
    );
}

pub(crate) async fn session_chat_read_route_marks_authenticated_owned_chat_read<S>(store: &S)
where
    S: SessionRepository + ChatReadRepository,
{
    seed_session(store).await;
    let marker = store
        .mark_chat_read("user-1", "chat-1")
        .await
        .expect("read marker upserts");
    assert_eq!(marker.user_id, "user-1");
    assert_eq!(marker.chat_id, "chat-1");

    let retry = store
        .mark_chat_read("user-1", "chat-1")
        .await
        .expect("read marker retry updates");
    assert_eq!(retry.created_at, marker.created_at);
    assert!(retry.updated_at >= marker.updated_at);
}

pub(crate) async fn session_chat_share_route_creates_reuses_and_revokes_share<S>(store: &S)
where
    S: SessionRepository + ShareRepository,
{
    seed_session(store).await;
    assert!(
        store
            .get_share_by_chat_id("chat-1")
            .await
            .expect("share lookup succeeds")
            .is_none()
    );

    let created = store
        .create_share_if_absent(CreateShareInput {
            id: "share-new".to_string(),
            chat_id: "chat-1".to_string(),
        })
        .await
        .expect("share inserts");
    assert!(created.inserted);
    assert_eq!(created.record.id, "share-new");

    let reused = store
        .create_share_if_absent(CreateShareInput {
            id: "share-other".to_string(),
            chat_id: "chat-1".to_string(),
        })
        .await
        .expect("share retry reuses existing row");
    assert!(!reused.inserted);
    assert_eq!(reused.record.id, "share-new");

    assert!(
        store
            .delete_share_by_chat_id("chat-1")
            .await
            .expect("share delete succeeds")
    );
    assert!(
        store
            .get_share_by_chat_id("chat-1")
            .await
            .expect("share lookup succeeds")
            .is_none()
    );
}

pub(crate) async fn db_sessions_normalizes_legacy_sandbox_state_and_deduplicates_titles<S>(
    store: &S,
) where
    S: SessionRepository,
{
    let hybrid = normalize_legacy_sandbox_state(serde_json::json!({
        "type": "hybrid",
        "sandboxId": "sbx-legacy-1",
        "snapshotId": "snap-legacy-1",
        "expiresAt": 123
    }))
    .expect("hybrid state normalizes");
    assert_eq!(
        hybrid,
        serde_json::json!({
            "type": "vercel",
            "sandboxName": "sbx-legacy-1",
            "snapshotId": "snap-legacy-1",
            "expiresAt": 123
        })
    );

    let session_id_state = normalize_legacy_sandbox_state(serde_json::json!({
        "type": "vercel",
        "sandboxId": "session_123",
        "expiresAt": 456
    }))
    .expect("session id state normalizes");
    assert_eq!(
        session_id_state,
        serde_json::json!({
            "type": "vercel",
            "sandboxName": "session_123",
            "expiresAt": 456
        })
    );

    let supported = serde_json::json!({
        "type": "vercel",
        "sandboxName": "session_current-1",
        "expiresAt": 456
    });
    assert_eq!(
        normalize_legacy_sandbox_state(supported.clone()),
        Some(supported)
    );
    assert_eq!(
        normalize_legacy_sandbox_state(serde_json::Value::Null),
        None
    );

    store
        .create_session_with_initial_chat(
            CreateSessionInput::new("title-session-1", "user-title", "Rome"),
            CreateChatInput::new("title-chat-1", "title-session-1", "Rome"),
        )
        .await
        .expect("first titled session inserts");
    store
        .create_session_with_initial_chat(
            CreateSessionInput::new("title-session-2", "user-title", "Rome"),
            CreateChatInput::new("title-chat-2", "title-session-2", "Rome"),
        )
        .await
        .expect("second titled session inserts");
    store
        .create_session_with_initial_chat(
            CreateSessionInput::new("title-session-3", "user-title", "Paris"),
            CreateChatInput::new("title-chat-3", "title-session-3", "Paris"),
        )
        .await
        .expect("third titled session inserts");

    let titles = store
        .used_session_titles("user-title")
        .await
        .expect("used titles load");
    assert_eq!(titles, vec!["Paris".to_string(), "Rome".to_string()]);
    assert!(
        store
            .used_session_titles("missing-user")
            .await
            .expect("missing user titles load")
            .is_empty()
    );
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
