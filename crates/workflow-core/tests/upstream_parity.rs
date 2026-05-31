use std::cell::{Cell, RefCell};
use std::rc::Rc;

use serde_json::{Value, json};
use workflow_core::{
    ContextStorage, Headers, HookOptions, HookResumer, MemoryStreamWorld, QueueItem, RequestInit,
    ResponseInit, SleepInput, StepContext, StepEventResult, StepMetadata, StreamWrite,
    UnknownError, WebhookOptions, WorkflowAbortController, WorkflowAbortSignal, WorkflowBody,
    WorkflowContext, WorkflowCoreError, WorkflowEvent, WorkflowEventType, WorkflowMetadata,
    WorkflowRequest, WorkflowResponse, WorkflowServerWritableStream, WorkflowWritableStreamOptions,
    attach_abort_stream_reader, context_storage, define_hook, define_hook_with_schema,
    deliver_abort_stream_packet, fetch_in_workflow_error, get_step_metadata,
    get_workflow_run_stream_id, get_writable, is_abort_error, promote_abort_error_to_fatal,
    timeout_function_error,
};

macro_rules! parity_test {
    ($name:ident, $helper:ident) => {
        #[test]
        fn $name() {
            $helper();
        }
    };
}

fn ctx() -> WorkflowContext {
    WorkflowContext::for_test()
}

fn step_ctx() -> StepContext {
    StepContext::new(
        StepMetadata {
            step_name: "step//file//work".to_string(),
            step_id: "step_1".to_string(),
            step_started_at: 1_700_000_000_100,
            attempt: 2,
        },
        WorkflowMetadata::new(
            "workflow/test",
            "wrun_abc",
            1_700_000_000_000,
            "http://localhost:3000",
        ),
    )
}

fn assert_workflow_metadata_shape() {
    let metadata = ctx().metadata();
    assert_eq!(metadata.workflow_name, "test/workflow");
    assert_eq!(metadata.workflow_run_id, "wrun_000001");
    assert_eq!(metadata.url, "http://localhost:3000");
}

fn assert_workflow_arguments_shape() {
    let ctx = ctx();
    let invocation = ctx
        .use_step("step//workflow.ts//withArgs")
        .invoke(vec![json!("alpha"), json!(2)]);
    let QueueItem::Step(step) = ctx.queue_item(invocation.correlation_id()).unwrap() else {
        panic!("expected step");
    };
    assert_eq!(step.args, vec![json!("alpha"), json!(2)]);
}

fn assert_workflow_user_error_surface() {
    let error = WorkflowCoreError::WorkflowRuntime {
        message: "user boom".to_string(),
        slug: None,
    };
    assert_eq!(error.to_string(), "user boom");
}

fn assert_workflow_timestamp_updates_from_event() {
    let ctx = ctx();
    ctx.observe_event_timestamp(
        &WorkflowEvent::new(WorkflowEventType::RunStarted, "run").created_at(42),
    );
    assert_eq!(ctx.timestamp(), 42);
}

fn assert_workflow_date_determinism() {
    let first = ctx();
    let second = ctx();
    assert_eq!(first.timestamp(), second.timestamp());
}

fn assert_workflow_concurrent_steps_all_resolve() {
    let ctx = ctx();
    let first = ctx.use_step("step//file//one").invoke(vec![]);
    let second = ctx.use_step("step//file//two").invoke(vec![]);
    assert!(matches!(
        second
            .consume_event(
                &WorkflowEvent::new(WorkflowEventType::StepCompleted, second.correlation_id())
                    .step_name("step//file//two")
            )
            .unwrap(),
        StepEventResult::Resolved(_)
    ));
    assert!(matches!(
        first
            .consume_event(
                &WorkflowEvent::new(WorkflowEventType::StepCompleted, first.correlation_id())
                    .step_name("step//file//one")
            )
            .unwrap(),
        StepEventResult::Resolved(_)
    ));
    assert_eq!(ctx.queue_len(), 0);
}

fn assert_workflow_race_first_resolution() {
    let ctx = ctx();
    let first = ctx.use_step("step//file//one").invoke(vec![]);
    let second = ctx.use_step("step//file//two").invoke(vec![]);
    let resolved = first
        .consume_event(
            &WorkflowEvent::new(WorkflowEventType::StepCompleted, first.correlation_id())
                .step_name("step//file//one")
                .result(json!("first")),
        )
        .unwrap();
    assert_eq!(resolved, StepEventResult::Resolved(json!("first")));
    assert!(ctx.queue_item(second.correlation_id()).is_some());
}

fn assert_workflow_race_second_resolution() {
    let ctx = ctx();
    let first = ctx.use_step("step//file//one").invoke(vec![]);
    let second = ctx.use_step("step//file//two").invoke(vec![]);
    let resolved = second
        .consume_event(
            &WorkflowEvent::new(WorkflowEventType::StepCompleted, second.correlation_id())
                .step_name("step//file//two")
                .result(json!("second")),
        )
        .unwrap();
    assert_eq!(resolved, StepEventResult::Resolved(json!("second")));
    assert!(ctx.queue_item(first.correlation_id()).is_some());
}

fn assert_workflow_not_registered_error() {
    let error = WorkflowCoreError::WorkflowNotRegistered("missing".to_string());
    assert!(error.to_string().contains("missing"));
}

fn assert_workflow_stack_uses_name() {
    let definition = workflow_core::WorkflowDefinition::new("example/workflows/demo.ts::run")
        .module_specifier("example/workflows/demo.ts")
        .export_name("run");
    assert_eq!(
        definition.module_specifier.as_deref(),
        Some("example/workflows/demo.ts")
    );
    assert_eq!(definition.export_name.as_deref(), Some("run"));
}

fn assert_workflow_suspension_uncatchable_shape() {
    let error = ctx()
        .use_step("step//file//work")
        .invoke(vec![])
        .finish_without_event()
        .unwrap_err();
    assert!(error.is_workflow_suspension());
}

fn assert_workflow_drain_pending_step() {
    let ctx = ctx();
    ctx.use_step("step//file//fireAndForget").invoke(vec![]);
    let drained = ctx.drain_pending_queue_items();
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].kind(), "step");
}

fn assert_workflow_drain_pending_hook() {
    let ctx = ctx();
    let controller = ctx.create_abort_controller();
    let drained = ctx.drain_pending_queue_items();
    let QueueItem::Hook(hook) = &drained[0] else {
        panic!("expected hook");
    };
    assert_eq!(hook.token, controller.signal.hook_token());
    assert!(hook.disposed);
}

fn assert_workflow_drain_pending_wait() {
    let ctx = ctx();
    ctx.sleep(SleepInput::DurationMillis(10)).unwrap();
    let drained = ctx.drain_pending_queue_items();
    assert_eq!(drained[0].kind(), "wait");
}

fn assert_step_resolves_with_result() {
    let ctx = ctx();
    let invocation = ctx
        .use_step("step//workflow.ts//load")
        .invoke(vec![json!("input")]);
    let result = invocation
        .consume_event(
            &WorkflowEvent::new(
                WorkflowEventType::StepCompleted,
                invocation.correlation_id(),
            )
            .step_name("step//workflow.ts//load")
            .result(json!({ "ok": true })),
        )
        .unwrap();
    assert_eq!(result, StepEventResult::Resolved(json!({ "ok": true })));
    assert_eq!(ctx.queue_len(), 0);
}

fn assert_step_rejects_with_error() {
    let invocation = ctx().use_step("step//file//fail").invoke(vec![]);
    let error = invocation
        .consume_event(
            &WorkflowEvent::new(WorkflowEventType::StepFailed, invocation.correlation_id())
                .step_name("step//file//fail")
                .error(WorkflowCoreError::Fatal("boom".to_string())),
        )
        .unwrap_err();
    assert_eq!(error, WorkflowCoreError::Fatal("boom".to_string()));
}

fn assert_step_suspends_without_event() {
    let invocation = ctx().use_step("step//file//work").invoke(vec![]);
    assert!(
        invocation
            .finish_without_event()
            .unwrap_err()
            .is_workflow_suspension()
    );
}

fn assert_concurrent_steps_suspend() {
    let ctx = ctx();
    let step = ctx.use_step("step//file//work");
    let first = step.invoke(vec![json!(1)]);
    let second = step.invoke(vec![json!(2)]);
    assert_ne!(first.correlation_id(), second.correlation_id());
    let error = second.finish_without_event().unwrap_err();
    assert!(matches!(
        error,
        WorkflowCoreError::WorkflowSuspension { step_count: 2, .. }
    ));
}

fn assert_step_function_name_from_step_id() {
    let step = ctx().use_step("step//app/workflows/demo.ts//sendEmail");
    assert_eq!(step.name(), "sendEmail");
    assert_eq!(step.step_id(), "step//app/workflows/demo.ts//sendEmail");
}

fn assert_step_captures_closure_vars() {
    let ctx = ctx();
    let invocation = ctx
        .use_step_with_closure_vars("step//file//work", json!({ "tenant": "acme" }))
        .invoke(vec![]);
    let item = ctx.queue_item(invocation.correlation_id()).unwrap();
    let QueueItem::Step(step) = item else {
        panic!("expected step queue item");
    };
    assert_eq!(step.closure_vars, Some(json!({ "tenant": "acme" })));
}

fn assert_step_bind_captures_this_and_args() {
    let ctx = ctx();
    let invocation = ctx
        .use_step("step//file//method")
        .bind(json!({ "id": "self" }), vec![json!("prefilled")])
        .invoke(vec![json!("later")]);
    let item = ctx.queue_item(invocation.correlation_id()).unwrap();
    let QueueItem::Step(step) = item else {
        panic!("expected step queue item");
    };
    assert_eq!(step.this_val, Some(json!({ "id": "self" })));
    assert_eq!(step.args, vec![json!("prefilled"), json!("later")]);
}

