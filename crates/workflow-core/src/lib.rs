//! Core runtime crate for the standalone Vercel Workflow SDK Rust port.
//!
//! This crate maps to upstream `packages/core`. The portable runtime pieces
//! implemented here intentionally use deterministic World traits and in-memory
//! fakes so replay, suspension, wait, step, and stream semantics can be tested
//! before local/postgres/vercel Worlds are ported.

#![forbid(unsafe_code)]

mod events_consumer;
mod flushable_stream;
mod runtime;

pub use events_consumer::*;
pub use flushable_stream::*;
pub use runtime::*;
pub use workflow_errors as errors;
pub use workflow_world as world;

/// Upstream repository used for this crate boundary.
pub const UPSTREAM_REPOSITORY: &str = "github.com/vercel/workflow";

/// Upstream ref fetched into the local source mirror.
pub const UPSTREAM_REF: &str = "main";

/// Remote HEAD verified for the initial crate skeleton.
pub const UPSTREAM_HEAD: &str = "1ee63b870afbf9754eb1022b1bb5f02d0ab042f9";

/// Upstream package owned by this crate.
pub const UPSTREAM_PACKAGE: &str = "@workflow/core";

/// Upstream package version inventoried for this skeleton.
pub const UPSTREAM_VERSION: &str = "5.0.0-beta.10";

