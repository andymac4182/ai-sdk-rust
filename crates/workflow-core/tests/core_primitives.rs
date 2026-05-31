use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use serde_json::{Value, json};
use workflow_core::{
    capabilities::{SerializationFormat, get_run_capabilities},
    classify_error::{RunError, classify_run_error},
    context_errors::{ContextViolationError, WorkflowMetadata, with_workflow_context},
    define_hook::{HookEntity, SchemaIssue, define_hook},
    describe_error::{
        CONTEXT_ERROR_HINT, CORRUPTED_EVENT_LOG_HINT, ErrorAttribution, ErrorDescription,
        MAX_DELIVERIES_HINT, PersistedErrorSignal, REPLAY_TIMEOUT_HINT, RUNTIME_ERROR_HINT,
        SERIALIZATION_ERROR_HINT, WORLD_CONTRACT_HINT, describe_error, describe_run_error,
    },
    global::{HookInvocationQueueItem, QueueItem, StepInvocationQueueItem, WorkflowSuspension},
    log_format::{LogMetadata, compose_log_line},
    logger::runtime_logger,
    schemas::{StepInvokePayload, WorkflowInvokePayload},
    set_attributes::{
        AttributeChange, AttributeWorld, ExperimentalSetAttributesOptions,
        ExperimentalSetAttributesOutcome, StepContext, clear_attribute_world,
        experimental_set_attributes, reset_unsupported_world_warning, set_attribute_world,
        with_step_context,
    },
    source_map::remap_error_stack,
    types::{
        ErrorLike, UnknownErrorShape, UnknownField, is_abort_error, is_abort_error_shape,
        promote_abort_error_to_fatal,
    },
    util::{build_workflow_suspension_message, get_workflow_run_stream_id},
};
use workflow_errors::{RunErrorCode, WorkflowError};

macro_rules! row_test {
    ($name:ident, $body:block) => {
        #[test]
        fn $name() $body
    };
}

fn supports(version: Option<&str>, format: SerializationFormat) -> bool {
    get_run_capabilities(version)
        .supported_formats
        .contains(&format)
}

fn assert_baseline_only(version: Option<&str>) {
    assert!(supports(version, SerializationFormat::DevalueV1));
    assert!(!supports(version, SerializationFormat::Encrypted));
}

row_test!(wf04_capabilities_row_001, {
    assert_baseline_only(None);
});
row_test!(wf04_capabilities_row_002, {
    assert_baseline_only(Some("dev"));
});
row_test!(wf04_capabilities_row_003, {
    assert_baseline_only(Some("not-a-version"));
});
row_test!(wf04_capabilities_row_004, {
    assert_baseline_only(Some(""));
});
row_test!(wf04_capabilities_row_005, {
    assert_baseline_only(Some("4.2"));
});
row_test!(wf04_capabilities_row_006, {
    assert_baseline_only(Some("4"));
});
row_test!(wf04_capabilities_row_007, {
    assert!(supports(
        Some("v4.2.0-beta.64"),
        SerializationFormat::Encrypted
    ));
});
row_test!(wf04_capabilities_row_008, {
    assert_baseline_only(Some("4.1.0-beta.63"));
});
row_test!(wf04_capabilities_row_009, {
    assert_baseline_only(Some("4.0.1-beta.27"));
});
row_test!(wf04_capabilities_row_010, {
    assert_baseline_only(Some("3.0.0"));
});
row_test!(wf04_capabilities_row_011, {
    assert!(supports(
        Some("4.2.0-beta.64"),
        SerializationFormat::DevalueV1
    ));
    assert!(supports(
        Some("4.2.0-beta.64"),
        SerializationFormat::Encrypted
    ));
});
row_test!(wf04_capabilities_row_012, {
    assert!(supports(
        Some("4.2.0-beta.74"),
        SerializationFormat::Encrypted
    ));
});
row_test!(wf04_capabilities_row_013, {
    assert!(supports(Some("4.2.0"), SerializationFormat::Encrypted));
});
row_test!(wf04_capabilities_row_014, {
    assert!(supports(Some("5.0.0"), SerializationFormat::Encrypted));
});

fn classify(error: impl Into<RunError>) -> RunErrorCode {
    classify_run_error(&error.into())
}

row_test!(wf04_classify_error_row_001, {
    assert_eq!(
        classify(WorkflowError::corrupted_event_log("corrupted event log")),
        RunErrorCode::CorruptedEventLog
    );
});
row_test!(wf04_classify_error_row_002, {
    assert_eq!(
        classify(WorkflowError::workflow_runtime("corrupted event log")),
        RunErrorCode::RuntimeError
    );
});
row_test!(wf04_classify_error_row_003, {
    assert_eq!(
        classify(WorkflowError::workflow_not_registered("myWorkflow")),
        RunErrorCode::RuntimeError
    );
});
row_test!(wf04_classify_error_row_004, {
    assert_eq!(
        classify(WorkflowError::native("Error", "user code broke")),
        RunErrorCode::UserError
    );
});
row_test!(wf04_classify_error_row_005, {
    assert_eq!(
        classify(WorkflowError::native("TypeError", "cannot read property")),
        RunErrorCode::UserError
    );
});
row_test!(wf04_classify_error_row_006, {
    assert_eq!(
        classify(WorkflowError::workflow_world("Internal Server Error").with_status(500)),
        RunErrorCode::UserError
    );
});
row_test!(wf04_classify_error_row_007, {
    assert_eq!(
        classify(
            WorkflowError::workflow_world("Schema validation failed for POST /v3/runs/wrun/events")
                .with_code("SCHEMA_VALIDATION")
        ),
        RunErrorCode::WorldContractError
    );
});
row_test!(wf04_classify_error_row_008, {
    assert_eq!(
        classify(
            WorkflowError::workflow_world(
                "Failed to parse response body for GET /v3/runs/wrun/events"
            )
            .with_code("PARSE_ERROR")
        ),
        RunErrorCode::WorldContractError
    );
});
row_test!(wf04_classify_error_row_009, {
    assert_eq!(
        classify_run_error(&RunError::String("string error".to_string())),
        RunErrorCode::UserError
    );
});
row_test!(wf04_classify_error_row_010, {
    assert_eq!(classify_run_error(&RunError::Null), RunErrorCode::UserError);
});
row_test!(wf04_classify_error_row_011, {
    assert_eq!(
        classify_run_error(&RunError::Undefined),
        RunErrorCode::UserError
    );
});
row_test!(wf04_classify_error_row_012, {
    assert_eq!(
        classify(WorkflowError::hook_conflict("my-token")),
        RunErrorCode::UserError
    );
});
row_test!(wf04_classify_error_row_013, {
    assert_eq!(
        classify(WorkflowError::runtime_decryption("decrypt failed")),
        RunErrorCode::RuntimeError
    );
});
row_test!(wf04_classify_error_row_014, {
    assert_eq!(
        classify(WorkflowError::native(
            "OperationError",
            "The operation failed for an operation-specific reason"
        )),
        RunErrorCode::UserError
    );
});