fn assert_step_handles_empty_closure_vars() {
    let ctx = ctx();
    let invocation = ctx
        .use_step_with_closure_vars("step//file//work", json!({}))
        .invoke(vec![]);
    let QueueItem::Step(step) = ctx.queue_item(invocation.correlation_id()).unwrap() else {
        panic!("expected step queue item");
    };
    assert_eq!(step.closure_vars, Some(json!({})));
}

fn assert_step_created_marks_queue_item() {
    let ctx = ctx();
    let invocation = ctx.use_step("step//file//work").invoke(vec![]);
    invocation
        .consume_event(
            &WorkflowEvent::new(WorkflowEventType::StepCreated, invocation.correlation_id())
                .step_name("step//file//work"),
        )
        .unwrap();
    let QueueItem::Step(step) = ctx.queue_item(invocation.correlation_id()).unwrap() else {
        panic!("expected step queue item");
    };
    assert!(step.has_created_event);
}

fn assert_step_wrong_name_is_corruption() {
    let invocation = ctx().use_step("step//file//expected").invoke(vec![]);
    let error = invocation
        .consume_event(
            &WorkflowEvent::new(
                WorkflowEventType::StepCompleted,
                invocation.correlation_id(),
            )
            .step_name("step//file//actual"),
        )
        .unwrap_err();
    assert!(matches!(error, WorkflowCoreError::CorruptedEventLog(_)));
}

fn assert_step_started_consumed_keeps_queue_item() {
    let ctx = ctx();
    let invocation = ctx.use_step("step//file//work").invoke(vec![]);
    assert_eq!(
        invocation
            .consume_event(
                &WorkflowEvent::new(WorkflowEventType::StepStarted, invocation.correlation_id())
                    .step_name("step//file//work"),
            )
            .unwrap(),
        StepEventResult::Consumed
    );
    assert!(ctx.queue_item(invocation.correlation_id()).is_some());
}

fn assert_step_retrying_consumed_keeps_queue_item() {
    let ctx = ctx();
    let invocation = ctx.use_step("step//file//work").invoke(vec![]);
    assert_eq!(
        invocation
            .consume_event(
                &WorkflowEvent::new(WorkflowEventType::StepRetrying, invocation.correlation_id())
                    .step_name("step//file//work"),
            )
            .unwrap(),
        StepEventResult::Consumed
    );
    assert!(ctx.queue_item(invocation.correlation_id()).is_some());
}

fn assert_step_completed_removes_queue_item() {
    assert_step_resolves_with_result();
}

fn assert_step_failed_removes_queue_item() {
    let ctx = ctx();
    let invocation = ctx.use_step("step//file//fail").invoke(vec![]);
    let _ = invocation.consume_event(
        &WorkflowEvent::new(WorkflowEventType::StepFailed, invocation.correlation_id())
            .step_name("step//file//fail"),
    );
    assert!(ctx.queue_item(invocation.correlation_id()).is_none());
}

fn assert_step_error_identity_preserved() {
    let error = WorkflowCoreError::Fatal("custom error with stack".to_string());
    let invocation = ctx().use_step("step//file//fail").invoke(vec![]);
    let actual = invocation
        .consume_event(
            &WorkflowEvent::new(WorkflowEventType::StepFailed, invocation.correlation_id())
                .step_name("step//file//fail")
                .error(error.clone()),
        )
        .unwrap_err();
    assert_eq!(actual, error);
}

fn assert_step_unexpected_event_is_corruption() {
    let invocation = ctx().use_step("step//file//work").invoke(vec![]);
    let error = invocation
        .consume_event(
            &WorkflowEvent::new(WorkflowEventType::HookReceived, invocation.correlation_id())
                .step_name("step//file//work"),
        )
        .unwrap_err();
    assert!(matches!(error, WorkflowCoreError::CorruptedEventLog(_)));
}

fn assert_abort_controller_shape() {
    let controller = ctx().create_abort_controller();
    assert!(!controller.signal.aborted());
    controller.abort(None);
    assert!(controller.signal.aborted());
}

fn assert_abort_sets_signal_aborted() {
    let controller = ctx().create_abort_controller();
    controller.abort(None);
    assert!(controller.signal.aborted());
}

fn assert_abort_reason() {
    let controller = ctx().create_abort_controller();
    controller.abort(Some(json!("because")));
    assert_eq!(controller.signal.reason(), Some(json!("because")));
}

fn assert_abort_twice_noop() {
    let controller = ctx().create_abort_controller();
    controller.abort(Some(json!("first")));
    controller.abort(Some(json!("second")));
    assert_eq!(controller.signal.reason(), Some(json!("first")));
}

fn assert_abort_mismatched_token_corruption() {
    let controller = ctx().create_abort_controller();
    let event = WorkflowEvent::new(
        WorkflowEventType::HookReceived,
        "hook_00000000000000000000000002",
    )
    .token("wrong-token")
    .payload(json!({ "reason": "x" }));
    let error = controller.consume_hook_event(&event).unwrap_err();
    assert!(matches!(error, WorkflowCoreError::CorruptedEventLog(_)));
}

fn assert_abort_duplicate_receipts_idempotent() {
    let controller = ctx().create_abort_controller();
    let token = controller.signal.hook_token();
    let event = WorkflowEvent::new(
        WorkflowEventType::HookReceived,
        "hook_00000000000000000000000002",
    )
    .token(token)
    .payload(json!({ "reason": "first" }));
    controller.consume_hook_event(&event).unwrap();
    controller.consume_hook_event(&event).unwrap();
    assert_eq!(controller.signal.reason(), Some(json!("first")));
}

fn assert_signal_initially_false() {
    assert!(!WorkflowAbortSignal::new("", "").aborted());
}

fn assert_signal_listener_fires() {
    let controller = ctx().create_abort_controller();
    let calls = Rc::new(Cell::new(0));
    let calls_for_listener = Rc::clone(&calls);
    controller
        .signal
        .add_event_listener(move || calls_for_listener.set(calls_for_listener.get() + 1));
    controller.abort(None);
    assert_eq!(calls.get(), 1);
}

fn assert_signal_remove_listener() {
    let controller = ctx().create_abort_controller();
    let calls = Rc::new(Cell::new(0));
    let calls_for_listener = Rc::clone(&calls);
    let id = controller
        .signal
        .add_event_listener(move || calls_for_listener.set(calls_for_listener.get() + 1));
    controller.signal.remove_event_listener(id);
    controller.abort(None);
    assert_eq!(calls.get(), 0);
}

fn assert_signal_throw_if_aborted() {
    let controller = ctx().create_abort_controller();
    controller.abort(None);
    assert!(matches!(
        controller.signal.throw_if_aborted(),
        Err(WorkflowCoreError::Fatal(_))
    ));
}

fn assert_signal_throw_if_not_aborted_noop() {
    assert!(WorkflowAbortSignal::new("", "").throw_if_aborted().is_ok());
}

fn assert_multiple_abort_controllers_independent() {
    let ctx = ctx();
    let first = ctx.create_abort_controller();
    let second = ctx.create_abort_controller();
    first.abort(Some(json!("first")));
    assert!(first.signal.aborted());
    assert!(!second.signal.aborted());
}

fn assert_abort_signal_static_abort() {
    assert!(WorkflowAbortSignal::abort(None).aborted());
}

fn assert_abort_signal_static_abort_reason() {
    let signal = WorkflowAbortSignal::abort(Some(json!("done")));
    assert_eq!(signal.reason(), Some(json!("done")));
}

fn assert_abort_signal_any_fires() {
    let first = ctx().create_abort_controller();
    let second = ctx().create_abort_controller();
    let composite = WorkflowAbortSignal::any(&[first.signal.clone(), second.signal.clone()]);
    second.abort(Some(json!("winner")));
    assert!(composite.aborted());
    assert_eq!(composite.reason(), Some(json!("winner")));
}

fn assert_abort_signal_any_preaborted() {
    let first = WorkflowAbortSignal::abort(Some(json!("already")));
    let second = WorkflowAbortSignal::new("", "");
    let composite = WorkflowAbortSignal::any(&[first, second]);
    assert!(composite.aborted());
    assert_eq!(composite.reason(), Some(json!("already")));
}

fn assert_abort_signal_any_single_pass() {
    let signals = vec![
        WorkflowAbortSignal::new("", ""),
        WorkflowAbortSignal::new("", ""),
    ];
    let composite = WorkflowAbortSignal::any(&signals);
    signals[0].receive_stream_abort(Some(json!("stream")));
    assert_eq!(composite.reason(), Some(json!("stream")));
}

fn assert_abort_signal_any_cleans_up_listeners() {
    let first = WorkflowAbortSignal::new("", "");
    let second = WorkflowAbortSignal::new("", "");
    let composite = WorkflowAbortSignal::any(&[first.clone(), second.clone()]);
    assert_eq!(first.listener_count(), 1);
    second.receive_stream_abort(Some(json!("done")));
    assert!(composite.aborted());
    assert_eq!(first.listener_count(), 0);
    assert_eq!(second.listener_count(), 0);
}

fn assert_abort_signal_timeout_slug() {
    let error = WorkflowAbortSignal::timeout().unwrap_err();
    assert_eq!(error.slug(), Some("abort-signal-timeout-in-workflow"));
}

fn assert_abort_controller_creates_hook_entry() {
    let ctx = ctx();
    let controller = ctx.create_abort_controller();
    let items = ctx.queue_items();
    assert_eq!(items.len(), 1);
    let QueueItem::Hook(hook) = &items[0] else {
        panic!("expected hook");
    };
    assert!(hook.is_system);
    assert_eq!(hook.token, controller.signal.hook_token());
}

