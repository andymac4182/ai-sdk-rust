//! Standalone Rust port of upstream `@workflow/ai`.
//!
//! This crate owns the `vercel/workflow` `packages/ai` surface. It bridges to
//! already ported Rust AI SDK/chat-sdk-compatible internals where those
//! contracts already exist, while keeping the Workflow SDK package boundary
//! separate from the Open Agents oriented `ai-sdk-workflow` crate.

#![forbid(unsafe_code)]

pub mod error_message;
pub mod provider_bridge;
pub mod stream_iterator;

use ai_sdk_provider::LanguageModelTool;
use ai_sdk_provider_utils::Tool;

pub use ai_sdk_provider::LanguageModelMessage as ModelMessage;
pub use ai_sdk_workflow::stream_text_iterator::{
    normalize_finish_reason, safe_parse_tool_call_input,
};
pub use ai_sdk_workflow::{
    ApprovalAction, ApprovalClassification, ApprovalReason, CancellationStatus,
    DoStreamStepOptions, DoStreamStepOutput, DurableRunAgent, DurableRunAgentContext,
    DurableRunAgentError, DurableRunAgentOutput, DurableRunEngine, DurableRunError,
    DurableRunEvent, DurableRunEventPayload, DurableRunExecution, DurableRunPause,
    DurableRunRecord, DurableRunResume, DurableRunStartOptions, DurableRunState, DurableRunStore,
    InMemoryDurableRunStore, ParsedToolCall, ProviderExecutedToolResult, REDACTED_VALUE,
    RateLimitCheck, RateLimitDecision, RateLimitScope, ReconnectToStreamOptions, Redacted,
    RedactionPolicy, RunEventMetadata, RunEventRecord, RunLifecycleEventKind,
    ScriptedStreamTextStepCall, ScriptedStreamTextStepExecutor, SendMessagesOptions,
    SerializableToolDef, SerializableToolError, SerializableToolSet, SerializableToolType,
    StreamFinish, StreamTextIterator, StreamTextIteratorYieldValue, ToolEventInput,
    ToolEventRecord, ToolLifecycleEventKind, UsageAccountingEvent, UsageAggregation, VERSION,
    WorkflowAgent, WorkflowAgentError, WorkflowAgentFinishInfo, WorkflowAgentOnFinishCallback,
    WorkflowAgentOnStartCallback, WorkflowAgentOnStepFinishCallback,
    WorkflowAgentOnStepStartCallback, WorkflowAgentOnToolExecutionEndCallback,
    WorkflowAgentOnToolExecutionStartCallback, WorkflowAgentOptions, WorkflowAgentStartInfo,
    WorkflowAgentStepStartInfo, WorkflowAgentStreamOptions, WorkflowAgentStreamResult,
    WorkflowAgentToolExecutionEndInfo, WorkflowAgentToolExecutionStartInfo, WorkflowChatEnd,
    WorkflowChatRequest, WorkflowChatRequestMethod, WorkflowChatResponse, WorkflowChatTransport,
    WorkflowChatTransportClient, WorkflowChatTransportError, WorkflowChatTransportOptions,
    WorkflowChatTransportResult, WorkflowChatTrigger, WorkflowGenerationSettings,
    WorkflowModelInfo, WorkflowPrepareStepCallback, WorkflowPrepareStepInfo,
    WorkflowPrepareStepResult, WorkflowPrompt, WorkflowPromptInput, WorkflowRuntimeContext,
    WorkflowStreamStep, WorkflowStreamStepContent, WorkflowStreamTextError,
    WorkflowStreamTextOnErrorCallback, WorkflowStreamTextStepExecutor,
    WorkflowToolCallRepairCallback, WorkflowToolsContext, add_language_model_usage,
    cancellation_status, classify_tool_approval, decide_rate_limit, is_cancellation_requested,
    model_call_stream_to_ui_chunks, operational_telemetry_fields, redact_json_value, redact_text,
    resolve_serializable_tools, serialize_tool_set, to_ui_message_chunk,
};
pub use error_message::{WorkflowErrorLike, get_error_message};
pub use provider_bridge::{
    MockWorkflowModelFactory, MockWorkflowModelResponse, StaticWorkflowModelFactory,
    WorkflowModelFactory, anthropic, gateway, google, mock_sequence_model, mock_text_model, openai,
    xai,
};
pub use stream_iterator::{
    IteratorStream, StreamIteratorEnvironment, StreamIteratorError, iterator_to_stream,
    stream_to_iterator,
};

/// Upstream repository used for this crate boundary.
pub const UPSTREAM_REPOSITORY: &str = "github.com/vercel/workflow";