row_test!(wf04_context_errors_row_001, {
    let err = ContextViolationError::not_in_workflow_context(
        "createHook()",
        "https://workflow-sdk.dev/docs/api-reference/workflow/create-hook",
    );
    assert_eq!(err.name(), "NotInWorkflowContextError");
    assert_eq!(
        err.message(),
        "`createHook()` can only be called inside a workflow function\n\u{2570}\u{25b6} docs: https://workflow-sdk.dev/docs/api-reference/workflow/create-hook"
    );
});
row_test!(wf04_context_errors_row_002, {
    let err =
        ContextViolationError::not_in_workflow_context("createHook()", "https://example.com/docs");
    assert!(!err.message().contains("functionName"));
});
row_test!(wf04_context_errors_row_003, {
    let err = ContextViolationError::not_in_step_context(
        "getStepMetadata()",
        "https://workflow-sdk.dev/docs/api-reference/workflow/get-step-metadata",
    );
    assert!(
        err.message()
            .contains("can only be called inside a step function")
    );
    assert!(
        err.message().contains(
            "docs: https://workflow-sdk.dev/docs/api-reference/workflow/get-step-metadata"
        )
    );
});
row_test!(wf04_context_errors_row_004, {
    let err = ContextViolationError::not_in_workflow_or_step_context(
        "getWorkflowMetadata()",
        "https://workflow-sdk.dev/docs/api-reference/workflow/get-workflow-metadata",
    );
    assert!(
        err.message()
            .contains("can only be called inside a workflow or step function")
    );
});
row_test!(wf04_context_errors_row_005, {
    let err = with_workflow_context(
        WorkflowMetadata {
            workflow_name: "workflow//./src/workflows/example.ts//myWorkflow".to_string(),
        },
        || {
            ContextViolationError::unavailable_in_workflow_context(
                "resumeHook()",
                "https://workflow-sdk.dev/docs/api-reference/workflow-api/resume-hook",
            )
        },
    );
    assert!(
        err.message()
            .contains("cannot be called from a workflow context")
    );
    assert!(
        err.message()
            .contains("workflow//./src/workflows/example.ts//myWorkflow")
    );
});
row_test!(wf04_context_errors_row_006, {
    let err = ContextViolationError::unavailable_in_workflow_context(
        "resumeHook()",
        "https://workflow-sdk.dev/docs/api-reference/workflow-api/resume-hook",
    );
    assert!(err.message().contains("from a workflow context"));
});
row_test!(wf04_context_errors_row_007, {
    let err =
        ContextViolationError::not_in_workflow_context("createHook()", "https://example.com/docs");
    assert!(!err.message().contains("\u{1b}["));
});
row_test!(wf04_context_errors_row_008, {
    let err =
        ContextViolationError::not_in_workflow_context("createHook()", "https://example.com/docs");
    assert!(!format!("{err:?}").contains("\u{1b}["));
});
row_test!(wf04_context_errors_row_009, {
    let err =
        ContextViolationError::not_in_workflow_context("createHook()", "https://example.com/docs");
    assert!(err.pretty_message().contains("NotInWorkflowContextError:"));
    assert!(err.pretty_message().contains("createHook()"));
    assert!(
        err.pretty_message()
            .contains("can only be called inside a workflow function")
    );
    assert!(err.pretty_message().contains("\u{2570}\u{25b6}"));
    assert!(err.pretty_message().contains("docs:"));
});
row_test!(wf04_context_errors_row_010, {
    let err =
        ContextViolationError::not_in_workflow_context("createHook()", "https://example.com/docs");
    assert_eq!(
        err.pretty_message()
            .matches("\u{2570}\u{25b6} docs:")
            .count(),
        1
    );
});
row_test!(wf04_context_errors_row_011, {
    let err =
        ContextViolationError::not_in_workflow_context("createHook()", "https://example.com/docs");
    assert!(err.pretty_message().contains("NotInWorkflowContextError:"));
    assert!(err.pretty_message().contains("\u{2570}\u{25b6}"));
});
row_test!(wf04_context_errors_row_012, {
    assert!(
        ContextViolationError::not_in_workflow_context("createHook()", "https://example.com")
            .is_fatal()
    );
});
row_test!(wf04_context_errors_row_013, {
    assert!(
        ContextViolationError::not_in_step_context("getStepMetadata()", "https://example.com")
            .is_fatal()
    );
});
row_test!(wf04_context_errors_row_014, {
    assert!(
        ContextViolationError::not_in_workflow_or_step_context(
            "getWorkflowMetadata()",
            "https://example.com"
        )
        .is_fatal()
    );
});
row_test!(wf04_context_errors_row_015, {
    assert!(
        ContextViolationError::unavailable_in_workflow_context(
            "resumeHook()",
            "https://example.com"
        )
        .is_fatal()
    );
});

fn approval_schema(value: Value) -> Result<Value, Vec<SchemaIssue>> {
    let Some(input) = value.as_object() else {
        return Err(vec![SchemaIssue::new("Invalid payload: expected object")]);
    };
    let mut issues = Vec::new();
    if !input.get("approved").is_some_and(Value::is_boolean) {
        issues.push(SchemaIssue::new(
            "Invalid input: expected boolean at \"approved\"",
        ));
    }
    if !input.get("comment").is_some_and(Value::is_string) {
        issues.push(SchemaIssue::new(
            "Invalid input: expected string at \"comment\"",
        ));
    }
    if !issues.is_empty() {
        return Err(issues);
    }
    Ok(json!({
        "approved": input["approved"].as_bool().unwrap(),
        "comment": input["comment"].as_str().unwrap().trim()
    }))
}

fn test_hook(
    calls: Arc<Mutex<Vec<(String, Value)>>>,
    schema: Option<Arc<workflow_core::define_hook::SchemaValidator>>,
) -> workflow_core::define_hook::TypedHook {
    define_hook(
        schema,
        Arc::new(move |token, payload| {
            calls
                .lock()
                .expect("hook calls mutex poisoned")
                .push((token.to_string(), payload));
            Ok(HookEntity {
                hook_id: "hook-id".to_string(),
                token: "token".to_string(),
                run_id: "run-id".to_string(),
            })
        }),
    )
}

row_test!(wf04_define_hook_row_001, {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let hook = test_hook(Arc::clone(&calls), None);
    let payload = json!({"approved": true, "comment": "Looks good"});
    hook.resume("token", payload.clone()).unwrap();
    assert_eq!(
        calls.lock().unwrap().as_slice(),
        &[("token".to_string(), payload)]
    );
});
row_test!(wf04_define_hook_row_002, {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let hook = test_hook(Arc::clone(&calls), Some(Arc::new(approval_schema)));
    hook.resume("token", json!({"approved": true, "comment": "  Ready!  "}))
        .unwrap();
    assert_eq!(
        calls.lock().unwrap().as_slice(),
        &[(
            "token".to_string(),
            json!({"approved": true, "comment": "Ready!"})
        )]
    );
});
row_test!(wf04_define_hook_row_003, {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let hook = test_hook(calls, Some(Arc::new(approval_schema)));
    let err = hook
        .resume("token", json!({"approved": "yes", "comment": 123}))
        .unwrap_err();
    assert_eq!(
        err.message(),
        "Hook payload did not match the defined schema:\n  Invalid input: expected boolean at \"approved\"\n  Invalid input: expected string at \"comment\""
    );
});

fn assert_description(
    description: ErrorDescription,
    attribution: ErrorAttribution,
    code: RunErrorCode,
    hint: Option<&str>,
) {
    assert_eq!(description.attribution, attribution);
    assert_eq!(description.error_code, code);
    assert_eq!(description.hint, hint);
}