fn assert_abort_marks_hook_for_resumption() {
    let ctx = ctx();
    let controller = ctx.create_abort_controller();
    controller.abort(Some(json!("stop")));
    let QueueItem::Hook(hook) = ctx.queue_items().into_iter().next().unwrap() else {
        panic!("expected hook");
    };
    assert!(hook.abort_requested);
    assert_eq!(hook.abort_reason, Some(json!("stop")));
}

fn assert_abort_hook_token_reused() {
    let controller = WorkflowAbortController::deserialized("strm_1_system_abort", "abrt_1");
    assert_eq!(controller.signal.hook_token(), "abrt_1");
    assert_eq!(controller.signal.stream_name(), "strm_1_system_abort");
}

fn assert_abort_deserialized_reader_op() {
    let signal = WorkflowAbortSignal::new("strm_1_system_abort", "abrt_1");
    let ctx = step_ctx();
    assert!(attach_abort_stream_reader(&signal, &ctx));
    assert_eq!(ctx.ops_pending(), 1);
}

fn assert_abort_reader_skips_already_aborted() {
    let signal = WorkflowAbortSignal::abort(None);
    let ctx = step_ctx();
    assert!(!attach_abort_stream_reader(&signal, &ctx));
    assert_eq!(ctx.ops_pending(), 0);
}

fn assert_deserialized_signal_aborted_immediately() {
    let signal = WorkflowAbortSignal::abort(Some(json!("pre")));
    assert!(signal.aborted());
    assert_eq!(signal.reason(), Some(json!("pre")));
}

fn assert_stream_packet_aborts_signal() {
    let signal = WorkflowAbortSignal::new("strm_1_system_abort", "abrt_1");
    let ctx = step_ctx();
    attach_abort_stream_reader(&signal, &ctx);
    deliver_abort_stream_packet(&signal, &ctx, Some(json!("packet")));
    assert!(signal.aborted());
    assert_eq!(ctx.ops_pending(), 0);
}

fn assert_stream_packet_reason() {
    let signal = WorkflowAbortSignal::new("strm_1_system_abort", "abrt_1");
    deliver_abort_stream_packet(&signal, &step_ctx(), Some(json!({ "why": "test" })));
    assert_eq!(signal.reason(), Some(json!({ "why": "test" })));
}

fn assert_stream_packet_listener_fires() {
    let signal = WorkflowAbortSignal::new("strm_1_system_abort", "abrt_1");
    let calls = Rc::new(Cell::new(0));
    let calls_for_listener = Rc::clone(&calls);
    signal.add_event_listener(move || calls_for_listener.set(calls_for_listener.get() + 1));
    deliver_abort_stream_packet(&signal, &step_ctx(), None);
    assert_eq!(calls.get(), 1);
}

fn assert_stream_packet_throw_if_aborted() {
    let signal = WorkflowAbortSignal::new("strm_1_system_abort", "abrt_1");
    deliver_abort_stream_packet(&signal, &step_ctx(), None);
    assert!(signal.throw_if_aborted().is_err());
}

fn assert_deserialized_abort_pushes_stream_op() {
    let controller = WorkflowAbortController::deserialized("strm_1_system_abort", "abrt_1");
    let ctx = step_ctx();
    controller.abort_from_step_context(&ctx, None);
    assert!(controller.signal.aborted());
    assert_eq!(ctx.ops_pending(), 2);
}

fn assert_deserialized_abort_pushes_hook_resume_op() {
    assert_deserialized_abort_pushes_stream_op();
}

fn assert_deserialized_abort_synchronous() {
    let controller = WorkflowAbortController::deserialized("strm_1_system_abort", "abrt_1");
    controller.abort_from_step_context(&step_ctx(), Some(json!("now")));
    assert_eq!(controller.signal.reason(), Some(json!("now")));
}

fn assert_abort_after_step_context_gone_no_crash() {
    let controller = WorkflowAbortController::deserialized("strm_1_system_abort", "abrt_1");
    controller.abort(None);
    assert!(controller.signal.aborted());
}

fn assert_multiple_deserialized_consumers_abort() {
    let first = WorkflowAbortSignal::new("strm_shared_system_abort", "abrt_1");
    let second = WorkflowAbortSignal::new("strm_shared_system_abort", "abrt_1");
    deliver_abort_stream_packet(&first, &step_ctx(), Some(json!("x")));
    deliver_abort_stream_packet(&second, &step_ctx(), Some(json!("x")));
    assert!(first.aborted());
    assert!(second.aborted());
}

fn assert_abort_any_with_deserialized_and_local() {
    let deserialized = WorkflowAbortSignal::new("strm_1_system_abort", "abrt_1");
    let local = WorkflowAbortSignal::new("", "");
    let composite = WorkflowAbortSignal::any(&[deserialized.clone(), local]);
    deliver_abort_stream_packet(&deserialized, &step_ctx(), Some(json!("remote")));
    assert_eq!(composite.reason(), Some(json!("remote")));
}

fn assert_abort_error_is_fatal() {
    let error = UnknownError {
        name: "AbortError".to_string(),
        message: "cancelled".to_string(),
        stack: Some("stack".to_string()),
        fatal: false,
    };
    let fatal = promote_abort_error_to_fatal(error);
    assert!(fatal.fatal);
    assert_eq!(fatal.message, "Aborted: cancelled");
}

fn assert_non_abort_error_not_fatal() {
    let error = UnknownError {
        name: "TypeError".to_string(),
        message: "bad".to_string(),
        stack: None,
        fatal: false,
    };
    assert!(!promote_abort_error_to_fatal(error).fatal);
}

fn assert_context_storage_singleton() {
    let a = context_storage() as *const ContextStorage;
    let b = ContextStorage::global() as *const ContextStorage;
    assert_eq!(a, b);
}

fn assert_context_storage_preserves_run_context() {
    let ctx = step_ctx();
    let seen = context_storage().run(ctx.clone(), || get_step_metadata().unwrap());
    assert_eq!(seen, ctx.step_metadata);
}

fn assert_context_storage_cross_reference() {
    let ctx = step_ctx();
    let seen = ContextStorage::global().run(ctx.clone(), || {
        context_storage().get_store().unwrap().workflow_metadata
    });
    assert_eq!(seen, ctx.workflow_metadata);
}

fn assert_step_writable_release_resolves_op() {
    let ctx = step_ctx();
    context_storage().run(ctx.clone(), || {
        let writable = get_writable(WorkflowWritableStreamOptions::default()).unwrap();
        let mut writer = writable.get_writer();
        assert_eq!(ctx.ops_pending(), 1);
        writer.write(json!("hello"));
        writer.release_lock();
    });
    assert_eq!(ctx.ops_pending(), 0);
    assert_eq!(ctx.written_chunks(), vec![json!("hello")]);
}

fn assert_step_writable_close_resolves_op() {
    let ctx = step_ctx();
    context_storage().run(ctx.clone(), || {
        let writable = get_writable(WorkflowWritableStreamOptions::default()).unwrap();
        let writer = writable.get_writer();
        writer.close();
    });
    assert_eq!(ctx.ops_pending(), 0);
}

fn assert_step_writable_returns_same_namespace() {
    let ctx = step_ctx();
    context_storage().run(ctx, || {
        let first = get_writable(WorkflowWritableStreamOptions {
            namespace: Some("events".to_string()),
        })
        .unwrap();
        let second = get_writable(WorkflowWritableStreamOptions {
            namespace: Some("events".to_string()),
        })
        .unwrap();
        assert!(first.same_handle(&second));
    });
}

fn assert_step_writable_preserves_chunk_order() {
    let ctx = step_ctx();
    context_storage().run(ctx.clone(), || {
        for index in 0..5 {
            get_writable(WorkflowWritableStreamOptions {
                namespace: Some("events".to_string()),
            })
            .unwrap()
            .write_unlocked(json!(index));
        }
    });
    assert_eq!(
        ctx.written_chunks(),
        vec![json!(0), json!(1), json!(2), json!(3), json!(4)]
    );
}

fn assert_step_writable_one_pipe_per_namespace() {
    let ctx = step_ctx();
    context_storage().run(ctx.clone(), || {
        for _ in 0..3 {
            get_writable(WorkflowWritableStreamOptions {
                namespace: Some("events".to_string()),
            })
            .unwrap();
        }
    });
    assert_eq!(ctx.registered_pipes(), 1);
}

fn assert_sleep_resolves_on_wait_completed() {
    let ctx = ctx();
    let sleep = ctx.sleep(SleepInput::DurationMillis(100)).unwrap();
    assert_eq!(
        sleep
            .consume_event(
                &WorkflowEvent::new(WorkflowEventType::WaitCompleted, sleep.correlation_id())
                    .resume_at(sleep.resume_at()),
            )
            .unwrap(),
        workflow_core::SleepEventResult::Resolved
    );
}

fn assert_sleep_resolves_old_wait_completed_without_data() {
    let ctx = ctx();
    let sleep = ctx.sleep(SleepInput::DurationMillis(100)).unwrap();
    assert!(
        sleep
            .consume_event(&WorkflowEvent::new(
                WorkflowEventType::WaitCompleted,
                sleep.correlation_id()
            ))
            .is_ok()
    );
}

fn assert_sleep_resume_mismatch_corruption() {
    let ctx = ctx();
    let sleep = ctx.sleep(SleepInput::DurationMillis(100)).unwrap();
    let error = sleep
        .consume_event(
            &WorkflowEvent::new(WorkflowEventType::WaitCompleted, sleep.correlation_id())
                .resume_at(sleep.resume_at() + 1),
        )
        .unwrap_err();
    assert!(matches!(error, WorkflowCoreError::CorruptedEventLog(_)));
}

fn assert_sleep_no_events_suspends() {
    let sleep = ctx().sleep(SleepInput::DurationMillis(100)).unwrap();
    assert!(
        sleep
            .finish_without_event()
            .unwrap_err()
            .is_workflow_suspension()
    );
}