/// Rust crate version compiled into the library.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn records_core_source_snapshot() {
        assert_eq!(UPSTREAM_PACKAGE, "@workflow/core");
        assert_eq!(UPSTREAM_VERSION, "5.0.0-beta.10");
        assert_eq!(UPSTREAM_HEAD.len(), 40);
    }

    fn event(event_id: &str, event_type: EventKind) -> Event {
        Event {
            event_id: event_id.to_string(),
            run_id: "wrun_test".to_string(),
            event_type,
            correlation_id: None,
            event_data: Value::Null,
            request_id: None,
            spec_version: SPEC_VERSION_CURRENT,
            created_at_ms: 0,
        }
    }

    fn event_with_data(
        event_id: &str,
        event_type: EventKind,
        correlation_id: &str,
        event_data: Value,
    ) -> Event {
        Event {
            correlation_id: Some(correlation_id.to_string()),
            event_data,
            ..event(event_id, event_type)
        }
    }

    fn running_run(run_id: &str) -> WorkflowRun {
        WorkflowRun {
            run_id: run_id.to_string(),
            workflow_name: "workflow".to_string(),
            status: RunStatus::Running,
            input: Value::Null,
            output: None,
            error: None,
            error_code: None,
            deployment_id: Some("deploy".to_string()),
            spec_version: SPEC_VERSION_CURRENT,
            created_at_ms: 0,
            started_at_ms: Some(0),
            completed_at_ms: None,
        }
    }

    fn world_with_running_run(run_id: &str) -> InMemoryWorld {
        let mut world = InMemoryWorld::new();
        world.insert_run(running_run(run_id));
        world
    }

    fn create_step(world: &mut InMemoryWorld, run_id: &str, step_id: &str, step_name: &str) {
        world
            .events_create(
                run_id,
                CreateEventRequest::new(EventKind::StepCreated)
                    .with_correlation_id(step_id)
                    .with_data(json!({ "stepName": step_name, "input": null })),
                EventCreateOptions::default(),
            )
            .expect("step_created");
    }

    fn step_params(run_id: &str, step_id: &str, step_name: &str) -> StepExecutorParams {
        StepExecutorParams {
            workflow_run_id: run_id.to_string(),
            workflow_name: "workflow".to_string(),
            workflow_started_at_ms: 0,
            step_id: step_id.to_string(),
            step_name: step_name.to_string(),
            request_id: Some("req_test".to_string()),
        }
    }

    #[test]
    fn events_consumer_initializes_and_subscribes_callbacks() {
        let first = event("event-1", EventKind::RunCreated);
        let mut consumer = EventsConsumer::new(vec![first.clone()]);
        assert_eq!(consumer.events(), &[first]);
        assert_eq!(consumer.event_index(), 0);

        let order = Rc::new(RefCell::new(Vec::new()));
        let first_order = Rc::clone(&order);
        consumer.subscribe(move |_| {
            first_order.borrow_mut().push("first");
            EventConsumerResult::NotConsumed
        });
        let second_order = Rc::clone(&order);
        consumer.subscribe(move |event| {
            if let Some(event) = event {
                assert_eq!(event.event_id, "event-1");
                second_order.borrow_mut().push("second");
                EventConsumerResult::Consumed
            } else {
                EventConsumerResult::NotConsumed
            }
        });

        assert_eq!(consumer.callback_count(), 2);
        assert_eq!(consumer.event_index(), 1);
        assert_eq!(&*order.borrow(), &["first", "first", "second", "first"]);
    }

    #[test]
    fn events_consumer_consumes_finished_not_consumed_and_null_events() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut empty = EventsConsumer::new(Vec::new());
        let empty_calls = Rc::clone(&calls);
        empty.subscribe(move |event| {
            empty_calls.borrow_mut().push(event.is_none());
            EventConsumerResult::NotConsumed
        });
        empty.flush_deferred_unconsumed_check();
        assert_eq!(&*calls.borrow(), &[true]);
        assert!(empty.unconsumed_events().is_empty());

        let mut consumer = EventsConsumer::new(vec![
            event("event-1", EventKind::RunCreated),
            event("event-2", EventKind::RunStarted),
        ]);
        consumer.subscribe(|event| {
            assert_eq!(event.expect("event").event_id, "event-1");
            EventConsumerResult::Finished
        });
        assert_eq!(consumer.event_index(), 1);
        assert_eq!(consumer.callback_count(), 0);

        consumer.subscribe(|event| {
            assert_eq!(event.expect("event").event_id, "event-2");
            EventConsumerResult::NotConsumed
        });
        assert_eq!(consumer.event_index(), 1);
        consumer.flush_deferred_unconsumed_check();
        assert_eq!(consumer.unconsumed_events()[0].event_id, "event-2");
    }

    #[test]
    fn events_consumer_complex_sequences_and_callback_errors() {
        let consumed = Rc::new(RefCell::new(Vec::new()));
        let mut consumer = EventsConsumer::new(vec![
            event_with_data("event-1", EventKind::HookReceived, "hook", Value::Null),
            event_with_data(
                "event-2",
                EventKind::HookReceived,
                "hook",
                json!({"payload": null}),
            ),
        ]);

        consumer.subscribe(|_| EventConsumerResult::Errored);
        let consumed_first = Rc::clone(&consumed);
        consumer.subscribe(move |event| {
            if let Some(event) = event {
                consumed_first.borrow_mut().push(event.event_id.clone());
                EventConsumerResult::Consumed
            } else {
                EventConsumerResult::NotConsumed
            }
        });

        assert_eq!(
            &*consumed.borrow(),
            &["event-1".to_string(), "event-2".to_string()]
        );
        assert_eq!(consumer.event_index(), 2);
        assert!(consumer.callback_errors().len() >= 2);
    }

    #[test]
    fn events_consumer_defers_and_cancels_unconsumed_checks() {
        let mut consumer = EventsConsumer::new(vec![event("event-1", EventKind::RunCreated)]);
        consumer.subscribe(|_| EventConsumerResult::NotConsumed);
        assert!(consumer.unconsumed_events().is_empty());
        consumer.flush_deferred_unconsumed_check();
        assert_eq!(consumer.unconsumed_events().len(), 1);

        let mut consumer = EventsConsumer::new(vec![event("event-2", EventKind::RunCreated)]);
        consumer.subscribe(|_| EventConsumerResult::NotConsumed);
        consumer.subscribe(|event| {
            assert_eq!(event.expect("event").event_id, "event-2");
            EventConsumerResult::Finished
        });
        consumer.flush_deferred_unconsumed_check();
        assert!(consumer.unconsumed_events().is_empty());
        assert_eq!(consumer.event_index(), 1);
    }

    #[test]
    fn flushable_stream_state_resolves_on_lock_release_or_close() {
        let mut state = FlushableStreamState::new();
        state.begin_write();
        state.poll_writable_lock();
        state.release_writable_lock();
        assert!(!state.is_done());
        state.finish_write();
        assert!(state.is_done());
        assert_eq!(state.pending_ops(), 0);

        let mut state = FlushableStreamState::new();
        state.close_stream();
        assert!(state.is_done());
        assert!(state.stream_ended());

        let mut state = FlushableStreamState::new();
        state.begin_write();
        state.poll_readable_lock();
        state.release_readable_lock();
        state.finish_write();
        assert!(state.is_done());
    }

    #[test]
    fn flushable_stream_state_propagates_errors_and_cancellation() {
        let mut state = FlushableStreamState::new();
        state.fail_stream("Writable stream closed prematurely");
        assert!(state.is_done());
        assert!(state.stream_ended());
        assert_eq!(
            state.rejection(),
            Some("Writable stream closed prematurely")
        );
        assert_eq!(
            state.cancel_reason(),
            Some("Writable stream closed prematurely")
        );
    }

    #[test]
    fn flushable_stream_state_tracks_concurrent_writes_and_single_pollers() {
        let mut state = FlushableStreamState::new();
        state.begin_write();
        state.begin_write();
        state.poll_writable_lock();
        state.poll_writable_lock();
        assert!(state.writable_polling_active());
        state.release_writable_lock();
        state.finish_write();
        assert!(!state.is_done());
        state.finish_write();
        assert!(state.is_done());

        let mut state = FlushableStreamState::new();
        state.poll_readable_lock();
        state.poll_readable_lock();
        assert!(state.readable_polling_active());
    }

    #[test]
    fn flushable_stream_state_handles_stream_end_while_ops_in_flight() {
        let mut state = FlushableStreamState::new();
        state.begin_write();
        state.poll_writable_lock();
        state.close_stream();
        assert!(state.is_done());
        assert!(state.stream_ended());
        assert!(!state.writable_polling_active());
    }

    #[test]
    fn replay_timeout_env_parsing_matches_upstream_bounds_and_warnings() {
        let mut warnings = ReplayTimeoutWarnings::new();
        assert_eq!(
            get_replay_timeout_ms(None, &mut warnings),
            REPLAY_TIMEOUT_MS
        );
        assert_eq!(
            get_replay_timeout_ms(Some(""), &mut warnings),
            REPLAY_TIMEOUT_MS
        );
        assert_eq!(
            get_replay_timeout_ms(Some("abc"), &mut warnings),
            REPLAY_TIMEOUT_MS
        );
        assert_eq!(
            get_replay_timeout_ms(Some("0"), &mut warnings),
            REPLAY_TIMEOUT_MS
        );
        assert_eq!(
            get_replay_timeout_ms(Some("-1"), &mut warnings),
            REPLAY_TIMEOUT_MS
        );
        assert_eq!(
            get_replay_timeout_ms(Some("1000"), &mut warnings),
            MIN_REPLAY_TIMEOUT_MS
        );
        assert_eq!(
            get_replay_timeout_ms(Some("999999"), &mut warnings),
            MAX_REPLAY_TIMEOUT_MS
        );
        assert_eq!(get_replay_timeout_ms(Some("60000"), &mut warnings), 60_000);
        assert_eq!(
            get_replay_timeout_ms(Some("30000"), &mut warnings),
            MIN_REPLAY_TIMEOUT_MS
        );
        assert_eq!(
            get_replay_timeout_ms(Some("780000"), &mut warnings),
            MAX_REPLAY_TIMEOUT_MS
        );
        assert_eq!(
            get_replay_timeout_ms(Some("Infinity"), &mut warnings),
            REPLAY_TIMEOUT_MS
        );
        assert_eq!(
            get_replay_timeout_ms(Some("NaN"), &mut warnings),
            REPLAY_TIMEOUT_MS
        );
        let before = warnings.warnings().len();
        let _ = get_replay_timeout_ms(Some("abc"), &mut warnings);
        assert_eq!(warnings.warnings().len(), before);
    }

    #[test]
    fn workflow_queue_name_validation_matches_upstream_safe_pattern() {
        for name in [
            "workflow",
            "abcXYZ123",
            "under_score-and-hyphen",
            "pkg.workflow",
            "folder/workflow",
            "@scope",
            "@scope/pkg.workflow",
        ] {
            assert_eq!(
                get_workflow_queue_name(name).expect("valid"),
                format!("__wkf_workflow_{name}")
            );
        }

        for name in ["has space", "bad!", ""] {
            assert!(get_workflow_queue_name(name).is_err());
        }
    }

    #[test]
    fn load_workflow_run_events_paginates_preserves_cursors_and_dedupes() {
        let mut world = InMemoryWorld::new();
        world.push_scripted_event_page(EventPage {
            data: vec![event("event-1", EventKind::RunCreated)],
            cursor: Some("cursor-1".to_string()),
            has_more: true,
        });
        world.push_scripted_event_page(EventPage {
            data: Vec::new(),
            cursor: None,
            has_more: false,
        });
        let loaded = load_workflow_run_events(&mut world, "wrun_test", None).expect("loaded");
        assert_eq!(loaded.events.len(), 1);
        assert_eq!(loaded.cursor.as_deref(), Some("cursor-1"));

        let mut world = InMemoryWorld::new();
        world.push_scripted_event_page(EventPage {
            data: vec![
                event("event-1", EventKind::RunCreated),
                event("event-2", EventKind::RunStarted),
            ],
            cursor: Some("cursor-2".to_string()),
            has_more: true,
        });
        world.push_scripted_event_page(EventPage {
            data: vec![
                event("event-2", EventKind::RunStarted),
                event("event-3", EventKind::StepCreated),
            ],
            cursor: Some("cursor-3".to_string()),
            has_more: false,
        });
        let loaded = load_workflow_run_events(&mut world, "wrun_test", None).expect("loaded");
        assert_eq!(
            loaded
                .events
                .iter()
                .map(|event| event.event_id.as_str())
                .collect::<Vec<_>>(),
            ["event-1", "event-2", "event-3"]
        );
        assert_eq!(loaded.cursor.as_deref(), Some("cursor-3"));
    }

    #[test]
    fn load_workflow_run_events_retries_bad_incremental_cursor_and_fails_bad_contracts() {
        let mut world = InMemoryWorld::new();
        world.insert_event("wrun_test", event("event-1", EventKind::RunCreated));
        world.rejected_cursors.insert("cursor_bad".to_string());
        let loaded =
            load_workflow_run_events(&mut world, "wrun_test", Some("cursor_bad".to_string()))
                .expect("retried full reload");
        assert_eq!(loaded.events.len(), 1);

        let mut world = InMemoryWorld::new();
        world.push_scripted_event_page(EventPage {
            data: vec![event("event-1", EventKind::RunCreated)],
            cursor: Some("same".to_string()),
            has_more: true,
        });
        world.push_scripted_event_page(EventPage {
            data: vec![event("event-2", EventKind::RunStarted)],
            cursor: Some("same".to_string()),
            has_more: true,
        });
        assert_eq!(
            load_workflow_run_events(&mut world, "wrun_test", None)
                .expect_err("repeat cursor")
                .kind,
            WorkflowErrorKind::WorldContract
        );

        let mut world = InMemoryWorld::new();
        world.push_scripted_event_page(EventPage {
            data: vec![event("event-1", EventKind::RunCreated)],
            cursor: None,
            has_more: true,
        });
        assert_eq!(
            load_workflow_run_events(&mut world, "wrun_test", None)
                .expect_err("missing cursor")
                .kind,
            WorkflowErrorKind::WorldContract
        );
    }

    #[test]
    fn replay_budget_accounts_for_non_step_time_and_pause_resume_cycles() {
        let mut budget = ReplayBudget::new(100, 0);
        assert_eq!(budget.elapsed(25), 25);
        budget.pause(25);
        assert_eq!(budget.elapsed(500), 25);
        budget.pause(600);
        assert_eq!(budget.elapsed(700), 25);
        budget.resume(700);
        budget.resume(750);
        assert_eq!(budget.elapsed(800), 125);
        assert!(budget.is_exhausted(800));

        let mut budget = ReplayBudget::new(REPLAY_TIMEOUT_MS, 0);
        budget.pause(0);
        assert!(!budget.is_exhausted(480_000));
    }

    #[test]
    fn replay_budget_exhaustion_uses_world_redelivery_capability() {
        let mut world = world_with_running_run("wrun_replay");
        let action = handle_replay_budget_exhausted(
            &mut world,
            "wrun_replay",
            "workflow",
            Some("req".to_string()),
            1,
            30_000,
        )
        .expect("handled");
        assert_eq!(action, ReplayTimeoutAction::Returned);
        assert_eq!(
            world.runs_get("wrun_replay").unwrap().status,
            RunStatus::Failed
        );

        let mut world = world_with_running_run("wrun_replay");
        world.process_exit_triggers_queue_redelivery = true;
        let action =
            handle_replay_budget_exhausted(&mut world, "wrun_replay", "workflow", None, 1, 30_000)
                .expect("handled");
        assert_eq!(action, ReplayTimeoutAction::ExitForRedelivery);
        assert_eq!(
            world.runs_get("wrun_replay").unwrap().status,
            RunStatus::Running
        );

        let action = handle_replay_budget_exhausted(
            &mut world,
            "wrun_replay",
            "workflow",
            None,
            REPLAY_TIMEOUT_MAX_RETRIES + 1,
            30_000,
        )
        .expect("handled");
        assert_eq!(action, ReplayTimeoutAction::ExitForRedelivery);
        assert_eq!(
            world.runs_get("wrun_replay").unwrap().status,
            RunStatus::Failed
        );
    }

    #[test]
    fn start_validates_workflow_and_resolves_spec_deployment_and_encryption_context() {
        let mut world = InMemoryWorld::new();
        assert!(start(&mut world, None, Vec::new(), StartOptions::default()).is_err());
        assert!(
            start(
                &mut world,
                Some(&WorkflowDefinition::new("")),
                Vec::new(),
                StartOptions::default(),
            )
            .is_err()
        );

        let mut world = InMemoryWorld::new();
        world.spec_version = None;
        let run = start(
            &mut world,
            Some(&WorkflowDefinition::new("test-workflow")),
            vec![json!(42)],
            StartOptions::default(),
        )
        .expect("started");
        assert!(run.run_id.starts_with("wrun_"));
        assert_eq!(
            world.last_event(&run.run_id).unwrap().event_type,
            EventKind::RunCreated
        );
        assert_eq!(
            world.last_event(&run.run_id).unwrap().spec_version,
            SPEC_VERSION_SUPPORTS_EVENT_SOURCING
        );

        let mut world = InMemoryWorld::new();
        let _ = start(
            &mut world,
            Some(&WorkflowDefinition::new("test-workflow")),
            Vec::new(),
            StartOptions {
                spec_version: Some(SPEC_VERSION_LEGACY),
                ..StartOptions::default()
            },
        )
        .expect("started");
        assert_eq!(world.last_event("wrun_000001").unwrap().spec_version, 1);

        let mut world = InMemoryWorld::new();
        let _ = start(
            &mut world,
            Some(&WorkflowDefinition::new("test-workflow")),
            Vec::new(),
            StartOptions {
                deployment_id: Some(DeploymentId::Latest),
                ..StartOptions::default()
            },
        )
        .expect("started latest");
        assert_eq!(world.resolve_latest_calls, 1);
        assert_eq!(
            world.encryption_contexts[0].1.deployment_id.as_deref(),
            Some("deploy_latest")
        );

        let mut world = InMemoryWorld::new();
        let _ = start(
            &mut world,
            Some(&WorkflowDefinition::new("test-workflow")),
            Vec::new(),
            StartOptions {
                deployment_id: Some(DeploymentId::Id("deploy_explicit".to_string())),
                ..StartOptions::default()
            },
        )
        .expect("started explicit");
        assert_eq!(world.resolve_latest_calls, 0);
        assert_eq!(
            world.encryption_contexts[0].1.deployment_id.as_deref(),
            Some("deploy_explicit")
        );
    }

    #[test]
    fn start_resilient_start_and_failure_paths_match_upstream() {
        let mut world = InMemoryWorld::new();
        world.spec_version = Some(SPEC_VERSION_SUPPORTS_CBOR_QUEUE_TRANSPORT);
        world.fail_next_event_create(
            Some(EventKind::RunCreated),
            WorkflowCoreError::world("Internal Server Error", Some(500)),
        );
        let run = start(
            &mut world,
            Some(&WorkflowDefinition::new("test-workflow")),
            vec![json!(42)],
            StartOptions::default(),
        )
        .expect("resilient start");
        assert!(run.resilient_start);
        assert!(world.queues[0].message.run_input.is_some());

        let mut world = InMemoryWorld::new();
        world.fail_next_queue(WorkflowCoreError::queue("Queue unavailable"));
        let error = start(
            &mut world,
            Some(&WorkflowDefinition::new("test-workflow")),
            Vec::new(),
            StartOptions::default(),
        )
        .expect_err("queue failure");
        assert_eq!(error.kind, WorkflowErrorKind::Queue);

        let mut world = InMemoryWorld::new();
        world.fail_next_event_create(
            Some(EventKind::RunCreated),
            WorkflowCoreError::world("Bad Request", Some(400)),
        );
        let error = start(
            &mut world,
            Some(&WorkflowDefinition::new("test-workflow")),
            Vec::new(),
            StartOptions::default(),
        )
        .expect_err("non-retryable event failure");
        assert_eq!(error.status, Some(400));
    }

    #[test]
    fn run_handle_exists_wakeup_serialization_and_return_value_errors() {
        let mut world = world_with_running_run("wrun_run");
        let handle = RunHandle::new("wrun_run");
        assert!(handle.exists(&world).expect("exists"));
        assert!(!RunHandle::new("missing").exists(&world).expect("missing"));
        world.runs_get_error = Some(WorkflowCoreError::world("backend down", Some(503)));
        assert_eq!(
            handle.exists(&world).expect_err("rethrows non-404").status,
            Some(503)
        );
        world.runs_get_error = None;

        world
            .events_create(
                "wrun_run",
                CreateEventRequest::new(EventKind::WaitCreated)
                    .with_correlation_id("wait-a")
                    .with_data(json!({ "resumeAt": 10 })),
                EventCreateOptions::default(),
            )
            .expect("wait");
        world.fail_next_event_create(
            Some(EventKind::WaitCompleted),
            WorkflowCoreError::conflict("already completed"),
        );
        assert_eq!(
            handle
                .wake_up(&mut world, StopSleepOptions::default())
                .expect("wake")
                .stopped_count,
            1
        );

        let serialized = handle.serialize();
        assert_eq!(
            RunHandle::deserialize(&serialized).expect("deserialize"),
            handle
        );

        let mut completed = running_run("wrun_done");
        completed.status = RunStatus::Completed;
        completed.output = Some(json!({"ok": true}));
        world.insert_run(completed);
        assert_eq!(
            RunHandle::new("wrun_done")
                .return_value(&world)
                .expect("value"),
            json!({"ok": true})
        );

        let mut failed = running_run("wrun_failed");
        failed.status = RunStatus::Failed;
        failed.error_code = Some(RunErrorCode::WorkflowRuntimeError);
        failed.error = Some(json!({
            "message": "outer failure",
            "cause": "inner cause",
        }));
        world.insert_run(failed);
        let error = RunHandle::new("wrun_failed")
            .return_value(&world)
            .expect_err("failed");
        assert_eq!(error.kind, WorkflowErrorKind::Fatal);
        assert_eq!(error.cause.as_deref(), Some("inner cause"));

        let mut failed = running_run("wrun_fallback");
        failed.status = RunStatus::Failed;
        failed.error = Some(json!({ "opaque": true }));
        world.insert_run(failed);
        assert!(
            RunHandle::new("wrun_fallback")
                .return_value(&world)
                .expect_err("fallback")
                .message
                .contains("Failed to hydrate workflow run error")
        );
    }

    #[test]
    fn run_wakeup_targets_pending_waits_and_queues_continuation() {
        let mut world = world_with_running_run("wrun_sleep");
        for wait_id in ["wait-a", "wait-b"] {
            world
                .events_create(
                    "wrun_sleep",
                    CreateEventRequest::new(EventKind::WaitCreated)
                        .with_correlation_id(wait_id)
                        .with_data(json!({ "resumeAt": 10 })),
                    EventCreateOptions::default(),
                )
                .expect("wait");
        }
        let result = wake_up_run(
            &mut world,
            "wrun_sleep",
            StopSleepOptions {
                correlation_ids: Some(vec!["wait-b".to_string()]),
            },
        )
        .expect("wake targeted");
        assert_eq!(result.stopped_count, 1);
        assert_eq!(world.queues.len(), 1);

        let result =
            wake_up_run(&mut world, "wrun_sleep", StopSleepOptions::default()).expect("wake all");
        assert_eq!(result.stopped_count, 1);

        let result =
            wake_up_run(&mut world, "wrun_sleep", StopSleepOptions::default()).expect("wake none");
        assert_eq!(result.stopped_count, 0);

        let mut world = world_with_running_run("wrun_sleep_error");
        world
            .events_create(
                "wrun_sleep_error",
                CreateEventRequest::new(EventKind::WaitCreated)
                    .with_correlation_id("wait-error")
                    .with_data(json!({ "resumeAt": 10 })),
                EventCreateOptions::default(),
            )
            .expect("wait");
        world.fail_next_event_create(
            Some(EventKind::WaitCompleted),
            WorkflowCoreError::world("backend failed", Some(500)),
        );
        assert_eq!(
            wake_up_run(&mut world, "wrun_sleep_error", StopSleepOptions::default())
                .expect_err("non-conflict wake error")
                .status,
            Some(500)
        );
    }

    #[test]
    fn step_executor_handles_conflicts_and_request_id_propagation() {
        let mut world = world_with_running_run("wrun_step");
        create_step(&mut world, "wrun_step", "step-1", "step");
        let mut registry = StepRegistry::new();
        registry.register("step", StepFunction::complete(json!("ok")));

        let result = execute_step(
            &mut world,
            &registry,
            step_params("wrun_step", "step-1", "step"),
        )
        .expect("step");
        assert_eq!(
            result,
            StepExecutionResult::Completed {
                has_pending_ops: false
            }
        );
        let request_ids = world
            .events
            .get("wrun_step")
            .unwrap()
            .iter()
            .filter(|event| {
                matches!(
                    event.event_type,
                    EventKind::StepStarted | EventKind::StepCompleted
                )
            })
            .map(|event| event.request_id.as_deref())
            .collect::<Vec<_>>();
        assert_eq!(request_ids, [Some("req_test"), Some("req_test")]);

        create_step(&mut world, "wrun_step", "step-2", "step");
        world.fail_next_event_create(
            Some(EventKind::StepCompleted),
            WorkflowCoreError::conflict("already complete"),
        );
        assert_eq!(
            execute_step(
                &mut world,
                &registry,
                step_params("wrun_step", "step-2", "step"),
            )
            .expect("conflict"),
            StepExecutionResult::Skipped
        );
    }

    #[test]
    fn step_executor_handles_missing_fatal_abort_and_retryable_failures() {
        let mut world = world_with_running_run("wrun_step");
        let registry = StepRegistry::new();
        assert_eq!(
            execute_step(
                &mut world,
                &registry,
                step_params("wrun_step", "missing", "missing"),
            )
            .expect("missing"),
            StepExecutionResult::Failed
        );

        let mut registry = StepRegistry::new();
        registry.register_non_function("not-a-function");
        assert_eq!(
            execute_step(
                &mut world,
                &registry,
                step_params("wrun_step", "not-fn", "not-a-function"),
            )
            .expect("not function"),
            StepExecutionResult::Failed
        );

        create_step(&mut world, "wrun_step", "fatal", "fatal");
        let mut registry = StepRegistry::new();
        registry.register(
            "fatal",
            StepFunction {
                max_retries: 3,
                behavior: StepBehavior::Fatal("fatal boom".to_string()),
            },
        );
        assert_eq!(
            execute_step(
                &mut world,
                &registry,
                step_params("wrun_step", "fatal", "fatal"),
            )
            .expect("fatal"),
            StepExecutionResult::Failed
        );

        create_step(&mut world, "wrun_step", "retry", "retry");
        let mut registry = StepRegistry::new();
        registry.register(
            "retry",
            StepFunction {
                max_retries: 1,
                behavior: StepBehavior::Retryable {
                    message: "try again".to_string(),
                    retry_after_seconds: 2,
                },
            },
        );
        assert_eq!(
            execute_step(
                &mut world,
                &registry,
                step_params("wrun_step", "retry", "retry"),
            )
            .expect("retry"),
            StepExecutionResult::Retry { timeout_seconds: 2 }
        );
        world.advance_ms(2_000);
        assert_eq!(
            execute_step(
                &mut world,
                &registry,
                step_params("wrun_step", "retry", "retry"),
            )
            .expect("retry exhausted"),
            StepExecutionResult::Failed
        );

        create_step(&mut world, "wrun_step", "abort", "abort");
        let mut registry = StepRegistry::new();
        registry.register(
            "abort",
            StepFunction {
                max_retries: 3,
                behavior: StepBehavior::Abort("AbortError".to_string()),
            },
        );
        assert_eq!(
            execute_step(
                &mut world,
                &registry,
                step_params("wrun_step", "abort", "abort"),
            )
            .expect("abort"),
            StepExecutionResult::Failed
        );
    }

    #[test]
    fn step_handler_max_deliveries_and_under_limit_paths() {
        let mut world = world_with_running_run("wrun_step");
        let registry = StepRegistry::new();
        handle_step_message(
            &mut world,
            &registry,
            StepHandlerMessage {
                workflow_run_id: "wrun_step".to_string(),
                workflow_name: "workflow".to_string(),
                workflow_started_at_ms: 0,
                step_id: "step-max".to_string(),
                step_name: "step".to_string(),
                attempt: MAX_QUEUE_DELIVERIES + 1,
                request_id: Some("req-max".to_string()),
            },
        )
        .expect("max deliveries");
        assert_eq!(world.queues.len(), 1);

        let mut world = world_with_running_run("wrun_step");
        world.fail_next_event_create(
            Some(EventKind::StepFailed),
            WorkflowCoreError::conflict("already failed"),
        );
        handle_step_message(
            &mut world,
            &registry,
            StepHandlerMessage {
                workflow_run_id: "wrun_step".to_string(),
                workflow_name: "workflow".to_string(),
                workflow_started_at_ms: 0,
                step_id: "step-max".to_string(),
                step_name: "step".to_string(),
                attempt: MAX_QUEUE_DELIVERIES + 1,
                request_id: None,
            },
        )
        .expect("conflict consumed");
        assert!(world.queues.is_empty());

        let mut world = world_with_running_run("wrun_step");
        create_step(&mut world, "wrun_step", "step-ok", "step");
        let mut registry = StepRegistry::new();
        registry.register("step", StepFunction::complete(json!("ok")));
        assert!(matches!(
            handle_step_message(
                &mut world,
                &registry,
                StepHandlerMessage {
                    workflow_run_id: "wrun_step".to_string(),
                    workflow_name: "workflow".to_string(),
                    workflow_started_at_ms: 0,
                    step_id: "step-ok".to_string(),
                    step_name: "step".to_string(),
                    attempt: MAX_QUEUE_DELIVERIES,
                    request_id: None,
                },
            )
            .expect("under limit"),
            Some(StepExecutionResult::Completed { .. })
        ));
    }

    #[test]
    fn suspension_handler_orders_hooks_steps_waits_and_detects_conflicts() {
        let mut world = world_with_running_run("wrun_suspend");
        let run = world.runs_get("wrun_suspend").unwrap();
        let suspension = WorkflowSuspension {
            items: vec![
                SuspensionItem::Hook {
                    correlation_id: "hook-a".to_string(),
                    token: "token-a".to_string(),
                    metadata: json!({"kind": "hook"}),
                    has_created_event: false,
                    disposed: false,
                    abort_requested: false,
                    abort_reason: None,
                    is_webhook: false,
                    is_system: false,
                },
                SuspensionItem::Step {
                    correlation_id: "step-a".to_string(),
                    step_name: "step".to_string(),
                    input: json!([1]),
                    has_created_event: false,
                },
                SuspensionItem::Wait {
                    correlation_id: "wait-a".to_string(),
                    resume_at_ms: 5_000,
                    has_created_event: false,
                },
            ],
        };
        let result = handle_suspension(&mut world, &run, &suspension, Some("req".to_string()))
            .expect("suspension");
        assert_eq!(result.pending_steps.len(), 1);
        assert!(result.created_step_correlation_ids.contains("step-a"));
        assert_eq!(result.timeout_seconds, Some(5));
        assert_eq!(
            world.event_kinds("wrun_suspend"),
            [
                EventKind::HookCreated,
                EventKind::StepCreated,
                EventKind::WaitCreated
            ]
        );

        let mut world = world_with_running_run("wrun_conflict");
        world
            .active_hook_tokens
            .insert("token-a".to_string(), "other-run".to_string());
        let run = world.runs_get("wrun_conflict").unwrap();
        let result = handle_suspension(
            &mut world,
            &run,
            &WorkflowSuspension {
                items: vec![SuspensionItem::Hook {
                    correlation_id: "hook-a".to_string(),
                    token: "token-a".to_string(),
                    metadata: Value::Null,
                    has_created_event: false,
                    disposed: false,
                    abort_requested: false,
                    abort_reason: None,
                    is_webhook: false,
                    is_system: false,
                }],
            },
            None,
        )
        .expect("hook conflict");
        assert!(result.has_hook_conflict);
        assert_eq!(result.timeout_seconds, Some(0));
        assert_eq!(
            world.event_kinds("wrun_conflict"),
            [EventKind::HookConflict]
        );
    }

    #[test]
    fn hook_sleep_interaction_preserves_waiters_steps_and_payload_ordering() {
        let mut world = world_with_running_run("wrun_hook_sleep");
        let run = world.runs_get("wrun_hook_sleep").unwrap();
        let suspension = WorkflowSuspension {
            items: vec![
                SuspensionItem::Wait {
                    correlation_id: "wait-early".to_string(),
                    resume_at_ms: 1_000,
                    has_created_event: false,
                },
                SuspensionItem::Step {
                    correlation_id: "step-fire-forget".to_string(),
                    step_name: "payload-step".to_string(),
                    input: json!({"payload": "A"}),
                    has_created_event: false,
                },
                SuspensionItem::Hook {
                    correlation_id: "hook-stream".to_string(),
                    token: "token-stream".to_string(),
                    metadata: Value::Null,
                    has_created_event: false,
                    disposed: false,
                    abort_requested: true,
                    abort_reason: Some("timeout".to_string()),
                    is_webhook: false,
                    is_system: true,
                },
            ],
        };
        let result = handle_suspension(&mut world, &run, &suspension, None).expect("suspend");
        assert_eq!(result.timeout_seconds, Some(1));
        assert_eq!(result.pending_steps.len(), 1);
        assert_eq!(
            world.event_kinds("wrun_hook_sleep"),
            [
                EventKind::HookCreated,
                EventKind::HookReceived,
                EventKind::WaitCreated,
                EventKind::StepCreated,
            ]
        );

        let mut consumer = EventsConsumer::new(vec![
            event_with_data(
                "event-a",
                EventKind::HookReceived,
                "hook-stream",
                json!({"token": "token-stream", "payload": "A"}),
            ),
            event_with_data(
                "event-b",
                EventKind::HookReceived,
                "hook-stream",
                json!({"token": "token-stream", "payload": "B"}),
            ),
        ]);
        consumer.subscribe(|event| {
            if let Some(event) = event {
                assert_eq!(
                    event.event_data.get("token").and_then(Value::as_str),
                    Some("token-stream")
                );
                EventConsumerResult::Consumed
            } else {
                EventConsumerResult::NotConsumed
            }
        });
        consumer.flush_deferred_unconsumed_check();
        assert!(consumer.unconsumed_events().is_empty());
    }

    #[test]
    fn wait_completion_replay_uses_incremental_cursor_and_falls_back_when_needed() {
        let base_events = vec![event_with_data(
            "event-1",
            EventKind::WaitCreated,
            "wait-a",
            json!({ "resumeAt": 10 }),
        )];

        let mut world = world_with_running_run("wrun_wait");
        world.push_scripted_event_page(EventPage {
            data: vec![event_with_data(
                "event-2",
                EventKind::WaitCompleted,
                "wait-a",
                json!({ "resumeAt": 10 }),
            )],
            cursor: Some("cursor-2".to_string()),
            has_more: false,
        });
        let refreshed = complete_elapsed_waits_and_refresh(
            &mut world,
            "wrun_wait",
            base_events.clone(),
            Some("cursor-1".to_string()),
            20,
            None,
        )
        .expect("refresh");
        assert!(refreshed.used_incremental);
        assert!(!refreshed.used_full_reload);
        assert_eq!(refreshed.events.len(), 2);

        let mut world = world_with_running_run("wrun_wait");
        world.insert_event("wrun_wait", base_events[0].clone());
        let refreshed = complete_elapsed_waits_and_refresh(
            &mut world,
            "wrun_wait",
            base_events.clone(),
            None,
            20,
            None,
        )
        .expect("full reload without cursor");
        assert!(refreshed.used_full_reload);

        let mut world = world_with_running_run("wrun_wait");
        world.insert_event("wrun_wait", base_events[0].clone());
        world.push_scripted_event_page(EventPage {
            data: vec![event("event-x", EventKind::RunStarted)],
            cursor: Some("cursor-x".to_string()),
            has_more: false,
        });
        let refreshed = complete_elapsed_waits_and_refresh(
            &mut world,
            "wrun_wait",
            base_events,
            Some("cursor-1".to_string()),
            20,
            None,
        )
        .expect("fallback when delta misses completion");
        assert!(refreshed.used_full_reload);
    }

    #[test]
    fn workflow_entrypoint_guards_record_world_contract_and_corrupted_logs() {
        let mut world = world_with_running_run("wrun_guard");
        record_run_failure(
            &mut world,
            "wrun_guard",
            Some("req".to_string()),
            RunErrorCode::WorldContractError,
            "Schema validation failed",
        )
        .expect("recorded");
        assert_eq!(
            world.runs_get("wrun_guard").unwrap().status,
            RunStatus::Failed
        );
        assert_eq!(
            world.runs_get("wrun_guard").unwrap().error_code,
            Some(RunErrorCode::WorldContractError)
        );

        let bad_waits = vec![
            event_with_data(
                "event-1",
                EventKind::WaitCreated,
                "wait-a",
                json!({ "resumeAt": 5 }),
            ),
            event_with_data(
                "event-2",
                EventKind::WaitCompleted,
                "wait-a",
                json!({ "resumeAt": 6 }),
            ),
        ];
        assert_eq!(
            validate_replay_event_log(&bad_waits)
                .expect_err("bad wait")
                .code,
            Some(RunErrorCode::CorruptedEventLog)
        );

        let bad_hook = vec![event_with_data(
            "event-1",
            EventKind::HookReceived,
            "hook-a",
            json!({ "token": "wrong-token" }),
        )];
        assert_eq!(
            validate_replay_event_log(&bad_hook)
                .expect_err("bad hook")
                .code,
            Some(RunErrorCode::CorruptedEventLog)
        );
    }

    #[test]
    fn world_init_registry_registers_lazy_world_and_preserves_prior_registration() {
        let mut registry = WorldRegistry::new();
        registry.register_get_world("host-world");
        assert_eq!(registry.get_world_lazy(), "host-world");
        assert_eq!(registry.fallback_imports(), 0);
        registry.register_get_world("replacement");
        assert_eq!(registry.get_world_lazy(), "host-world");

        let mut registry = WorldRegistry::new();
        assert_eq!(registry.get_world_lazy(), "dynamic-world");
        assert_eq!(registry.fallback_imports(), 1);
    }
}