row_test!(wf04_describe_error_row_001, {
    assert_description(
        describe_error(
            &WorkflowError::native("Error", "something user code did").into(),
            None,
        ),
        ErrorAttribution::User,
        RunErrorCode::UserError,
        None,
    );
});
row_test!(wf04_describe_error_row_002, {
    assert_eq!(
        describe_error(&RunError::String("string".to_string()), None).attribution,
        ErrorAttribution::User
    );
    assert_eq!(
        describe_error(&RunError::Undefined, None).attribution,
        ErrorAttribution::User
    );
    assert_eq!(
        describe_error(&RunError::Null, None).attribution,
        ErrorAttribution::User
    );
});
row_test!(wf04_describe_error_row_003, {
    assert_description(
        describe_error(
            &WorkflowError::serialization("Failed to serialize step arguments").into(),
            None,
        ),
        ErrorAttribution::User,
        RunErrorCode::UserError,
        Some(SERIALIZATION_ERROR_HINT),
    );
});
row_test!(wf04_describe_error_row_004, {
    let workflow_only = describe_error(
        &ContextViolationError::not_in_workflow_context("createHook", "https://example.com/hooks")
            .into(),
        None,
    );
    let step_only = describe_error(
        &ContextViolationError::not_in_step_context("respondWith", "https://example.com/webhook")
            .into(),
        None,
    );
    assert_eq!(workflow_only.attribution, ErrorAttribution::User);
    assert_eq!(step_only.attribution, ErrorAttribution::User);
    assert!(workflow_only.hint.unwrap().contains("wrong context"));
});
row_test!(wf04_describe_error_row_005, {
    let result = describe_error(
        &WorkflowError::workflow_runtime("corrupted event log").into(),
        None,
    );
    assert_eq!(result.attribution, ErrorAttribution::Sdk);
    assert_eq!(result.error_code, RunErrorCode::RuntimeError);
    assert!(result.hint.unwrap().contains("internal workflow SDK error"));
});
row_test!(wf04_describe_error_row_006, {
    let result = describe_error(
        &WorkflowError::corrupted_event_log("corrupted event log").into(),
        None,
    );
    assert_eq!(result.attribution, ErrorAttribution::Sdk);
    assert_eq!(result.error_code, RunErrorCode::CorruptedEventLog);
    assert!(result.hint.unwrap().contains("event log contains"));
});
row_test!(wf04_describe_error_row_007, {
    let result = describe_error(
        &WorkflowError::step_not_registered("missingStep").into(),
        None,
    );
    assert_eq!(result.attribution, ErrorAttribution::Sdk);
    assert_eq!(result.error_code, RunErrorCode::RuntimeError);
});
row_test!(wf04_describe_error_row_008, {
    let result = describe_error(&RunError::Undefined, Some(RunErrorCode::ReplayTimeout));
    assert_eq!(result.attribution, ErrorAttribution::Sdk);
    assert_eq!(result.error_code, RunErrorCode::ReplayTimeout);
    assert!(
        result
            .hint
            .unwrap()
            .contains("replay between step boundaries took too long")
    );
});
row_test!(wf04_describe_error_row_009, {
    let result = describe_error(
        &RunError::Undefined,
        Some(RunErrorCode::MaxDeliveriesExceeded),
    );
    assert_eq!(result.attribution, ErrorAttribution::Sdk);
    assert_eq!(result.error_code, RunErrorCode::MaxDeliveriesExceeded);
    assert!(result.hint.unwrap().contains("max-delivery budget"));
});
row_test!(wf04_describe_error_row_010, {
    let result = describe_error(&RunError::Undefined, Some(RunErrorCode::WorldContractError));
    assert_eq!(result.attribution, ErrorAttribution::Sdk);
    assert_eq!(result.error_code, RunErrorCode::WorldContractError);
    assert!(result.hint.unwrap().contains("SDK contract"));
});
row_test!(wf04_describe_error_row_011, {
    let result = describe_error(
        &WorkflowError::native("Error", "something").into(),
        Some(RunErrorCode::ReplayTimeout),
    );
    assert_eq!(result.error_code, RunErrorCode::ReplayTimeout);
    assert_eq!(result.attribution, ErrorAttribution::Sdk);
});
row_test!(wf04_describe_error_row_012, {
    assert_description(
        describe_run_error(&PersistedErrorSignal {
            error_code: Some("USER_ERROR".to_string()),
            error_name: Some("Error".to_string()),
        }),
        ErrorAttribution::User,
        RunErrorCode::UserError,
        None,
    );
});
row_test!(wf04_describe_error_row_013, {
    assert_description(
        describe_run_error(&PersistedErrorSignal {
            error_code: Some("USER_ERROR".to_string()),
            error_name: Some("SerializationError".to_string()),
        }),
        ErrorAttribution::User,
        RunErrorCode::UserError,
        Some(SERIALIZATION_ERROR_HINT),
    );
});
row_test!(wf04_describe_error_row_014, {
    let result = describe_run_error(&PersistedErrorSignal {
        error_code: Some("USER_ERROR".to_string()),
        error_name: Some("NotInWorkflowContextError".to_string()),
    });
    assert_eq!(result.attribution, ErrorAttribution::User);
    assert!(result.hint.unwrap().contains("wrong context"));
});
row_test!(wf04_describe_error_row_015, {
    let result = describe_run_error(&PersistedErrorSignal {
        error_code: Some("RUNTIME_ERROR".to_string()),
        error_name: Some("WorkflowRuntimeError".to_string()),
    });
    assert_eq!(result.attribution, ErrorAttribution::Sdk);
    assert!(result.hint.unwrap().contains("internal workflow SDK error"));
});
row_test!(wf04_describe_error_row_016, {
    let result = describe_run_error(&PersistedErrorSignal {
        error_code: Some("REPLAY_TIMEOUT".to_string()),
        error_name: None,
    });
    assert_eq!(result.attribution, ErrorAttribution::Sdk);
    assert!(
        result
            .hint
            .unwrap()
            .contains("replay between step boundaries took too long")
    );
});
row_test!(wf04_describe_error_row_017, {
    let result = describe_run_error(&PersistedErrorSignal {
        error_code: Some("CORRUPTED_EVENT_LOG".to_string()),
        error_name: None,
    });
    assert_eq!(result.attribution, ErrorAttribution::Sdk);
    assert!(result.hint.unwrap().contains("event log contains"));
});
row_test!(wf04_describe_error_row_018, {
    let result = describe_run_error(&PersistedErrorSignal {
        error_code: None,
        error_name: Some("CorruptedEventLogError".to_string()),
    });
    assert_eq!(result.attribution, ErrorAttribution::Sdk);
    assert_eq!(result.error_code, RunErrorCode::CorruptedEventLog);
    assert!(result.hint.unwrap().contains("event log contains"));
});
row_test!(wf04_describe_error_row_019, {
    let result = describe_run_error(&PersistedErrorSignal {
        error_code: Some("MAX_DELIVERIES_EXCEEDED".to_string()),
        error_name: None,
    });
    assert_eq!(result.attribution, ErrorAttribution::Sdk);
    assert!(result.hint.unwrap().contains("max-delivery budget"));
});
row_test!(wf04_describe_error_row_020, {
    let result = describe_run_error(&PersistedErrorSignal {
        error_code: Some("WORLD_CONTRACT_ERROR".to_string()),
        error_name: None,
    });
    assert_eq!(result.attribution, ErrorAttribution::Sdk);
    assert!(result.hint.unwrap().contains("SDK contract"));
});
row_test!(wf04_describe_error_row_021, {
    let result = describe_run_error(&PersistedErrorSignal {
        error_code: Some("RUNTIME_ERROR".to_string()),
        error_name: None,
    });
    assert_eq!(result.attribution, ErrorAttribution::Sdk);
    assert!(result.hint.unwrap().contains("internal workflow SDK error"));
});
row_test!(wf04_describe_error_row_022, {
    assert_description(
        describe_run_error(&PersistedErrorSignal::default()),
        ErrorAttribution::User,
        RunErrorCode::UserError,
        None,
    );
});
row_test!(wf04_describe_error_row_023, {
    assert_description(
        describe_error(&WorkflowError::native("Error", "boom").into(), None),
        ErrorAttribution::User,
        RunErrorCode::UserError,
        None,
    );
});
row_test!(wf04_describe_error_row_024, {
    assert_description(
        describe_error(&WorkflowError::serialization("boom").into(), None),
        ErrorAttribution::User,
        RunErrorCode::UserError,
        Some(SERIALIZATION_ERROR_HINT),
    );
});
row_test!(wf04_describe_error_row_025, {
    assert_description(
        describe_error(
            &ContextViolationError::not_in_workflow_context(
                "createHook",
                "https://workflow-sdk.dev/docs/api-reference/workflow/create-hook",
            )
            .into(),
            None,
        ),
        ErrorAttribution::User,
        RunErrorCode::UserError,
        Some(CONTEXT_ERROR_HINT),
    );
});
row_test!(wf04_describe_error_row_026, {
    assert_description(
        describe_error(
            &WorkflowError::workflow_runtime("internal invariant").into(),
            None,
        ),
        ErrorAttribution::Sdk,
        RunErrorCode::RuntimeError,
        Some(RUNTIME_ERROR_HINT),
    );
});
row_test!(wf04_describe_error_row_027, {
    assert_description(
        describe_error(
            &WorkflowError::corrupted_event_log("event mismatch").into(),
            None,
        ),
        ErrorAttribution::Sdk,
        RunErrorCode::CorruptedEventLog,
        Some(CORRUPTED_EVENT_LOG_HINT),
    );
});
row_test!(wf04_describe_error_row_028, {
    assert_description(
        describe_error(&RunError::Undefined, Some(RunErrorCode::ReplayTimeout)),
        ErrorAttribution::Sdk,
        RunErrorCode::ReplayTimeout,
        Some(REPLAY_TIMEOUT_HINT),
    );
});
row_test!(wf04_describe_error_row_029, {
    assert_description(
        describe_error(
            &RunError::Undefined,
            Some(RunErrorCode::MaxDeliveriesExceeded),
        ),
        ErrorAttribution::Sdk,
        RunErrorCode::MaxDeliveriesExceeded,
        Some(MAX_DELIVERIES_HINT),
    );
});
row_test!(wf04_describe_error_row_030, {
    assert_description(
        describe_error(&RunError::Undefined, Some(RunErrorCode::WorldContractError)),
        ErrorAttribution::Sdk,
        RunErrorCode::WorldContractError,
        Some(WORLD_CONTRACT_HINT),
    );
});