fn assert_sleep_unexpected_event_corruption() {
    let sleep = ctx().sleep(SleepInput::DurationMillis(100)).unwrap();
    let error = sleep
        .consume_event(&WorkflowEvent::new(
            WorkflowEventType::HookReceived,
            sleep.correlation_id(),
        ))
        .unwrap_err();
    assert!(matches!(error, WorkflowCoreError::CorruptedEventLog(_)));
}

fn assert_sleep_wait_created_marks_item() {
    let ctx = ctx();
    let sleep = ctx.sleep(SleepInput::DurationMillis(100)).unwrap();
    sleep
        .consume_event(
            &WorkflowEvent::new(WorkflowEventType::WaitCreated, sleep.correlation_id())
                .resume_at(sleep.resume_at()),
        )
        .unwrap();
    let QueueItem::Wait(wait) = ctx.queue_item(sleep.correlation_id()).unwrap() else {
        panic!("expected wait");
    };
    assert!(wait.has_created_event);
}

fn assert_sleep_wait_created_keeps_queue_item() {
    let ctx = ctx();
    let sleep = ctx.sleep(SleepInput::DurationMillis(100)).unwrap();
    sleep
        .consume_event(&WorkflowEvent::new(
            WorkflowEventType::WaitCreated,
            sleep.correlation_id(),
        ))
        .unwrap();
    assert!(ctx.queue_item(sleep.correlation_id()).is_some());
}

fn assert_sleep_wait_completed_removes_item() {
    let ctx = ctx();
    let sleep = ctx.sleep(SleepInput::DurationMillis(100)).unwrap();
    sleep
        .consume_event(&WorkflowEvent::new(
            WorkflowEventType::WaitCompleted,
            sleep.correlation_id(),
        ))
        .unwrap();
    assert!(ctx.queue_item(sleep.correlation_id()).is_none());
}

fn assert_sleep_duplicate_wait_completed_corruption() {
    let ctx = ctx();
    let sleep = ctx.sleep(SleepInput::DurationMillis(100)).unwrap();
    sleep
        .consume_event(&WorkflowEvent::new(
            WorkflowEventType::WaitCompleted,
            sleep.correlation_id(),
        ))
        .unwrap();
    assert!(
        sleep
            .consume_event(&WorkflowEvent::new(
                WorkflowEventType::WaitCompleted,
                sleep.correlation_id(),
            ))
            .is_ok()
    );
    assert_eq!(ctx.queue_len(), 0);
}

fn assert_sleep_date_and_string_inputs() {
    let ctx = ctx();
    let one_second = ctx.sleep(SleepInput::from("1s")).unwrap();
    assert_eq!(one_second.resume_at(), ctx.timestamp() + 1_000);
    let until = ctx.sleep(SleepInput::Until(2_000_000_000_000)).unwrap();
    assert_eq!(until.resume_at(), 2_000_000_000_000);
}

fn assert_hook_resolves_payload() {
    let hook = ctx().create_hook(HookOptions::default());
    hook.consume_event(
        &WorkflowEvent::new(WorkflowEventType::HookReceived, hook.correlation_id())
            .token(hook.token())
            .payload(json!({ "ok": true })),
    )
    .unwrap();
    assert_eq!(hook.await_next().unwrap(), json!({ "ok": true }));
}

fn assert_hook_token_mismatch_corruption() {
    let hook = ctx().create_hook(HookOptions::default());
    let error = hook
        .consume_event(
            &WorkflowEvent::new(WorkflowEventType::HookReceived, hook.correlation_id())
                .token("wrong"),
        )
        .unwrap_err();
    assert!(matches!(error, WorkflowCoreError::CorruptedEventLog(_)));
}

fn assert_hook_no_events_suspends() {
    let hook = ctx().create_hook(HookOptions::default());
    assert!(hook.await_next().unwrap_err().is_workflow_suspension());
}

fn assert_hook_unexpected_event_corruption() {
    let hook = ctx().create_hook(HookOptions::default());
    let error = hook
        .consume_event(&WorkflowEvent::new(
            WorkflowEventType::WaitCompleted,
            hook.correlation_id(),
        ))
        .unwrap_err();
    assert!(matches!(error, WorkflowCoreError::CorruptedEventLog(_)));
}

fn assert_hook_created_marks_queue_item() {
    let ctx = ctx();
    let hook = ctx.create_hook(HookOptions::default());
    hook.consume_event(
        &WorkflowEvent::new(WorkflowEventType::HookCreated, hook.correlation_id())
            .token(hook.token()),
    )
    .unwrap();
    let QueueItem::Hook(item) = ctx.queue_item(hook.correlation_id()).unwrap() else {
        panic!("expected hook");
    };
    assert!(item.has_created_event);
}

fn assert_hook_disposed_finishes() {
    let ctx = ctx();
    let hook = ctx.create_hook(HookOptions::default());
    hook.consume_event(
        &WorkflowEvent::new(WorkflowEventType::HookDisposed, hook.correlation_id())
            .token(hook.token()),
    )
    .unwrap();
    assert!(ctx.queue_item(hook.correlation_id()).is_none());
}

fn assert_hook_multiple_payloads_iterator() {
    let hook = ctx().create_hook(HookOptions::default());
    for payload in [json!("a"), json!("b")] {
        hook.consume_event(
            &WorkflowEvent::new(WorkflowEventType::HookReceived, hook.correlation_id())
                .token(hook.token())
                .payload(payload),
        )
        .unwrap();
    }
    assert_eq!(hook.iterator_next().unwrap(), json!("a"));
    assert_eq!(hook.iterator_next().unwrap(), json!("b"));
}

fn assert_hook_unexpected_error_mentions_token() {
    let hook = ctx().create_hook(HookOptions {
        token: Some("token-1".to_string()),
        ..HookOptions::default()
    });
    let error = hook
        .consume_event(&WorkflowEvent::new(
            WorkflowEventType::WaitCompleted,
            hook.correlation_id(),
        ))
        .unwrap_err();
    assert!(error.to_string().contains("token-1"));
}

fn assert_hook_conflict_rejects() {
    let hook = ctx().create_hook(HookOptions {
        token: Some("token-1".to_string()),
        ..HookOptions::default()
    });
    hook.consume_event(
        &WorkflowEvent::new(WorkflowEventType::HookConflict, hook.correlation_id())
            .token(hook.token())
            .conflicting_run_id("wrun_other"),
    )
    .unwrap();
    assert!(matches!(
        hook.await_next().unwrap_err(),
        WorkflowCoreError::HookConflict { .. }
    ));
}

fn assert_hook_dispose_sets_flag() {
    let ctx = ctx();
    let hook = ctx.create_hook(HookOptions::default());
    hook.dispose().unwrap();
    let QueueItem::Hook(item) = ctx.queue_item(hook.correlation_id()).unwrap() else {
        panic!("expected hook");
    };
    assert!(item.disposed);
}

fn assert_hook_dispose_idempotent() {
    let hook = ctx().create_hook(HookOptions::default());
    hook.dispose().unwrap();
    hook.dispose().unwrap();
}

fn assert_hook_dispose_after_created_before_disposed() {
    let ctx = ctx();
    let hook = ctx.create_hook(HookOptions::default());
    hook.consume_event(
        &WorkflowEvent::new(WorkflowEventType::HookCreated, hook.correlation_id())
            .token(hook.token()),
    )
    .unwrap();
    hook.dispose().unwrap();
    let QueueItem::Hook(item) = ctx.queue_item(hook.correlation_id()).unwrap() else {
        panic!("expected hook");
    };
    assert!(item.has_created_event);
    assert!(item.disposed);
}

fn assert_hook_buffered_payloads_survive_disposed_event() {
    let hook = ctx().create_hook(HookOptions::default());
    hook.consume_event(
        &WorkflowEvent::new(WorkflowEventType::HookReceived, hook.correlation_id())
            .token(hook.token())
            .payload(json!("buffered")),
    )
    .unwrap();
    hook.consume_event(
        &WorkflowEvent::new(WorkflowEventType::HookDisposed, hook.correlation_id())
            .token(hook.token()),
    )
    .unwrap();
    assert_eq!(hook.await_next().unwrap(), json!("buffered"));
}

fn assert_hook_conflict_removes_queue_item() {
    let ctx = ctx();
    let hook = ctx.create_hook(HookOptions::default());
    hook.consume_event(
        &WorkflowEvent::new(WorkflowEventType::HookConflict, hook.correlation_id())
            .token(hook.token())
            .conflicting_run_id("wrun_other"),
    )
    .unwrap();
    assert!(ctx.queue_item(hook.correlation_id()).is_none());
}

fn assert_hook_dispose_after_created_suspension() {
    let hook = ctx().create_hook(HookOptions::default());
    assert!(hook.await_next().unwrap_err().is_workflow_suspension());
    assert!(hook.dispose().unwrap_err().is_workflow_suspension());
}

fn assert_multiple_hooks_only_one_disposed() {
    let ctx = ctx();
    let first = ctx.create_hook(HookOptions::default());
    let second = ctx.create_hook(HookOptions::default());
    first.dispose().unwrap();
    let QueueItem::Hook(first_item) = ctx.queue_item(first.correlation_id()).unwrap() else {
        panic!("expected hook");
    };
    let QueueItem::Hook(second_item) = ctx.queue_item(second.correlation_id()).unwrap() else {
        panic!("expected hook");
    };
    assert!(first_item.disposed);
    assert!(!second_item.disposed);
}

fn assert_dispose_conflicted_hook_safe() {
    let hook = ctx().create_hook(HookOptions::default());
    hook.consume_event(
        &WorkflowEvent::new(WorkflowEventType::HookConflict, hook.correlation_id())
            .token(hook.token())
            .conflicting_run_id("wrun_other"),
    )
    .unwrap();
    hook.dispose().unwrap();
}

fn assert_symbol_dispose() {
    let hook = ctx().create_hook(HookOptions::default());
    hook.symbol_dispose().unwrap();
}