/// Upstream ref fetched into the local source mirror.
pub const UPSTREAM_REF: &str = "main";

/// Remote HEAD recorded by the workflow inventory generator.
pub const UPSTREAM_HEAD: &str = "ae3c833acd4f44ab84db65b44eb2ba2646eaecf9";

/// Upstream package owned by this crate.
pub const UPSTREAM_PACKAGE: &str = "@workflow/ai";

/// Upstream package version inventoried for this implementation.
pub const UPSTREAM_VERSION: &str = "5.0.0-beta.6";

/// Rust crate version compiled into the library.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Converts runtime workflow tools into provider-facing language-model tools.
///
/// This is the Rust equivalent of upstream `toolsToModelTools`: function and
/// dynamic tools become function tools, while provider tools preserve their
/// provider id and args.
pub fn tools_to_model_tools(tools: impl IntoIterator<Item = Tool>) -> Vec<LanguageModelTool> {
    tools
        .into_iter()
        .map(|tool| tool.to_language_model_tool())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_sdk_provider::json::JsonObject;
    use ai_sdk_provider::{
        FinishReason, LanguageModelFinishReason, LanguageModelStreamFinish,
        LanguageModelStreamPart, LanguageModelTextDelta, LanguageModelToolCall, LanguageModelUsage,
        LanguageModelUserContentPart, LanguageModelUserMessage, OutputTokenUsage, ProviderOptions,
    };
    use ai_sdk_provider_utils::Tool;
    use serde_json::json;

    fn object_schema() -> JsonObject {
        serde_json::from_value(json!({
            "type": "object",
            "additionalProperties": true
        }))
        .expect("schema is an object")
    }

    fn user_prompt() -> WorkflowPrompt {
        vec![ModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(ai_sdk_provider::LanguageModelTextPart::new(
                "hello",
            )),
        ]))]
    }

    fn usage() -> LanguageModelUsage {
        LanguageModelUsage {
            input_tokens: Default::default(),
            output_tokens: OutputTokenUsage {
                total: Some(1),
                text: Some(1),
                reasoning: None,
            },
            raw: None,
        }
    }

    fn finish(reason: FinishReason) -> LanguageModelStreamPart {
        LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
            usage(),
            LanguageModelFinishReason {
                unified: reason,
                raw: None,
            },
        ))
    }

    fn output_from_parts(
        parts: impl IntoIterator<Item = LanguageModelStreamPart>,
    ) -> DoStreamStepOutput {
        ai_sdk_workflow::do_stream_step_from_parts(
            parts,
            DoStreamStepOptions {
                step_number: 0,
                ..DoStreamStepOptions::default()
            },
        )
    }

    #[test]
    fn do_stream_step_upstream_normalize_finish_reason_matches_strings_objects_and_edges() {
        for reason in [
            "stop",
            "tool-calls",
            "length",
            "content-filter",
            "error",
            "other",
        ] {
            assert_eq!(normalize_finish_reason(Some(&json!(reason))), reason);
            assert_eq!(
                normalize_finish_reason(Some(
                    &json!({ "type": reason, "metadata": { "foo": "bar" } })
                )),
                reason
            );
        }

        assert_eq!(
            normalize_finish_reason(Some(&json!({ "type": "unknown" }))),
            "other"
        );
        assert_eq!(normalize_finish_reason(Some(&json!({}))), "other");
        assert_eq!(
            normalize_finish_reason(Some(&json!({ "type": null }))),
            "other"
        );
        assert_eq!(normalize_finish_reason(None), "other");
        assert_eq!(normalize_finish_reason(Some(&json!(null))), "other");
        assert_eq!(normalize_finish_reason(Some(&json!(42))), "other");
        assert_eq!(normalize_finish_reason(Some(&json!(true))), "other");
        assert_eq!(normalize_finish_reason(Some(&json!(["stop"]))), "other");
        assert_eq!(normalize_finish_reason(Some(&json!(""))), "");
    }

    #[test]
    fn do_stream_step_upstream_safe_parse_tool_call_input_parses_missing_and_malformed_inputs() {
        assert_eq!(
            safe_parse_tool_call_input(Some(r#"{"city":"San Francisco"}"#)),
            json!({ "city": "San Francisco" })
        );
        assert_eq!(safe_parse_tool_call_input(None), json!({}));
        assert_eq!(safe_parse_tool_call_input(Some("")), json!({}));
        assert_eq!(
            safe_parse_tool_call_input(Some(r#"{"city":"San Francisco""#)),
            json!(r#"{"city":"San Francisco""#)
        );
    }

    #[test]
    fn do_stream_step_upstream_should_not_throw_when_streamed_tool_call_input_is_malformed_json() {
        let output = output_from_parts([
            LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                "call-1",
                "getWeather",
                r#"{"city":"San Francisco""#,
            )),
            finish(FinishReason::ToolCalls),
        ]);

        assert_eq!(output.tool_calls[0].invalid, Some(true));
    }

    #[test]
    fn workflow_ai_facade_exercises_stream_text_iterator_bridge() {
        let executor = ScriptedStreamTextStepExecutor::new([
            output_from_parts([
                LanguageModelStreamPart::ToolCall(
                    LanguageModelToolCall::new("call-1", "testTool", "{}").with_provider_metadata(
                        serde_json::from_value(json!({
                            "openai": {
                                "itemId": "fc_preserved"
                            }
                        }))
                        .expect("provider metadata"),
                    ),
                ),
                finish(FinishReason::ToolCalls),
            ]),
            output_from_parts([
                LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", "done")),
                finish(FinishReason::Stop),
            ]),
        ]);
        let mut iterator =
            StreamTextIterator::new(user_prompt(), SerializableToolSet::new(), executor);

        iterator.next(None).expect("first step succeeds");
        let second = iterator
            .next(Some(vec![
                ai_sdk_provider::LanguageModelToolResultPart::new(
                    "call-1",
                    "testTool",
                    ai_sdk_provider::LanguageModelToolResultOutput::json(json!({ "ok": true })),
                ),
            ]))
            .expect("second step succeeds")
            .expect("yield");

        assert_eq!(second.step.text, "done");
        let prompt = &iterator.executor().calls()[1].prompt;
        let assistant = prompt
            .iter()
            .find_map(|message| match message {
                ModelMessage::Assistant(message) => Some(message),
                _ => None,
            })
            .expect("assistant message");
        assert!(matches!(
            assistant.content[0],
            ai_sdk_provider::LanguageModelAssistantContentPart::ToolCall(_)
        ));
    }

    #[test]
    fn workflow_ai_facade_exercises_durable_agent_bridge() {
        let agent = WorkflowAgent::new(
            WorkflowAgentOptions::new(WorkflowModelInfo::new("test", "test-model")).with_tool(
                Tool::new("testTool", object_schema())
                    .with_execute(|_, _| async { Ok(json!("tool-result")) }),
            ),
        );
        let executor = ScriptedStreamTextStepExecutor::new([
            output_from_parts([
                LanguageModelStreamPart::ToolCall(LanguageModelToolCall::new(
                    "call-1", "testTool", "{}",
                )),
                finish(FinishReason::ToolCalls),
            ]),
            output_from_parts([finish(FinishReason::Stop)]),
        ]);

        let result = futures_executor::block_on(
            agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)),
        )
        .expect("agent stream succeeds");

        assert_eq!(result.steps.len(), 2);
        assert!(result.tool_calls.is_empty());
    }

    #[test]
    fn tools_to_model_tools_upstream_serializes_function_tools_with_description_and_input_schema() {
        let model_tools = tools_to_model_tools([
            Tool::new("weather", object_schema()).with_description("Get the weather")
        ]);

        let LanguageModelTool::Function(tool) = &model_tools[0] else {
            panic!("expected function tool");
        };
        assert_eq!(tool.name, "weather");
        assert_eq!(tool.description.as_deref(), Some("Get the weather"));
        assert_eq!(tool.input_schema, object_schema());
    }

    #[test]
    fn tools_to_model_tools_upstream_preserves_provider_tool_type_id_and_args() {
        let args = serde_json::from_value(json!({ "maxUses": 5 })).expect("args");
        let model_tools = tools_to_model_tools([Tool::provider_tool(
            "webSearch",
            "anthropic.web_search",
            args,
            object_schema(),
            true,
        )]);

        assert_eq!(model_tools.len(), 1);
        let LanguageModelTool::Provider(tool) = &model_tools[0] else {
            panic!("expected provider tool");
        };
        assert_eq!(tool.name, "webSearch");
        assert_eq!(tool.id, "anthropic.web_search");
        assert_eq!(
            tool.args,
            serde_json::from_value(json!({ "maxUses": 5 })).expect("args")
        );
    }

    #[test]
    fn tools_to_model_tools_upstream_handles_mixed_function_and_provider_tools() {
        let model_tools = tools_to_model_tools([
            Tool::new("weather", object_schema()),
            Tool::provider_tool(
                "webSearch",
                "anthropic.web_search",
                JsonObject::new(),
                object_schema(),
                true,
            ),
        ]);

        assert_eq!(model_tools.len(), 2);
        assert!(matches!(model_tools[0], LanguageModelTool::Function(_)));
        assert!(matches!(model_tools[1], LanguageModelTool::Provider(_)));
    }

    #[test]
    fn tools_to_model_tools_upstream_defaults_args_to_empty_object_when_not_provided_on_provider_tool()
     {
        let model_tools = tools_to_model_tools([Tool::provider_tool(
            "codeExec",
            "anthropic.code_execution",
            JsonObject::new(),
            object_schema(),
            true,
        )]);

        let LanguageModelTool::Provider(tool) = &model_tools[0] else {
            panic!("expected provider tool");
        };
        assert_eq!(tool.args, JsonObject::new());
    }

    #[test]
    fn tools_to_model_tools_upstream_forwards_strict_true() {
        let model_tools =
            tools_to_model_tools([Tool::new("weather", object_schema()).with_strict(true)]);

        let LanguageModelTool::Function(tool) = &model_tools[0] else {
            panic!("expected function tool");
        };
        assert_eq!(tool.strict, Some(true));
    }

    #[test]
    fn tools_to_model_tools_upstream_forwards_strict_false() {
        let model_tools =
            tools_to_model_tools([Tool::new("weather", object_schema()).with_strict(false)]);

        let LanguageModelTool::Function(tool) = &model_tools[0] else {
            panic!("expected function tool");
        };
        assert_eq!(tool.strict, Some(false));
    }

    #[test]
    fn tools_to_model_tools_upstream_omits_strict_key_when_not_set() {
        let model_tools = tools_to_model_tools([Tool::new("weather", object_schema())]);

        let LanguageModelTool::Function(tool) = &model_tools[0] else {
            panic!("expected function tool");
        };
        assert_eq!(tool.strict, None);
    }

    #[test]
    fn tools_to_model_tools_upstream_forwards_input_examples() {
        let example: JsonObject =
            serde_json::from_value(json!({ "location": "Tokyo" })).expect("example");
        let model_tools = tools_to_model_tools([
            Tool::new("weather", object_schema()).with_input_example(example.clone())
        ]);

        let LanguageModelTool::Function(tool) = &model_tools[0] else {
            panic!("expected function tool");
        };
        assert_eq!(
            tool.input_examples.as_ref().expect("examples")[0].input,
            example
        );
    }

    #[test]
    fn tools_to_model_tools_upstream_omits_input_examples_key_when_not_set() {
        let model_tools = tools_to_model_tools([Tool::new("weather", object_schema())]);

        let LanguageModelTool::Function(tool) = &model_tools[0] else {
            panic!("expected function tool");
        };
        assert_eq!(tool.input_examples, None);
    }

    #[test]
    fn tools_to_model_tools_upstream_forwards_provider_options() {
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "parallelToolCalls": false
            }
        }))
        .expect("provider options");
        let model_tools =
            tools_to_model_tools([Tool::new("weather", object_schema())
                .with_provider_options(provider_options.clone())]);

        let LanguageModelTool::Function(tool) = &model_tools[0] else {
            panic!("expected function tool");
        };
        assert_eq!(tool.provider_options, Some(provider_options));
    }

    #[test]
    fn tools_to_model_tools_upstream_handles_tools_with_type_dynamic_as_function_tools() {
        let model_tools = tools_to_model_tools([
            Tool::dynamic("dynamic", object_schema()).with_description("A dynamic tool")
        ]);

        let LanguageModelTool::Function(tool) = &model_tools[0] else {
            panic!("expected function tool");
        };
        assert_eq!(tool.name, "dynamic");
        assert_eq!(tool.description.as_deref(), Some("A dynamic tool"));
    }

    #[test]
    fn tools_to_model_tools_upstream_returns_empty_array_for_empty_tools() {
        assert!(tools_to_model_tools(Vec::<Tool>::new()).is_empty());
    }

    #[test]
    fn workflow_ai_facade_exercises_workflow_chat_transport_bridge() {
        let transport = WorkflowChatTransport::new()
            .with_api("/custom/chat")
            .with_max_consecutive_errors(5)
            .with_initial_start_index(-10);

        let request =
            transport.reconnect_request(&ReconnectToStreamOptions::new("chat-1"), Some("run-1"), 7);

        assert_eq!(transport.api(), "/custom/chat");
        assert_eq!(transport.max_consecutive_errors(), 5);
        assert_eq!(transport.initial_start_index(), -10);
        assert_eq!(request.url, "/custom/chat/run-1/stream?startIndex=7");
    }
}