fn step_item(correlation_id: &str, step_name: &str, args: Vec<Value>) -> QueueItem {
    QueueItem::Step(StepInvocationQueueItem {
        correlation_id: correlation_id.to_string(),
        step_name: step_name.to_string(),
        args,
        closure_vars: None,
        this_val: None,
        has_created_event: None,
    })
}

fn hook_item(correlation_id: &str, token: &str) -> QueueItem {
    QueueItem::Hook(HookInvocationQueueItem {
        correlation_id: correlation_id.to_string(),
        token: token.to_string(),
        metadata: None,
        has_created_event: None,
        disposed: None,
        is_webhook: None,
        is_system: None,
        abort_requested: None,
        abort_reason: None,
    })
}

fn disposed_hook_item(correlation_id: &str, token: &str) -> QueueItem {
    let mut item = match hook_item(correlation_id, token) {
        QueueItem::Hook(item) => item,
        _ => unreachable!(),
    };
    item.disposed = Some(true);
    QueueItem::Hook(item)
}

row_test!(wf04_global_row_001, {
    let error = WorkflowError::fatal("Test fatal error");
    assert_eq!(error.message(), "Test fatal error");
    assert_eq!(error.name(), "FatalError");
    assert!(error.is_fatal());
});
row_test!(wf04_global_row_002, {
    let error = WorkflowError::fatal("Test error");
    assert!(error.is_fatal());
    assert_eq!(error.name(), "FatalError");
});
row_test!(wf04_global_row_003, {
    let error = WorkflowError::fatal("Test error").with_stack("FatalError: Test error");
    assert!(error.stack().unwrap().contains("FatalError"));
    assert!(error.stack().unwrap().contains("Test error"));
});
row_test!(wf04_global_row_004, {
    let steps = vec![step_item(
        "inv-1",
        "test-step",
        vec![json!("arg1"), json!("arg2")],
    )];
    let error = WorkflowSuspension::new(steps.clone());
    assert_eq!(error.name(), "WorkflowSuspension");
    assert_eq!(error.steps, steps);
});
row_test!(wf04_global_row_005, {
    let error = WorkflowSuspension::new(vec![step_item(
        "inv-1",
        "test-step",
        vec![json!("arg1"), json!(42), json!({"key": "value"})],
    )]);
    assert_eq!(error.message(), "1 step has not been run yet");
});
row_test!(wf04_global_row_006, {
    let error = WorkflowSuspension::new(vec![
        step_item("inv-1", "__wkf_step_1", vec![json!("arg1")]),
        step_item("inv-2", "__wkf_step_2", vec![json!("arg2")]),
    ]);
    assert_eq!(error.message(), "2 steps have not been run yet");
});
row_test!(wf04_global_row_007, {
    let error = WorkflowSuspension::new(Vec::new());
    assert!(error.steps.is_empty());
    assert_eq!(error.message(), "0 steps have not been run yet");
});
row_test!(wf04_global_row_008, {
    let error = WorkflowSuspension::new(vec![
        step_item(
            "complex-inv",
            "complex-step",
            vec![
                json!("string"),
                json!(123),
                json!(true),
                json!({"nested": {"object": "value"}}),
                json!([1, 2, 3]),
                Value::Null,
            ],
        ),
        step_item("another-inv", "another-step", Vec::new()),
    ]);
    assert_eq!(error.message(), "2 steps have not been run yet");
    assert_eq!(error.step_count, 2);
});
row_test!(wf04_global_row_009, {
    let error = WorkflowSuspension::new(vec![step_item("inv-1", "test-step", Vec::new())]);
    assert_eq!(error.name(), "WorkflowSuspension");
    assert_eq!(error.step_count, 1);
});
row_test!(wf04_global_row_010, {
    let error = WorkflowSuspension::new(vec![step_item("inv-1", "test-step", vec![json!("arg")])]);
    assert_eq!(format!("{error}"), "1 step has not been run yet");
});
row_test!(wf04_global_row_011, {
    let error = WorkflowSuspension::new(vec![
        step_item(
            "db-query-123",
            "database-query",
            vec![json!("SELECT * FROM users"), json!({"limit": 10})],
        ),
        step_item(
            "email-456",
            "send-email",
            vec![json!("user@example.com"), json!("Welcome!")],
        ),
    ]);
    assert_eq!(error.steps.len(), 2);
    assert_eq!(error.step_count, 2);
});
row_test!(wf04_global_row_012, {
    let error = WorkflowSuspension::new(vec![hook_item("hook_123", "webhook-token")]);
    assert_eq!(error.message(), "1 hook has not been created yet");
    assert_eq!(error.hook_count, 1);
});
row_test!(wf04_global_row_013, {
    let error = WorkflowSuspension::new(vec![
        hook_item("hook_123", "webhook-token-1"),
        hook_item("hook_456", "webhook-token-2"),
    ]);
    assert_eq!(error.message(), "2 hooks have not been created yet");
    assert_eq!(error.hook_count, 2);
});
row_test!(wf04_global_row_014, {
    let error = WorkflowSuspension::new(vec![hook_item("hook_123", "my-token")]);
    assert_eq!(error.message(), "1 hook has not been created yet");
    assert_eq!(error.hook_count, 1);
});
row_test!(wf04_global_row_015, {
    let error = WorkflowSuspension::new(vec![
        hook_item("hook_123", "token-1"),
        hook_item("hook_456", "token-2"),
    ]);
    assert_eq!(error.message(), "2 hooks have not been created yet");
    assert_eq!(error.hook_count, 2);
});
row_test!(wf04_global_row_016, {
    let error = WorkflowSuspension::new(vec![
        step_item("inv-1", "test-step", Vec::new()),
        hook_item("hook_123", "webhook-token"),
        hook_item("hook_456", "my-token"),
    ]);
    assert_eq!(
        error.message(),
        "1 step and 2 hooks have not been processed yet"
    );
    assert_eq!(error.step_count, 1);
    assert_eq!(error.hook_count, 2);
});
row_test!(wf04_global_row_017, {
    let error = WorkflowSuspension::new(vec![
        step_item("inv-1", "step-1", Vec::new()),
        step_item("inv-2", "step-2", Vec::new()),
        hook_item("hook_123", "webhook-token"),
    ]);
    assert_eq!(
        error.message(),
        "2 steps and 1 hook have not been processed yet"
    );
});
row_test!(wf04_global_row_018, {
    let error = WorkflowSuspension::new(vec![
        step_item("inv-1", "test-step", Vec::new()),
        hook_item("hook_123", "my-token"),
    ]);
    assert_eq!(
        error.message(),
        "1 step and 1 hook have not been processed yet"
    );
});
row_test!(wf04_global_row_019, {
    let error = WorkflowSuspension::new(vec![hook_item("hook_123", "webhook-token")]);
    assert_eq!(error.message(), "1 hook has not been created yet");
});
row_test!(wf04_global_row_020, {
    let error = WorkflowSuspension::new(vec![hook_item("hook_123", "my-token")]);
    assert_eq!(error.message(), "1 hook has not been created yet");
});
row_test!(wf04_global_row_021, {
    let error = WorkflowSuspension::new(vec![disposed_hook_item("hook_123", "token-1")]);
    assert_eq!(error.hook_count, 0);
    assert_eq!(error.hook_disposed_count, 1);
    assert_eq!(
        error.message(),
        "1 hook disposal has not been processed yet"
    );
});
row_test!(wf04_global_row_022, {
    let error = WorkflowSuspension::new(vec![
        disposed_hook_item("hook_123", "token-1"),
        disposed_hook_item("hook_456", "token-2"),
    ]);
    assert_eq!(error.hook_disposed_count, 2);
    assert_eq!(
        error.message(),
        "2 hook disposals have not been processed yet"
    );
});
row_test!(wf04_global_row_023, {
    let error = WorkflowSuspension::new(vec![
        hook_item("hook_123", "token-1"),
        disposed_hook_item("hook_456", "token-2"),
    ]);
    assert_eq!(error.hook_count, 1);
    assert_eq!(error.hook_disposed_count, 1);
    assert_eq!(
        error.message(),
        "1 hook and 1 hook disposal have not been processed yet"
    );
});