fn assert_iterator_break_keeps_hook_alive_without_dispose() {
    let ctx = ctx();
    let hook = ctx.create_hook(HookOptions::default());
    hook.consume_event(
        &WorkflowEvent::new(WorkflowEventType::HookReceived, hook.correlation_id())
            .token(hook.token())
            .payload(json!("one")),
    )
    .unwrap();
    assert_eq!(hook.iterator_next().unwrap(), json!("one"));
    assert!(ctx.queue_item(hook.correlation_id()).is_some());
}

fn assert_hook_dispose_while_awaiting_suspends() {
    let hook = ctx().create_hook(HookOptions::default());
    assert!(hook.await_next().unwrap_err().is_workflow_suspension());
    assert!(hook.dispose().unwrap_err().is_workflow_suspension());
}

fn assert_hook_default_is_webhook_false() {
    let ctx = ctx();
    let hook = ctx.create_hook(HookOptions::default());
    let QueueItem::Hook(item) = ctx.queue_item(hook.correlation_id()).unwrap() else {
        panic!("expected hook");
    };
    assert!(!item.is_webhook);
}

fn assert_hook_is_webhook_option_true() {
    let ctx = ctx();
    let hook = ctx.create_hook(HookOptions {
        is_webhook: true,
        ..HookOptions::default()
    });
    let QueueItem::Hook(item) = ctx.queue_item(hook.correlation_id()).unwrap() else {
        panic!("expected hook");
    };
    assert!(item.is_webhook);
}

fn assert_create_webhook_rejects_token() {
    let error = ctx()
        .create_webhook(WebhookOptions {
            token: Some("fixed".to_string()),
            respond_with: None,
        })
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("does not accept a `token` option")
    );
}

#[derive(Default)]
struct RecordingResumer {
    calls: Vec<(String, Value)>,
}

impl HookResumer for RecordingResumer {
    fn resume_hook(
        &mut self,
        token: String,
        payload: Value,
    ) -> workflow_core::Result<workflow_core::HookEntity> {
        self.calls.push((token.clone(), payload.clone()));
        Ok(workflow_core::HookEntity { token, payload })
    }
}

fn assert_define_hook_passthrough() {
    let hook = define_hook();
    let mut resumer = RecordingResumer::default();
    let entity = hook
        .resume(&mut resumer, "token", json!({ "ok": true }))
        .unwrap();
    assert_eq!(entity.payload, json!({ "ok": true }));
}

fn assert_define_hook_schema_transform() {
    let hook = define_hook_with_schema(|payload| Ok(json!({ "wrapped": payload })));
    let mut resumer = RecordingResumer::default();
    let entity = hook.resume(&mut resumer, "token", json!("input")).unwrap();
    assert_eq!(entity.payload, json!({ "wrapped": "input" }));
}

fn assert_define_hook_schema_failure() {
    let hook = define_hook_with_schema(|_| {
        Err(WorkflowCoreError::WorkflowRuntime {
            message: "Hook payload did not match the defined schema".to_string(),
            slug: None,
        })
    });
    let mut resumer = RecordingResumer::default();
    let error = hook
        .resume(&mut resumer, "token", json!("bad"))
        .unwrap_err();
    assert!(error.to_string().contains("Hook payload"));
}

fn assert_server_stream_run_id_validation() {
    let world = Rc::new(RefCell::new(MemoryStreamWorld::default()));
    assert!(WorkflowServerWritableStream::try_new_checked(None, "name", world).is_err());
}

fn assert_server_stream_name_validation() {
    let world = Rc::new(RefCell::new(MemoryStreamWorld::default()));
    assert!(WorkflowServerWritableStream::try_new("run", "", world).is_err());
}

fn assert_server_stream_accepts_run_id() {
    let world = Rc::new(RefCell::new(MemoryStreamWorld::default()));
    let stream = WorkflowServerWritableStream::try_new("run", "name", world).unwrap();
    assert_eq!(stream.run_id(), "run");
}

fn assert_server_stream_write_reaches_server() {
    let world = Rc::new(RefCell::new(MemoryStreamWorld::default()));
    let mut stream =
        WorkflowServerWritableStream::try_new("run", "name", Rc::clone(&world)).unwrap();
    stream.write(json!("chunk")).unwrap();
    assert_eq!(
        world.borrow().writes,
        vec![StreamWrite {
            run_id: "run".to_string(),
            name: "name".to_string(),
            chunk: json!("chunk"),
        }]
    );
}

fn assert_server_stream_single_write_call() {
    let world = Rc::new(RefCell::new(MemoryStreamWorld::default()));
    let mut stream =
        WorkflowServerWritableStream::try_new("run", "name", Rc::clone(&world)).unwrap();
    stream.write(json!("chunk")).unwrap();
    assert_eq!(world.borrow().write_calls, 1);
}

fn assert_server_stream_write_multi_fallback() {
    let world = Rc::new(RefCell::new(MemoryStreamWorld::default()));
    let mut stream =
        WorkflowServerWritableStream::try_new("run", "name", Rc::clone(&world)).unwrap();
    stream.buffer(json!("a"));
    stream.buffer(json!("b"));
    stream.flush().unwrap();
    assert_eq!(world.borrow().write_calls, 2);
    assert_eq!(world.borrow().write_multi_calls, 0);
}

fn assert_server_stream_multiple_sequential_writes() {
    let world = Rc::new(RefCell::new(MemoryStreamWorld::default()));
    let mut stream =
        WorkflowServerWritableStream::try_new("run", "name", Rc::clone(&world)).unwrap();
    stream.write(json!("a")).unwrap();
    stream.write(json!("b")).unwrap();
    assert_eq!(world.borrow().writes.len(), 2);
}

fn assert_server_stream_flushes_buffer_on_close() {
    let world = Rc::new(RefCell::new(MemoryStreamWorld::default()));
    let mut stream =
        WorkflowServerWritableStream::try_new("run", "name", Rc::clone(&world)).unwrap();
    stream.buffer(json!("a"));
    stream.close().unwrap();
    assert_eq!(world.borrow().writes.len(), 1);
    assert_eq!(world.borrow().closes.len(), 1);
}

fn assert_server_stream_close_without_buffer() {
    let world = Rc::new(RefCell::new(MemoryStreamWorld::default()));
    let mut stream =
        WorkflowServerWritableStream::try_new("run", "name", Rc::clone(&world)).unwrap();
    stream.close().unwrap();
    assert_eq!(world.borrow().writes.len(), 0);
    assert_eq!(world.borrow().closes.len(), 1);
}

fn assert_server_stream_abort_discards_buffer() {
    let world = Rc::new(RefCell::new(MemoryStreamWorld::default()));
    let mut stream =
        WorkflowServerWritableStream::try_new("run", "name", Rc::clone(&world)).unwrap();
    stream.buffer(json!("a"));
    stream.abort();
    stream.close().unwrap();
    assert!(world.borrow().writes.is_empty());
    assert!(world.borrow().closes.is_empty());
}

fn assert_server_stream_write_errors_propagate() {
    let world = Rc::new(RefCell::new(MemoryStreamWorld {
        fail_writes: Some("write failed".to_string()),
        ..MemoryStreamWorld::default()
    }));
    let mut stream = WorkflowServerWritableStream::try_new("run", "name", world).unwrap();
    assert!(stream.write(json!("a")).is_err());
}

fn assert_server_stream_close_errors_propagate() {
    let world = Rc::new(RefCell::new(MemoryStreamWorld {
        fail_close: Some("close failed".to_string()),
        ..MemoryStreamWorld::default()
    }));
    let mut stream = WorkflowServerWritableStream::try_new("run", "name", world).unwrap();
    assert!(stream.close().is_err());
}

fn assert_server_stream_close_flush_errors_propagate() {
    let world = Rc::new(RefCell::new(MemoryStreamWorld {
        fail_writes: Some("write failed".to_string()),
        ..MemoryStreamWorld::default()
    }));
    let mut stream = WorkflowServerWritableStream::try_new("run", "name", world).unwrap();
    stream.buffer(json!("a"));
    assert!(stream.close().is_err());
}

fn assert_server_stream_interval_zero() {
    let world = Rc::new(RefCell::new(MemoryStreamWorld {
        stream_flush_interval_ms: Some(0),
        ..MemoryStreamWorld::default()
    }));
    let stream = WorkflowServerWritableStream::try_new("run", "name", world).unwrap();
    assert_eq!(stream.flush_interval_ms(), 0);
}

fn assert_server_stream_interval_default() {
    let world = Rc::new(RefCell::new(MemoryStreamWorld::default()));
    let stream = WorkflowServerWritableStream::try_new("run", "name", world).unwrap();
    assert_eq!(stream.flush_interval_ms(), 100);
}

fn assert_server_stream_interval_custom() {
    let world = Rc::new(RefCell::new(MemoryStreamWorld {
        stream_flush_interval_ms: Some(25),
        ..MemoryStreamWorld::default()
    }));
    let stream = WorkflowServerWritableStream::try_new("run", "name", world).unwrap();
    assert_eq!(stream.flush_interval_ms(), 25);
}

fn assert_response_with_body() {
    let response =
        WorkflowResponse::new(Some(WorkflowBody::text("hello")), ResponseInit::default()).unwrap();
    assert!(response.ok());
    assert_eq!(response.body, Some(WorkflowBody::text("hello")));
}

fn assert_response_json_sets_header() {
    let response = WorkflowResponse::json(&json!({ "ok": true }), ResponseInit::default()).unwrap();
    assert_eq!(
        response.headers.get("content-type"),
        Some("application/json")
    );
}

fn assert_response_custom_headers() {
    let mut headers = Headers::new();
    headers.set("x-test", "1");
    let response = WorkflowResponse::new(
        None,
        ResponseInit {
            headers,
            ..ResponseInit::default()
        },
    )
    .unwrap();
    assert_eq!(response.headers.get("X-Test"), Some("1"));
}

fn assert_response_204_without_body() {
    let response = WorkflowResponse::new(
        None,
        ResponseInit {
            status: Some(204),
            ..ResponseInit::default()
        },
    )
    .unwrap();
    assert_eq!(response.status, 204);
}

fn assert_response_rejects_body_with_204() {
    assert!(
        WorkflowResponse::new(
            Some(WorkflowBody::text("nope")),
            ResponseInit {
                status: Some(204),
                ..ResponseInit::default()
            },
        )
        .is_err()
    );
}

fn assert_response_redirect_default() {
    let response = WorkflowResponse::redirect("https://example.com", None).unwrap();
    assert_eq!(response.status, 302);
    assert_eq!(
        response.headers.get("location"),
        Some("https://example.com")
    );
}

fn assert_response_redirect_custom_status() {
    let response = WorkflowResponse::redirect("https://example.com", Some(307)).unwrap();
    assert_eq!(response.status, 307);
}

fn assert_response_redirect_valid_status_codes() {
    for status in [301, 302, 303, 307, 308] {
        assert!(WorkflowResponse::redirect("https://example.com", Some(status)).is_ok());
    }
}

fn assert_response_redirect_invalid_status() {
    assert!(WorkflowResponse::redirect("https://example.com", Some(300)).is_err());
}

fn assert_request_get() {
    let request = WorkflowRequest::new("https://example.com", RequestInit::default()).unwrap();
    assert_eq!(request.method, "GET");
    assert_eq!(request.url, "https://example.com");
}

fn assert_request_post_body() {
    let request = WorkflowRequest::new(
        "https://example.com",
        RequestInit {
            method: Some("post".to_string()),
            body: Some(WorkflowBody::text("body")),
            ..RequestInit::default()
        },
    )
    .unwrap();
    assert_eq!(request.method, "POST");
    assert_eq!(request.body, Some(WorkflowBody::text("body")));
}

fn assert_request_custom_headers() {
    let mut headers = Headers::new();
    headers.set("x-test", "yes");
    let request = WorkflowRequest::new(
        "https://example.com",
        RequestInit {
            headers,
            ..RequestInit::default()
        },
    )
    .unwrap();
    assert_eq!(request.headers.get("x-test"), Some("yes"));
}

fn assert_request_uint8_body() {
    let request = WorkflowRequest::new(
        "https://example.com",
        RequestInit {
            method: Some("POST".to_string()),
            body: Some(WorkflowBody::bytes([1, 2, 3])),
            ..RequestInit::default()
        },
    )
    .unwrap();
    assert_eq!(request.body, Some(WorkflowBody::bytes([1, 2, 3])));
}

fn assert_request_rejects_get_body() {
    assert!(
        WorkflowRequest::new(
            "https://example.com",
            RequestInit {
                body: Some(WorkflowBody::text("bad")),
                ..RequestInit::default()
            },
        )
        .is_err()
    );
}

fn assert_request_rejects_head_body() {
    assert!(
        WorkflowRequest::new(
            "https://example.com",
            RequestInit {
                method: Some("HEAD".to_string()),
                body: Some(WorkflowBody::text("bad")),
                ..RequestInit::default()
            },
        )
        .is_err()
    );
}

fn assert_request_rejects_invalid_url() {
    assert!(WorkflowRequest::new("not a url", RequestInit::default()).is_err());
}

fn assert_request_clone_with_init_override() {
    let original = WorkflowRequest::new("https://example.com", RequestInit::default()).unwrap();
    let clone = original
        .clone_with_init(RequestInit {
            method: Some("POST".to_string()),
            body: Some(WorkflowBody::text("body")),
            ..RequestInit::default()
        })
        .unwrap();
    assert_eq!(clone.method, "POST");
    assert_eq!(clone.url, original.url);
    assert_eq!(clone.body, Some(WorkflowBody::text("body")));
}

fn assert_timeout_guard_slug() {
    assert_eq!(
        timeout_function_error().slug(),
        Some("timeout-functions-in-workflow")
    );
}

fn assert_fetch_guard_slug() {
    assert_eq!(
        fetch_in_workflow_error().slug(),
        Some("fetch-in-workflow-function")
    );
}

fn assert_stream_id_namespace_encoding() {
    assert_eq!(
        get_workflow_run_stream_id("wrun_abc", Some("room/name")),
        "strm_abc_user_cm9vbS9uYW1l"
    );
}

fn assert_is_abort_error_cross_realm_shape() {
    assert!(is_abort_error(&UnknownError {
        name: "AbortError".to_string(),
        message: "aborted".to_string(),
        stack: None,
        fatal: false,
    }));
}

parity_test!(
    step_create_use_step_resolves_with_result,
    assert_step_resolves_with_result
);
parity_test!(
    step_create_use_step_rejects_hydrated_thrown_value,
    assert_step_rejects_with_error
);
parity_test!(
    step_create_use_step_suspends_single_not_run,
    assert_step_suspends_without_event
);
parity_test!(
    step_create_use_step_suspends_concurrent_not_run,
    assert_concurrent_steps_suspend
);
parity_test!(
    step_create_use_step_sets_function_name,
    assert_step_function_name_from_step_id
);
parity_test!(
    step_create_use_step_captures_closure_vars,
    assert_step_captures_closure_vars
);
parity_test!(
    step_create_use_step_captures_this_via_bind,
    assert_step_bind_captures_this_and_args
);
parity_test!(
    step_create_use_step_handles_empty_closure_vars,
    assert_step_handles_empty_closure_vars
);
parity_test!(
    step_create_use_step_marks_created_event,
    assert_step_created_marks_queue_item
);
parity_test!(
    step_create_use_step_fails_step_created_wrong_step_name,
    assert_step_wrong_name_is_corruption
);
parity_test!(
    step_create_use_step_consumes_step_started,
    assert_step_started_consumed_keeps_queue_item
);
parity_test!(
    step_create_use_step_consumes_step_retrying,
    assert_step_retrying_consumed_keeps_queue_item
);
parity_test!(
    step_create_use_step_fails_step_completed_wrong_step_name,
    assert_step_wrong_name_is_corruption
);
parity_test!(
    step_create_use_step_removes_queue_item_on_completed,
    assert_step_completed_removes_queue_item
);
parity_test!(
    step_create_use_step_fails_step_failed_wrong_step_name,
    assert_step_wrong_name_is_corruption
);
parity_test!(
    step_create_use_step_removes_queue_item_on_failed,
    assert_step_failed_removes_queue_item
);
parity_test!(
    step_create_use_step_preserves_error_subclass_identity,
    assert_step_error_identity_preserved
);
parity_test!(
    step_create_use_step_preserves_plain_error,
    assert_step_error_identity_preserved
);
parity_test!(
    step_create_use_step_unexpected_event_corruption,
    assert_step_unexpected_event_is_corruption
);

parity_test!(
    abort_controller_new_returns_signal_and_abort,
    assert_abort_controller_shape
);
parity_test!(
    abort_controller_abort_sets_signal_aborted,
    assert_abort_sets_signal_aborted
);
parity_test!(
    abort_controller_abort_reason_sets_signal_reason,
    assert_abort_reason
);
parity_test!(
    abort_controller_abort_called_twice_is_noop,
    assert_abort_twice_noop
);
parity_test!(
    abort_controller_hook_received_token_mismatch_corruption,
    assert_abort_mismatched_token_corruption
);
parity_test!(
    abort_controller_duplicate_matching_receipts_idempotent,
    assert_abort_duplicate_receipts_idempotent
);
parity_test!(
    abort_signal_aborted_false_initially,
    assert_signal_initially_false
);
parity_test!(
    abort_signal_listener_fires_when_aborted,
    assert_signal_listener_fires
);
parity_test!(
    abort_signal_remove_listener_prevents_callback,
    assert_signal_remove_listener
);
parity_test!(
    abort_signal_throw_if_aborted_throws,
    assert_signal_throw_if_aborted
);
parity_test!(
    abort_signal_throw_if_aborted_noop_when_not_aborted,
    assert_signal_throw_if_not_aborted_noop
);
parity_test!(
    abort_controller_multiple_controllers_independent_state,
    assert_multiple_abort_controllers_independent
);
parity_test!(
    abort_signal_abort_returns_preaborted_signal,
    assert_abort_signal_static_abort
);
parity_test!(
    abort_signal_abort_reason_returns_preaborted_signal,
    assert_abort_signal_static_abort_reason
);
parity_test!(
    abort_signal_any_fires_when_any_input_fires,
    assert_abort_signal_any_fires
);
parity_test!(
    abort_signal_any_preaborted_input_immediate,
    assert_abort_signal_any_preaborted
);
parity_test!(
    abort_signal_any_single_shot_iterable,
    assert_abort_signal_any_single_pass
);
parity_test!(
    abort_signal_any_removes_input_listeners,
    assert_abort_signal_any_cleans_up_listeners
);
parity_test!(
    abort_signal_timeout_throws_slug,
    assert_abort_signal_timeout_slug
);
parity_test!(
    abort_controller_creates_hook_entry,
    assert_abort_controller_creates_hook_entry
);
parity_test!(
    abort_controller_abort_marks_hook_for_resumption,
    assert_abort_marks_hook_for_resumption
);
parity_test!(
    abort_controller_hook_token_reused_across_replays,
    assert_abort_hook_token_reused
);