fn meta(entries: impl IntoIterator<Item = (&'static str, Value)>) -> LogMetadata {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

row_test!(wf04_log_format_row_001, {
    assert_eq!(
        compose_log_line("[workflow-sdk]", "something happened", None),
        "[workflow-sdk] something happened"
    );
    assert_eq!(
        compose_log_line(
            "[workflow-sdk]",
            "something happened",
            Some(&LogMetadata::new())
        ),
        "[workflow-sdk] something happened"
    );
});
row_test!(wf04_log_format_row_002, {
    let out = compose_log_line(
        "[workflow-sdk]",
        "Step add (./workflows/x) threw a FatalError \u{2014} bubbling up to parent workflow\nFatalError: User threw a FatalError\n    at maybeFailingStep (./workflows/x.ts:15:11)\n    at <unknown> (../../packages/core/src/runtime/step-handler.ts:535:32)",
        Some(&meta([
            ("workflowRunId", json!("wrun_01ABC")),
            ("stepId", json!("step_01XYZ")),
            ("stepName", json!("step//./workflows/x//add")),
            ("errorAttribution", json!("user")),
            ("errorName", json!("FatalError")),
            ("errorMessage", json!("User threw a FatalError")),
        ])),
    );
    assert!(out.contains("user error \u{00b7} FatalError"));
    assert!(out.contains("run    wrun_01ABC"));
    assert!(out.contains("step   step_01XYZ \u{00b7} add (./workflows/x)"));
    assert!(out.contains("FatalError: User threw a FatalError"));
});
row_test!(wf04_log_format_row_003, {
    let stack = [
        "FatalError: boom",
        "    at userStep (./workflows/x.ts:15:11)",
        "    at <unknown> (../../packages/core/src/runtime/step-handler.ts:535:32)",
        "    at <unknown> (.../node_modules/.pnpm/next@16.2.1/dist/server/base-server.js:1454:9)",
        "    at <unknown> (.../node_modules/.pnpm/next@16.2.1/dist/server/dev/next-dev-server.js:394:20)",
        "    at <unknown> (.../node_modules/.pnpm/@opentelemetry+api@1.9.1/build/src/api/trace.js:160:25)",
        "    at <unknown> (../../packages/core/src/runtime/helpers.ts:414:12)",
        "    at <unknown> (node:internal/process/task_queues:64:5)",
        "    at <unknown> (.../node_modules/next/dist/server/lib/start-server.js:225:13)",
    ].join("\n");
    let out = compose_log_line("[workflow-sdk]", &format!("Step blew up\n{stack}"), None);
    assert!(out.contains("\u{2026} 3 more frames in framework internals"));
    assert!(out.contains("\u{2026} 2 more frames in framework internals"));
});
row_test!(wf04_log_format_row_004, {
    let out = compose_log_line(
        "[workflow-sdk]",
        "Workflow myFlow failed due to an SDK runtime error\nCorruptedEventLogError: corrupted event log",
        Some(&meta([
            ("errorCode", json!("CORRUPTED_EVENT_LOG")),
            ("errorAttribution", json!("sdk")),
            ("errorName", json!("CorruptedEventLogError")),
            ("errorMessage", json!("corrupted event log")),
            ("hint", json!("This is an internal workflow SDK error.")),
        ])),
    );
    assert!(out.contains("sdk error \u{00b7} CorruptedEventLogError"));
    assert!(out.contains("code   CORRUPTED_EVENT_LOG"));
    assert!(out.contains("hint: This is an internal workflow SDK error."));
});
row_test!(wf04_log_format_row_005, {
    let out = compose_log_line(
        "[workflow-sdk]",
        "Workflow simple threw: thing went wrong\nError: thing went wrong",
        Some(&meta([
            ("errorAttribution", json!("user")),
            ("errorName", json!("Error")),
            ("errorMessage", json!("thing went wrong")),
        ])),
    );
    assert!(!out.contains("message"));
    assert!(out.contains("user error \u{00b7} Error"));
});
row_test!(wf04_log_format_row_006, {
    let out = compose_log_line(
        "[workflow-sdk]",
        "msg",
        Some(&meta([
            ("workflowRunId", json!("wrun_X")),
            ("workflowName", json!("not-a-machine-name")),
        ])),
    );
    assert!(out.contains("wrun_X"));
});
row_test!(wf04_log_format_row_007, {
    let out = compose_log_line(
        "[workflow-sdk]",
        "msg",
        Some(&meta([
            ("zoo", json!("last")),
            ("apple", json!("first")),
            ("banana", json!(42)),
        ])),
    );
    assert!(out.contains("apple  first\n  banana 42\n  zoo    last"));
});
row_test!(wf04_log_format_row_008, {
    let out = compose_log_line(
        "[workflow-sdk]",
        "Step add (./workflows/x) hit max retries \u{2014} bubbling error",
        Some(&meta([
            ("workflowRunId", json!("wrun_01ABC")),
            ("workflowName", json!("workflow//./workflows/x//myWorkflow")),
            ("stepId", json!("step_01XYZ")),
            ("stepName", json!("step//./workflows/x//add")),
            ("attempt", json!(4)),
            ("retryCount", json!(3)),
            ("errorAttribution", json!("user")),
            ("errorName", json!("Error")),
            ("errorMessage", json!("Transient failure")),
        ])),
    );
    assert!(out.contains("retry  4 attempts \u{00b7} 3 max retries"));
});

row_test!(wf04_logger_row_001, {
    let out = runtime_logger().error("boom", Some(meta([("foo", json!("bar"))])));
    assert!(out.contains("[workflow-sdk] boom"));
    assert!(out.contains("foo"));
    assert!(out.contains("bar"));
});
row_test!(wf04_logger_row_002, {
    let out = runtime_logger().warn("watch out", Some(meta([("foo", json!("bar"))])));
    assert!(out.contains("[workflow-sdk] watch out"));
    assert!(out.contains("foo"));
});
row_test!(wf04_logger_row_003, {
    assert!(runtime_logger().info("quiet", None).is_none());
    assert!(runtime_logger().debug("quieter", None).is_none());
});
row_test!(wf04_logger_row_004, {
    let child = runtime_logger().child(meta([("workflowRunId", json!("run-1"))]));
    let out = child.error("boom", Some(meta([("stepId", json!("step-1"))])));
    assert!(out.contains("run-1"));
    assert!(out.contains("step-1"));
});
row_test!(wf04_logger_row_005, {
    let child = runtime_logger().child(meta([("workflowRunId", json!("parent-id"))]));
    let out = child.error("boom", Some(meta([("workflowRunId", json!("override"))])));
    assert!(out.contains("override"));
    assert!(!out.contains("parent-id"));
});
row_test!(wf04_logger_row_006, {
    let out = runtime_logger()
        .child(meta([("workflowRunId", json!("run-1"))]))
        .child(meta([("stepId", json!("step-1"))]))
        .error("boom", None);
    assert!(out.contains("run-1"));
    assert!(out.contains("step-1"));
});
row_test!(wf04_logger_row_007, {
    let out = runtime_logger()
        .for_run("run-1", Some("workflow//./src/jobs//myWorkflow"), None)
        .error("boom", None);
    assert!(out.contains("run-1"));
    assert!(out.contains("myWorkflow (./src/jobs)"));
});
row_test!(wf04_logger_row_008, {
    let out = runtime_logger()
        .for_run("run-1", None, None)
        .error("boom", None);
    assert!(out.contains("run-1"));
});
row_test!(wf04_logger_row_009, {
    let out = runtime_logger()
        .for_run(
            "run-1",
            Some("myWorkflow"),
            Some(meta([("stepId", json!("step-1"))])),
        )
        .error("boom", None);
    assert!(out.contains("run-1"));
    assert!(out.contains("step-1"));
});
row_test!(wf04_logger_row_010, {
    assert_eq!(runtime_logger().error("boom", None), "[workflow-sdk] boom");
});
row_test!(wf04_logger_row_011, {
    let log = runtime_logger()
        .for_run("wrun_123", Some("workflow//my-wf"), None)
        .child(meta([
            ("stepId", json!("step_456")),
            ("stepName", json!("step//my-step")),
        ]));
    let out = log.error(
        "Step \"step//my-step\" threw a FatalError",
        Some(meta([
            ("errorAttribution", json!("user")),
            ("errorName", json!("FatalError")),
            ("errorMessage", json!("boom")),
            ("hint", json!("Move the call to a step function.")),
        ])),
    );
    assert!(out.contains("user error \u{00b7} FatalError"));
    assert!(out.contains("run    wrun_123"));
    assert!(out.contains("step   step_456"));
    assert!(out.contains("hint: Move the call to a step function."));
});
row_test!(wf04_logger_row_012, {
    let log = runtime_logger()
        .for_run("wrun_abc", Some("workflow//main"), None)
        .child(meta([
            ("stepId", json!("step_xyz")),
            ("stepName", json!("step//doWork")),
        ]));
    let out = log.error(
        "Step \"step//doWork\" hit max retries \u{2014} bubbling error thrown by your step to the parent workflow",
        Some(meta([
            ("attempt", json!(4)),
            ("retryCount", json!(3)),
            ("errorAttribution", json!("user")),
            ("errorName", json!("Error")),
            ("errorMessage", json!("Transient failure")),
        ])),
    );
    assert!(out.contains("retry  4 attempts \u{00b7} 3 max retries"));
});

row_test!(wf04_schemas_row_001, {
    let mut trace_carrier = BTreeMap::new();
    trace_carrier.insert(
        "traceparent".to_string(),
        "00-00000000000000000000000000000000-0000000000000000-00".to_string(),
    );
    trace_carrier.insert(
        "tracestate".to_string(),
        "ro=00000000000000000000000000000000".to_string(),
    );
    let payload = WorkflowInvokePayload {
        run_id: "test-run-123".to_string(),
        trace_carrier: Some(trace_carrier),
    };
    assert_eq!(payload.run_id, "test-run-123");
    assert!(payload.trace_carrier.is_some());
});
row_test!(wf04_schemas_row_002, {
    let payload = StepInvokePayload {
        workflow_run_id: "test-run-123".to_string(),
        workflow_started_at: 123,
        workflow_name: "test-workflow".to_string(),
        step_id: "test-step".to_string(),
    };
    assert_eq!(payload.workflow_run_id, "test-run-123");
    assert_eq!(payload.step_id, "test-step");
    assert_eq!(payload.workflow_name, "test-workflow");
});

static SET_ATTRIBUTES_LOCK: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct RecordingWorld {
    name: Option<String>,
    supports: bool,
    calls: Mutex<
        Vec<(
            String,
            Vec<AttributeChange>,
            ExperimentalSetAttributesOptions,
        )>,
    >,
}

impl RecordingWorld {
    fn supported(name: Option<&str>) -> Self {
        Self {
            name: name.map(ToOwned::to_owned),
            supports: true,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn unsupported(name: Option<&str>) -> Self {
        Self {
            name: name.map(ToOwned::to_owned),
            supports: false,
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl AttributeWorld for RecordingWorld {
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    fn supports_experimental_set_attributes(&self) -> bool {
        self.supports
    }

    fn experimental_set_attributes(
        &self,
        run_id: &str,
        changes: &[AttributeChange],
        options: ExperimentalSetAttributesOptions,
    ) -> Result<(), WorkflowError> {
        self.calls
            .lock()
            .unwrap()
            .push((run_id.to_string(), changes.to_vec(), options));
        Ok(())
    }
}

fn reset_attribute_globals() {
    clear_attribute_world();
    reset_unsupported_world_warning();
}

row_test!(wf04_set_attributes_row_001, {
    let _guard = SET_ATTRIBUTES_LOCK.lock().unwrap();
    reset_attribute_globals();
    let mut attrs = BTreeMap::new();
    attrs.insert("phase".to_string(), Some("init".to_string()));
    let err = experimental_set_attributes(attrs, ExperimentalSetAttributesOptions::default())
        .unwrap_err();
    assert_eq!(err.name(), "FatalError");
    assert!(err.message().contains("workflow"));
    assert!(err.message().contains("step"));
});
row_test!(wf04_set_attributes_row_002, {
    let _guard = SET_ATTRIBUTES_LOCK.lock().unwrap();
    reset_attribute_globals();
    let world = Arc::new(RecordingWorld::supported(Some("test-world")));
    set_attribute_world(world.clone());
    let mut attrs = BTreeMap::new();
    attrs.insert("phase".to_string(), Some("ready".to_string()));
    attrs.insert("stale".to_string(), None);
    let outcome = with_step_context(
        StepContext {
            workflow_run_id: "run_123".to_string(),
        },
        || experimental_set_attributes(attrs, ExperimentalSetAttributesOptions::default()),
    )
    .unwrap();
    assert_eq!(outcome, ExperimentalSetAttributesOutcome::Posted);
    let calls = world.calls.lock().unwrap();
    assert_eq!(calls[0].0, "run_123");
    assert_eq!(
        calls[0].1,
        vec![
            AttributeChange {
                key: "phase".to_string(),
                value: Some("ready".to_string())
            },
            AttributeChange {
                key: "stale".to_string(),
                value: None
            }
        ]
    );
    reset_attribute_globals();
});
row_test!(wf04_set_attributes_row_003, {
    let _guard = SET_ATTRIBUTES_LOCK.lock().unwrap();
    reset_attribute_globals();
    let world = Arc::new(RecordingWorld::supported(None));
    set_attribute_world(world.clone());
    let mut attrs = BTreeMap::new();
    attrs.insert("$agent.kind".to_string(), Some("durable-agent".to_string()));
    with_step_context(
        StepContext {
            workflow_run_id: "run_123".to_string(),
        },
        || {
            experimental_set_attributes(
                attrs,
                ExperimentalSetAttributesOptions {
                    allow_reserved_attributes: true,
                },
            )
        },
    )
    .unwrap();
    let calls = world.calls.lock().unwrap();
    assert_eq!(calls[0].2.allow_reserved_attributes, true);
    reset_attribute_globals();
});
row_test!(wf04_set_attributes_row_004, {
    let _guard = SET_ATTRIBUTES_LOCK.lock().unwrap();
    reset_attribute_globals();
    let world = Arc::new(RecordingWorld::supported(None));
    set_attribute_world(world.clone());
    let mut attrs = BTreeMap::new();
    attrs.insert("$sys".to_string(), Some("x".to_string()));
    let err = with_step_context(
        StepContext {
            workflow_run_id: "run_123".to_string(),
        },
        || experimental_set_attributes(attrs, ExperimentalSetAttributesOptions::default()),
    )
    .unwrap_err();
    assert_eq!(err.name(), "FatalError");
    assert!(world.calls.lock().unwrap().is_empty());
    reset_attribute_globals();
});
row_test!(wf04_set_attributes_row_005, {
    let _guard = SET_ATTRIBUTES_LOCK.lock().unwrap();
    reset_attribute_globals();
    let world = Arc::new(RecordingWorld::unsupported(Some("legacy-world")));
    set_attribute_world(world);
    let mut attrs = BTreeMap::new();
    attrs.insert("phase".to_string(), Some("ignored".to_string()));
    let first = with_step_context(
        StepContext {
            workflow_run_id: "run_123".to_string(),
        },
        || experimental_set_attributes(attrs, ExperimentalSetAttributesOptions::default()),
    )
    .unwrap();
    let mut attrs = BTreeMap::new();
    attrs.insert("phase".to_string(), Some("ignored-again".to_string()));
    let second = with_step_context(
        StepContext {
            workflow_run_id: "run_123".to_string(),
        },
        || experimental_set_attributes(attrs, ExperimentalSetAttributesOptions::default()),
    )
    .unwrap();
    match first {
        ExperimentalSetAttributesOutcome::UnsupportedWorld {
            warning_emitted,
            warning: Some(warning),
        } => {
            assert!(warning_emitted);
            assert!(warning.contains("legacy-world"));
        }
        other => panic!("unexpected outcome: {other:?}"),
    }
    match second {
        ExperimentalSetAttributesOutcome::UnsupportedWorld {
            warning_emitted,
            warning,
        } => {
            assert!(!warning_emitted);
            assert!(warning.is_none());
        }
        other => panic!("unexpected outcome: {other:?}"),
    }
    reset_attribute_globals();
});

row_test!(wf04_source_map_row_001, {
    let source_map = "eyJ2ZXJzaW9uIjozLCJzb3VyY2VzIjpbIndvcmtmbG93cy9leGFtcGxlLnRzIl0sIm5hbWVzIjpbXSwibWFwcGluZ3MiOiIifQ==";
    let workflow_code = format!(
        "const padding = \"{}\";\n//# sourceMappingURL=data:application/json;base64,{source_map}",
        "a".repeat(1024 * 1024)
    );
    let stack = "Error: boom\n    at example (workflow.js:1:1)";
    assert_eq!(
        remap_error_stack(stack, "workflow.js", &workflow_code),
        stack
    );
});

row_test!(wf04_types_row_001, {
    let error = ErrorLike::new("AbortError", "cancelled");
    assert!(is_abort_error(&error));
});
row_test!(wf04_types_row_002, {
    let error = ErrorLike::new("AbortError", "serialized abort");
    assert!(is_abort_error(&error));
});
row_test!(wf04_types_row_003, {
    let error = ErrorLike::new("AbortError", "dom abort");
    assert!(is_abort_error(&error));
});
row_test!(wf04_types_row_004, {
    assert!(!is_abort_error_shape(&UnknownErrorShape::default()));
});
row_test!(wf04_types_row_005, {
    assert!(!is_abort_error_shape(&UnknownErrorShape::default()));
});
row_test!(wf04_types_row_006, {
    assert!(!is_abort_error(&ErrorLike::new(
        "TypeError",
        "not an abort"
    )));
});
row_test!(wf04_types_row_007, {
    assert!(!is_abort_error_shape(&UnknownErrorShape {
        name: Some(UnknownField::String("AbortError".to_string())),
        message: None,
        stack: None,
    }));
});
row_test!(wf04_types_row_008, {
    assert!(!is_abort_error_shape(&UnknownErrorShape {
        name: Some(UnknownField::String("AbortError".to_string())),
        message: Some(UnknownField::Number(42)),
        stack: None,
    }));
});
row_test!(wf04_types_row_009, {
    assert!(!is_abort_error_shape(&UnknownErrorShape {
        name: Some(UnknownField::String("AbortError".to_string())),
        message: Some(UnknownField::String("bad stack".to_string())),
        stack: Some(UnknownField::Number(42)),
    }));
});
row_test!(wf04_types_row_010, {
    let error = ErrorLike::new("AbortError", "cancelled").with_stack("AbortError: cancelled");
    let promoted = promote_abort_error_to_fatal(error);
    assert_eq!(promoted.name, "FatalError");
    assert_eq!(promoted.message, "Aborted: cancelled");
    assert_eq!(promoted.stack.as_deref(), Some("AbortError: cancelled"));
    assert!(promoted.fatal);
});
row_test!(wf04_types_row_011, {
    let error = ErrorLike::new("AbortError", "already fatal").with_fatal(true);
    assert_eq!(promote_abort_error_to_fatal(error.clone()), error);
});

row_test!(wf04_utility_row_001, {
    assert_eq!(build_workflow_suspension_message(0, 0, 0), None);
});
row_test!(wf04_utility_row_002, {
    assert_eq!(
        build_workflow_suspension_message(1, 0, 0).unwrap(),
        "1 step to be enqueued\n  Workflow will suspend and resume when steps are completed"
    );
});
row_test!(wf04_utility_row_003, {
    assert_eq!(
        build_workflow_suspension_message(3, 0, 0).unwrap(),
        "3 steps to be enqueued\n  Workflow will suspend and resume when steps are completed"
    );
});
row_test!(wf04_utility_row_004, {
    assert_eq!(
        build_workflow_suspension_message(0, 1, 0).unwrap(),
        "1 hook to be enqueued\n  Workflow will suspend and resume when hooks are received"
    );
});
row_test!(wf04_utility_row_005, {
    assert_eq!(
        build_workflow_suspension_message(0, 2, 0).unwrap(),
        "2 hooks to be enqueued\n  Workflow will suspend and resume when hooks are received"
    );
});
row_test!(wf04_utility_row_006, {
    assert_eq!(
        build_workflow_suspension_message(1, 1, 0).unwrap(),
        "1 step and 1 hook to be enqueued\n  Workflow will suspend and resume when steps are completed and hooks are received"
    );
});
row_test!(wf04_utility_row_007, {
    assert_eq!(
        build_workflow_suspension_message(5, 1, 0).unwrap(),
        "5 steps and 1 hook to be enqueued\n  Workflow will suspend and resume when steps are completed and hooks are received"
    );
});
row_test!(wf04_utility_row_008, {
    assert_eq!(
        build_workflow_suspension_message(1, 3, 0).unwrap(),
        "1 step and 3 hooks to be enqueued\n  Workflow will suspend and resume when steps are completed and hooks are received"
    );
});
row_test!(wf04_utility_row_009, {
    assert_eq!(
        build_workflow_suspension_message(4, 2, 0).unwrap(),
        "4 steps and 2 hooks to be enqueued\n  Workflow will suspend and resume when steps are completed and hooks are received"
    );
});
row_test!(wf04_utility_row_010, {
    assert_eq!(
        build_workflow_suspension_message(100, 50, 0).unwrap(),
        "100 steps and 50 hooks to be enqueued\n  Workflow will suspend and resume when steps are completed and hooks are received"
    );
});
row_test!(wf04_utility_row_011, {
    assert_eq!(
        build_workflow_suspension_message(0, 0, 1).unwrap(),
        "1 timer to be enqueued\n  Workflow will suspend and resume when timers have elapsed"
    );
});
row_test!(wf04_utility_row_012, {
    assert_eq!(
        build_workflow_suspension_message(0, 0, 2).unwrap(),
        "2 timers to be enqueued\n  Workflow will suspend and resume when timers have elapsed"
    );
});
row_test!(wf04_utility_row_013, {
    assert_eq!(
        build_workflow_suspension_message(0, 1, 1).unwrap(),
        "1 hook and 1 timer to be enqueued\n  Workflow will suspend and resume when hooks are received and timers have elapsed"
    );
});
row_test!(wf04_utility_row_014, {
    assert_eq!(
        build_workflow_suspension_message(1, 0, 1).unwrap(),
        "1 step and 1 timer to be enqueued\n  Workflow will suspend and resume when steps are completed and timers have elapsed"
    );
});
row_test!(wf04_utility_row_015, {
    assert_eq!(
        build_workflow_suspension_message(1, 1, 1).unwrap(),
        "1 step and 1 hook and 1 timer to be enqueued\n  Workflow will suspend and resume when steps are completed and hooks are received and timers have elapsed"
    );
});
row_test!(wf04_utility_row_016, {
    assert_eq!(
        build_workflow_suspension_message(2, 1, 3).unwrap(),
        "2 steps and 1 hook and 3 timers to be enqueued\n  Workflow will suspend and resume when steps are completed and hooks are received and timers have elapsed"
    );
});
row_test!(wf04_utility_row_017, {
    assert_eq!(
        get_workflow_run_stream_id("wrun_abc123", None),
        "strm_abc123_user"
    );
});
row_test!(wf04_utility_row_018, {
    assert_eq!(
        get_workflow_run_stream_id("wrun_abc123", Some("my-namespace")),
        "strm_abc123_user_bXktbmFtZXNwYWNl"
    );
});
row_test!(wf04_utility_row_019, {
    assert_eq!(
        get_workflow_run_stream_id("wrun_xyz789", Some("namespace:with/special@chars")),
        "strm_xyz789_user_bmFtZXNwYWNlOndpdGgvc3BlY2lhbEBjaGFycw"
    );
});
row_test!(wf04_utility_row_020, {
    assert_eq!(
        get_workflow_run_stream_id("wrun_test", Some("my namespace with spaces")),
        "strm_test_user_bXkgbmFtZXNwYWNlIHdpdGggc3BhY2Vz"
    );
});
row_test!(wf04_utility_row_021, {
    assert_eq!(
        get_workflow_run_stream_id("wrun_test", Some("namespace-with-\u{e9}mojis-\u{1f389}")),
        "strm_test_user_bmFtZXNwYWNlLXdpdGgtw6ltb2ppcy3wn46J"
    );
});
row_test!(wf04_utility_row_022, {
    assert!(get_workflow_run_stream_id("wrun_abc123", None).starts_with("strm_"));
});
row_test!(wf04_utility_row_023, {
    assert!(get_workflow_run_stream_id("wrun_abc123", None).contains("_user"));
});