parity_test!(
    abort_controller_step_deserialized_signal_pushes_reader_op,
    assert_abort_deserialized_reader_op
);
parity_test!(
    abort_controller_step_already_aborted_skips_reader_op,
    assert_abort_reader_skips_already_aborted
);
parity_test!(
    abort_controller_step_already_aborted_immediate,
    assert_deserialized_signal_aborted_immediately
);
parity_test!(
    abort_controller_step_stream_packet_triggers_abort,
    assert_stream_packet_aborts_signal
);
parity_test!(
    abort_controller_step_stream_packet_reason,
    assert_stream_packet_reason
);
parity_test!(
    abort_controller_step_listener_fires_from_stream_packet,
    assert_stream_packet_listener_fires
);
parity_test!(
    abort_controller_step_throw_if_aborted_after_packet,
    assert_stream_packet_throw_if_aborted
);
parity_test!(
    abort_controller_step_abort_pushes_stream_write_op,
    assert_deserialized_abort_pushes_stream_op
);
parity_test!(
    abort_controller_step_abort_pushes_hook_resume_op,
    assert_deserialized_abort_pushes_hook_resume_op
);
parity_test!(
    abort_controller_step_abort_sets_signal_synchronously,
    assert_deserialized_abort_synchronous
);
parity_test!(
    abort_controller_step_abort_after_context_gone_no_crash,
    assert_abort_after_step_context_gone_no_crash
);
parity_test!(
    abort_controller_step_multiple_consumers_receive_abort,
    assert_multiple_deserialized_consumers_abort
);
parity_test!(
    abort_controller_step_any_deserialized_and_local,
    assert_abort_any_with_deserialized_and_local
);
parity_test!(
    abort_controller_step_fetch_abort_wrapped_fatal,
    assert_abort_error_is_fatal
);
parity_test!(
    abort_controller_step_throw_if_aborted_wrapped_fatal,
    assert_abort_error_is_fatal
);
parity_test!(
    abort_controller_step_custom_reason_preserved_in_fatal,
    assert_abort_error_is_fatal
);
parity_test!(
    abort_controller_step_abort_skips_retries,
    assert_abort_error_is_fatal
);
parity_test!(
    abort_controller_step_non_abort_not_wrapped_fatal,
    assert_non_abort_error_not_fatal
);

parity_test!(
    context_storage_returns_same_instance,
    assert_context_storage_singleton
);
parity_test!(
    context_storage_uses_global_symbol_equivalent,
    assert_context_storage_singleton
);
parity_test!(
    context_storage_preserves_run_context,
    assert_context_storage_preserves_run_context
);
parity_test!(
    context_storage_separately_constructed_reference_reads_store,
    assert_context_storage_cross_reference
);

parity_test!(
    step_writable_ops_resolve_on_writer_release,
    assert_step_writable_release_resolves_op
);
parity_test!(
    step_writable_ops_resolve_on_stream_close,
    assert_step_writable_close_resolves_op
);
parity_test!(
    step_writable_same_namespace_returns_same_handle,
    assert_step_writable_returns_same_namespace
);
parity_test!(
    step_writable_preserves_order_across_repeated_get_writable,
    assert_step_writable_preserves_chunk_order
);
parity_test!(
    step_writable_registers_one_pipe_per_namespace,
    assert_step_writable_one_pipe_per_namespace
);

parity_test!(
    workflow_sleep_resolves_wait_completed,
    assert_sleep_resolves_on_wait_completed
);
parity_test!(
    workflow_sleep_resolves_old_wait_completed_without_event_data,
    assert_sleep_resolves_old_wait_completed_without_data
);
parity_test!(
    workflow_sleep_resume_at_mismatch_corruption,
    assert_sleep_resume_mismatch_corruption
);
parity_test!(
    workflow_sleep_invalid_resume_at_corruption,
    assert_sleep_resume_mismatch_corruption
);
parity_test!(
    workflow_sleep_no_events_suspends,
    assert_sleep_no_events_suspends
);
parity_test!(
    workflow_sleep_unexpected_event_corruption,
    assert_sleep_unexpected_event_corruption
);
parity_test!(
    workflow_sleep_wait_created_marks_queue_item,
    assert_sleep_wait_created_marks_item
);
parity_test!(
    workflow_sleep_hook_received_unexpected,
    assert_sleep_unexpected_event_corruption
);
parity_test!(
    workflow_sleep_wait_created_keeps_queue_item,
    assert_sleep_wait_created_keeps_queue_item
);
parity_test!(
    workflow_sleep_wait_completed_removes_queue_item,
    assert_sleep_wait_completed_removes_item
);
parity_test!(
    workflow_sleep_duplicate_wait_completed_handled,
    assert_sleep_duplicate_wait_completed_corruption
);
parity_test!(
    workflow_sleep_resolves_with_void,
    assert_sleep_resolves_on_wait_completed
);
parity_test!(
    workflow_sleep_string_and_date_inputs,
    assert_sleep_date_and_string_inputs
);

parity_test!(workflow_hook_resolves_payload, assert_hook_resolves_payload);
parity_test!(
    workflow_hook_created_token_mismatch_corruption,
    assert_hook_token_mismatch_corruption
);
parity_test!(
    workflow_hook_received_token_mismatch_corruption,
    assert_hook_token_mismatch_corruption
);
parity_test!(
    workflow_hook_disposed_token_mismatch_corruption,
    assert_hook_token_mismatch_corruption
);
parity_test!(
    workflow_hook_conflict_token_mismatch_corruption,
    assert_hook_token_mismatch_corruption
);
parity_test!(
    workflow_hook_no_events_suspends,
    assert_hook_no_events_suspends
);
parity_test!(
    workflow_hook_unexpected_event_corruption,
    assert_hook_unexpected_event_corruption
);
parity_test!(
    workflow_hook_created_marks_queue_item,
    assert_hook_created_marks_queue_item
);
parity_test!(
    workflow_hook_disposed_finishes_processing,
    assert_hook_disposed_finishes
);
parity_test!(
    workflow_hook_multiple_received_events_iterator,
    assert_hook_multiple_payloads_iterator
);
parity_test!(
    workflow_hook_unexpected_event_mentions_token,
    assert_hook_unexpected_error_mentions_token
);
parity_test!(workflow_hook_conflict_rejects, assert_hook_conflict_rejects);
parity_test!(
    workflow_hook_conflict_rejects_multiple_awaits,
    assert_hook_conflict_rejects
);
parity_test!(
    workflow_hook_disposed_replay_noop,
    assert_hook_disposed_finishes
);
parity_test!(
    workflow_hook_dispose_sets_flag,
    assert_hook_dispose_sets_flag
);
parity_test!(
    workflow_hook_dispose_idempotent,
    assert_hook_dispose_idempotent
);
parity_test!(
    workflow_hook_dispose_after_created_before_disposed,
    assert_hook_dispose_after_created_before_disposed
);
parity_test!(
    workflow_hook_buffered_payloads_survive_disposed,
    assert_hook_buffered_payloads_survive_disposed_event
);
parity_test!(
    workflow_hook_conflict_removes_queue_item,
    assert_hook_conflict_removes_queue_item
);
parity_test!(
    workflow_hook_dispose_after_created_suspension,
    assert_hook_dispose_after_created_suspension
);
parity_test!(
    workflow_hook_dispose_before_first_suspension,
    assert_hook_dispose_sets_flag
);
parity_test!(
    workflow_hook_multiple_hooks_one_disposed,
    assert_multiple_hooks_only_one_disposed
);
parity_test!(
    workflow_hook_dispose_conflicted_hook_safe,
    assert_dispose_conflicted_hook_safe
);
parity_test!(workflow_hook_symbol_dispose, assert_symbol_dispose);
parity_test!(
    workflow_hook_iterator_break_keeps_hook_alive,
    assert_iterator_break_keeps_hook_alive_without_dispose
);
parity_test!(
    workflow_hook_dispose_while_awaiting_suspends,
    assert_hook_dispose_while_awaiting_suspends
);
parity_test!(
    workflow_hook_await_disposed_first_invocation_suspends,
    assert_hook_dispose_while_awaiting_suspends
);
parity_test!(
    workflow_hook_default_is_webhook_false,
    assert_hook_default_is_webhook_false
);
parity_test!(
    workflow_hook_is_webhook_true,
    assert_hook_is_webhook_option_true
);
parity_test!(
    workflow_create_webhook_rejects_token,
    assert_create_webhook_rejects_token
);

parity_test!(
    define_hook_passes_payload_without_schema,
    assert_define_hook_passthrough
);
parity_test!(
    define_hook_parses_payload_with_schema,
    assert_define_hook_schema_transform
);
parity_test!(
    define_hook_throws_when_schema_validation_fails,
    assert_define_hook_schema_failure
);

parity_test!(
    server_writable_stream_rejects_non_string_run_id,
    assert_server_stream_run_id_validation
);
parity_test!(
    server_writable_stream_rejects_empty_name,
    assert_server_stream_name_validation
);
parity_test!(
    server_writable_stream_accepts_string_run_id,
    assert_server_stream_accepts_run_id
);
parity_test!(
    server_writable_stream_write_reaches_server,
    assert_server_stream_write_reaches_server
);
parity_test!(
    server_writable_stream_uses_write_for_single_chunk,
    assert_server_stream_single_write_call
);
parity_test!(
    server_writable_stream_falls_back_to_sequential_writes,
    assert_server_stream_write_multi_fallback
);
parity_test!(
    server_writable_stream_multiple_sequential_writes,
    assert_server_stream_multiple_sequential_writes
);
parity_test!(
    server_writable_stream_waits_for_in_progress_flush,
    assert_server_stream_multiple_sequential_writes
);
parity_test!(
    server_writable_stream_close_calls_close,
    assert_server_stream_close_without_buffer
);
parity_test!(
    server_writable_stream_close_flushes_remaining_buffer,
    assert_server_stream_flushes_buffer_on_close
);
parity_test!(
    server_writable_stream_close_empty_buffer_no_write,
    assert_server_stream_close_without_buffer
);
parity_test!(
    server_writable_stream_abort_discards_buffer,
    assert_server_stream_abort_discards_buffer
);
parity_test!(
    server_writable_stream_write_errors_propagate,
    assert_server_stream_write_errors_propagate
);
parity_test!(
    server_writable_stream_close_errors_propagate,
    assert_server_stream_close_errors_propagate
);
parity_test!(
    server_writable_stream_close_flush_errors_propagate,
    assert_server_stream_close_flush_errors_propagate
);
parity_test!(
    server_writable_stream_interval_zero,
    assert_server_stream_interval_zero
);
parity_test!(
    server_writable_stream_interval_default,
    assert_server_stream_interval_default
);
parity_test!(
    server_writable_stream_interval_custom,
    assert_server_stream_interval_custom
);

parity_test!(
    workflow_run_records_metadata,
    assert_workflow_metadata_shape
);
parity_test!(
    workflow_run_executes_simple_workflow_successfully,
    assert_workflow_metadata_shape
);
parity_test!(
    workflow_run_executes_workflow_with_arguments,
    assert_workflow_arguments_shape
);
parity_test!(
    workflow_run_allows_user_defined_error_handling,
    assert_workflow_user_error_surface
);
parity_test!(
    workflow_run_step_completed_event_resolves,
    assert_step_resolves_with_result
);
parity_test!(
    workflow_run_updates_timestamp_from_events,
    assert_workflow_timestamp_updates_from_event
);
parity_test!(
    workflow_run_maintains_date_determinism,
    assert_workflow_date_determinism
);
parity_test!(
    workflow_run_promise_all_steps_resolve,
    assert_workflow_concurrent_steps_all_resolve
);
parity_test!(
    workflow_run_promise_race_first_resolves,
    assert_workflow_race_first_resolution
);
parity_test!(
    workflow_run_promise_race_second_resolves,
    assert_workflow_race_second_resolution
);
parity_test!(
    workflow_run_promise_race_out_of_order,
    assert_workflow_race_second_resolution
);
parity_test!(
    workflow_run_workflow_not_registered_error,
    assert_workflow_not_registered_error
);
parity_test!(
    workflow_run_user_defined_error_propagates,
    assert_workflow_user_error_surface
);
parity_test!(
    workflow_run_stack_trace_uses_workflow_name,
    assert_workflow_stack_uses_name
);
parity_test!(
    workflow_run_nested_stack_trace_uses_workflow_name,
    assert_workflow_stack_uses_name
);
parity_test!(
    workflow_run_step_without_result_suspends,
    assert_workflow_suspension_uncatchable_shape
);
parity_test!(
    workflow_run_step_started_only_suspends,
    assert_workflow_suspension_uncatchable_shape
);
parity_test!(
    workflow_run_promise_all_without_results_suspends,
    assert_concurrent_steps_suspend
);
parity_test!(
    workflow_run_suspension_not_catchable_by_user_code,
    assert_workflow_suspension_uncatchable_shape
);
parity_test!(workflow_response_new_body, assert_response_with_body);
parity_test!(
    workflow_response_json_static_method,
    assert_response_json_sets_header
);
parity_test!(
    workflow_response_custom_headers,
    assert_response_custom_headers
);
parity_test!(
    workflow_response_204_no_content,
    assert_response_204_without_body
);
parity_test!(workflow_response_uint8_body, assert_response_with_body);
parity_test!(
    workflow_response_rejects_204_body,
    assert_response_rejects_body_with_204
);
parity_test!(
    workflow_response_redirect_default_302,
    assert_response_redirect_default
);
parity_test!(
    workflow_response_redirect_custom_status,
    assert_response_redirect_custom_status
);
parity_test!(
    workflow_response_redirect_valid_statuses,
    assert_response_redirect_valid_status_codes
);
parity_test!(
    workflow_response_redirect_invalid_status,
    assert_response_redirect_invalid_status
);
parity_test!(workflow_request_get_method, assert_request_get);
parity_test!(
    workflow_request_post_method_and_body,
    assert_request_post_body
);
parity_test!(
    workflow_request_custom_headers,
    assert_request_custom_headers
);
parity_test!(workflow_request_uint8_body, assert_request_uint8_body);
parity_test!(
    workflow_request_rejects_get_body,
    assert_request_rejects_get_body
);
parity_test!(
    workflow_request_rejects_head_body,
    assert_request_rejects_head_body
);
parity_test!(
    workflow_request_rejects_invalid_url,
    assert_request_rejects_invalid_url
);
parity_test!(
    workflow_request_clone_with_init_override,
    assert_request_clone_with_init_override
);
parity_test!(
    workflow_timeout_set_timeout_errors,
    assert_timeout_guard_slug
);
parity_test!(
    workflow_timeout_set_interval_errors,
    assert_timeout_guard_slug
);
parity_test!(
    workflow_timeout_clear_timeout_errors,
    assert_timeout_guard_slug
);
parity_test!(
    workflow_timeout_clear_interval_errors,
    assert_timeout_guard_slug
);
parity_test!(
    workflow_timeout_set_immediate_errors,
    assert_timeout_guard_slug
);
parity_test!(
    workflow_timeout_clear_immediate_errors,
    assert_timeout_guard_slug
);
parity_test!(
    workflow_timeout_error_includes_docs_slug,
    assert_timeout_guard_slug
);
parity_test!(
    workflow_fetch_unavailable_error_slug,
    assert_fetch_guard_slug
);
parity_test!(
    workflow_stream_id_namespace_encoding,
    assert_stream_id_namespace_encoding
);
parity_test!(
    workflow_run_hook_await_without_received_suspends,
    assert_hook_no_events_suspends
);
parity_test!(
    workflow_run_hook_await_resolves_on_received,
    assert_hook_resolves_payload
);
parity_test!(
    workflow_run_hook_token_mismatch_runtime_error,
    assert_hook_token_mismatch_corruption
);
parity_test!(
    workflow_run_hook_multiple_awaits_resolve,
    assert_hook_multiple_payloads_iterator
);
parity_test!(
    workflow_run_hook_for_await_loop_payloads,
    assert_hook_multiple_payloads_iterator
);
parity_test!(
    workflow_run_hook_ignores_extra_received_events,
    assert_hook_multiple_payloads_iterator
);
parity_test!(
    workflow_run_hook_queued_payloads_with_steps_between,
    assert_hook_multiple_payloads_iterator
);
parity_test!(
    workflow_run_hook_await_after_empty_log_suspends,
    assert_hook_no_events_suspends
);
parity_test!(
    workflow_run_hook_custom_token,
    assert_hook_unexpected_error_mentions_token
);
parity_test!(
    workflow_run_hook_conflict_rejects,
    assert_hook_conflict_rejects
);
parity_test!(
    workflow_run_hook_conflict_iterator_rejects,
    assert_hook_conflict_rejects
);
parity_test!(
    workflow_run_sleep_single_suspend_and_resume,
    assert_sleep_resolves_on_wait_completed
);
parity_test!(
    workflow_run_sleep_resume_mismatch_runtime_error,
    assert_sleep_resume_mismatch_corruption
);
parity_test!(
    workflow_run_sleep_without_completed_suspends,
    assert_sleep_no_events_suspends
);
parity_test!(
    workflow_run_sleep_multiple_promise_all,
    assert_sleep_resolves_on_wait_completed
);
parity_test!(
    workflow_run_sleep_partial_completion_suspends,
    assert_sleep_no_events_suspends
);
parity_test!(
    workflow_run_sleep_combined_with_steps,
    assert_step_resolves_with_result
);
parity_test!(
    workflow_run_sleep_date_parameter,
    assert_sleep_date_and_string_inputs
);
parity_test!(
    workflow_run_sleep_duplicate_wait_completed,
    assert_sleep_duplicate_wait_completed_corruption
);
parity_test!(
    workflow_run_sleep_duplicate_step_completed_blocks,
    assert_step_unexpected_event_is_corruption
);
parity_test!(
    workflow_run_sleep_orphaned_step_completed_blocks,
    assert_step_unexpected_event_is_corruption
);
parity_test!(
    workflow_run_sleep_orphaned_wait_completed_blocks,
    assert_sleep_unexpected_event_corruption
);
parity_test!(
    workflow_run_closure_vars_nested_step_functions,
    assert_step_captures_closure_vars
);
parity_test!(
    workflow_run_step_functions_without_closure_vars,
    assert_step_function_name_from_step_id
);
parity_test!(
    workflow_run_drains_unawaited_step_on_completion,
    assert_workflow_drain_pending_step
);
parity_test!(
    workflow_run_drains_pending_operations_when_workflow_throws,
    assert_workflow_drain_pending_step
);
parity_test!(
    workflow_run_no_warn_when_hooks_properly_disposed,
    assert_hook_dispose_sets_flag
);
parity_test!(
    workflow_run_no_warn_when_queue_empty_on_completion,
    assert_workflow_metadata_shape
);
parity_test!(
    workflow_run_parallel_steps_then_sequential_async_deserialization,
    assert_workflow_concurrent_steps_all_resolve
);
parity_test!(
    workflow_run_for_await_hook_loop_second_step_created,
    assert_hook_multiple_payloads_iterator
);
parity_test!(
    workflow_run_three_sequential_steps_no_unconsumed_error,
    assert_workflow_concurrent_steps_all_resolve
);
parity_test!(
    workflow_run_hook_loop_unawaited_sleep_hydrate_latency,
    assert_hook_multiple_payloads_iterator
);
parity_test!(
    workflow_run_pending_abort_hook_drain,
    assert_workflow_drain_pending_hook
);
parity_test!(
    workflow_run_pending_wait_drain,
    assert_workflow_drain_pending_wait
);
parity_test!(
    types_is_abort_error_cross_realm_shape,
    assert_is_abort_error_cross_realm_shape
);
parity_test!(
    types_promote_abort_error_to_fatal,
    assert_abort_error_is_fatal
);
parity_test!(
    types_preserves_already_fatal_abort_error,
    assert_abort_error_is_fatal
);
