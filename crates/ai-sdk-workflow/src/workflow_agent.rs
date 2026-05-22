use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use ai_sdk_provider::json::JsonValue;
use ai_sdk_provider::{
    LanguageModelMessage, LanguageModelToolResultOutput, LanguageModelToolResultPart,
};
use ai_sdk_provider_utils::{
    ExecuteToolOutput, Tool, ToolExecutionOptions, ToolModelOutputOptions, execute_tool,
};
use serde::{Deserialize, Serialize};

use crate::{
    ParsedToolCall, ProviderExecutedToolResult, StreamTextIterator, WorkflowGenerationSettings,
    WorkflowModelInfo, WorkflowPrompt, WorkflowRuntimeContext, WorkflowStreamStep,
    WorkflowStreamTextError, WorkflowStreamTextStepExecutor, WorkflowToolsContext,
};

/// Constructor options for [`WorkflowAgent`].
#[derive(Clone, Debug)]
pub struct WorkflowAgentOptions {
    /// Agent identifier exposed to callers.
    pub id: Option<String>,

    /// Default model identity for this agent.
    pub model: WorkflowModelInfo,

    /// Runtime tools available to the agent.
    pub tools: BTreeMap<String, Tool>,

    /// Default generation settings.
    pub generation_settings: WorkflowGenerationSettings,

    /// Default active tools list.
    pub active_tools: Option<Vec<String>>,

    /// Default serialized tool-choice value.
    pub tool_choice: Option<JsonValue>,
}

impl WorkflowAgentOptions {
    /// Creates workflow-agent options with a default model.
    pub fn new(model: WorkflowModelInfo) -> Self {
        Self {
            id: None,
            model,
            tools: BTreeMap::new(),
            generation_settings: WorkflowGenerationSettings::default(),
            active_tools: None,
            tool_choice: None,
        }
    }

    /// Sets the optional agent id.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Adds one runtime tool.
    pub fn with_tool(mut self, tool: Tool) -> Self {
        self.tools.insert(tool.name.clone(), tool);
        self
    }

    /// Adds runtime tools.
    pub fn with_tools(mut self, tools: impl IntoIterator<Item = Tool>) -> Self {
        self.tools
            .extend(tools.into_iter().map(|tool| (tool.name.clone(), tool)));
        self
    }

    /// Sets constructor-level generation settings.
    pub fn with_generation_settings(
        mut self,
        generation_settings: WorkflowGenerationSettings,
    ) -> Self {
        self.generation_settings = generation_settings;
        self
    }

    /// Sets constructor-level active tools.
    pub fn with_active_tools(mut self, active_tools: impl IntoIterator<Item = String>) -> Self {
        self.active_tools = Some(active_tools.into_iter().collect());
        self
    }

    /// Sets constructor-level tool choice.
    pub fn with_tool_choice(mut self, tool_choice: impl Into<JsonValue>) -> Self {
        self.tool_choice = Some(tool_choice.into());
        self
    }
}

/// Deterministic Rust equivalent of upstream `WorkflowAgent`.
#[derive(Clone, Debug)]
pub struct WorkflowAgent {
    id: Option<String>,
    model: WorkflowModelInfo,
    tools: BTreeMap<String, Tool>,
    generation_settings: WorkflowGenerationSettings,
    active_tools: Option<Vec<String>>,
    tool_choice: Option<JsonValue>,
}

impl WorkflowAgent {
    /// Creates a workflow agent.
    pub fn new(options: WorkflowAgentOptions) -> Self {
        Self {
            id: options.id,
            model: options.model,
            tools: options.tools,
            generation_settings: options.generation_settings,
            active_tools: options.active_tools,
            tool_choice: options.tool_choice,
        }
    }

    /// Returns the optional agent id.
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// Returns the default model identity.
    pub fn model(&self) -> &WorkflowModelInfo {
        &self.model
    }

    /// Returns the configured runtime tools.
    pub fn tools(&self) -> &BTreeMap<String, Tool> {
        &self.tools
    }

    /// Runs the agent loop with a supplied deterministic stream-step executor.
    pub async fn stream<E>(
        &self,
        options: WorkflowAgentStreamOptions<E>,
    ) -> Result<WorkflowAgentStreamResult, WorkflowAgentError>
    where
        E: WorkflowStreamTextStepExecutor,
    {
        let generation_settings = options
            .generation_settings
            .unwrap_or_else(|| self.generation_settings.clone());
        let active_tools = options
            .active_tools
            .or_else(|| self.active_tools.clone())
            .unwrap_or_default();
        let tool_choice = options.tool_choice.or_else(|| self.tool_choice.clone());

        let mut iterator = StreamTextIterator::from_runtime_tools(
            options.prompt,
            self.tools.values().cloned(),
            options.executor,
        )
        .with_generation_settings(generation_settings)
        .with_runtime_context(options.runtime_context)
        .with_tools_context(options.tools_context);

        if !active_tools.is_empty() {
            iterator = iterator.with_active_tools(active_tools);
        }
        if let Some(tool_choice) = tool_choice {
            iterator = iterator.with_tool_choice(tool_choice);
        }

        let mut pending_tool_results = None;
        let mut steps = Vec::new();
        let mut messages = iterator.prompt().to_vec();
        let mut runtime_context = WorkflowRuntimeContext::new();
        let mut tools_context = WorkflowToolsContext::new();
        let mut last_tool_calls = Vec::new();
        let mut last_tool_results = Vec::new();
        let mut missing_provider_executed_tool_results = Vec::new();

        while let Some(yield_value) = iterator
            .next(pending_tool_results.take())
            .map_err(WorkflowAgentError::Stream)?
        {
            steps.push(yield_value.step.clone());
            messages = yield_value.messages.clone();
            runtime_context = yield_value.runtime_context.clone();
            tools_context = yield_value.tools_context.clone();

            if yield_value.tool_calls.is_empty() {
                last_tool_calls.clear();
                last_tool_results.clear();
                continue;
            }

            let execution = self.execute_tool_calls(&yield_value).await?;
            missing_provider_executed_tool_results
                .extend(execution.missing_provider_executed_tool_results);

            last_tool_calls = yield_value.tool_calls.clone();
            last_tool_results = execution.tool_results.clone();

            if execution.has_unresolved_client_tools {
                break;
            }

            pending_tool_results = Some(execution.tool_results);
        }

        Ok(WorkflowAgentStreamResult {
            messages,
            steps,
            tool_calls: last_tool_calls,
            tool_results: last_tool_results,
            runtime_context,
            tools_context,
            missing_provider_executed_tool_results,
        })
    }

    async fn execute_tool_calls(
        &self,
        yield_value: &crate::StreamTextIteratorYieldValue,
    ) -> Result<WorkflowAgentToolExecution, WorkflowAgentError> {
        let mut execution = WorkflowAgentToolExecution::default();

        for tool_call in &yield_value.tool_calls {
            if tool_call.provider_executed == Some(true) {
                execution.tool_results.push(provider_executed_tool_result(
                    tool_call,
                    yield_value
                        .provider_executed_tool_results
                        .get(&tool_call.tool_call_id),
                    &mut execution.missing_provider_executed_tool_results,
                ));
                continue;
            }

            if tool_call.invalid == Some(true) {
                execution
                    .tool_results
                    .push(LanguageModelToolResultPart::new(
                        tool_call.tool_call_id.clone(),
                        tool_call.tool_name.clone(),
                        LanguageModelToolResultOutput::error_text(
                            tool_call.error.clone().unwrap_or_else(|| {
                                format!("Invalid input for tool {}", tool_call.tool_name)
                            }),
                        ),
                    ));
                continue;
            }

            let Some(tool) = self.tools.get(&tool_call.tool_name) else {
                execution.has_unresolved_client_tools = true;
                continue;
            };

            if !tool.is_executable() {
                execution.has_unresolved_client_tools = true;
                continue;
            }

            let tool_result = execute_local_tool(
                tool,
                tool_call,
                yield_value.messages.clone(),
                &yield_value.tools_context,
            )
            .await?;
            execution.tool_results.push(tool_result);
        }

        Ok(execution)
    }
}

/// Per-call options for [`WorkflowAgent::stream`].
#[derive(Clone, Debug)]
pub struct WorkflowAgentStreamOptions<E> {
    /// Initial prompt.
    pub prompt: WorkflowPrompt,

    /// Deterministic stream-step executor.
    pub executor: E,

    /// Stream-level generation settings that override constructor defaults.
    pub generation_settings: Option<WorkflowGenerationSettings>,

    /// Stream-level runtime context.
    pub runtime_context: WorkflowRuntimeContext,

    /// Stream-level per-tool context.
    pub tools_context: WorkflowToolsContext,

    /// Stream-level active tools that override constructor defaults.
    pub active_tools: Option<Vec<String>>,

    /// Stream-level tool choice that overrides constructor defaults.
    pub tool_choice: Option<JsonValue>,
}

impl<E> WorkflowAgentStreamOptions<E> {
    /// Creates agent stream options.
    pub fn new(prompt: WorkflowPrompt, executor: E) -> Self {
        Self {
            prompt,
            executor,
            generation_settings: None,
            runtime_context: WorkflowRuntimeContext::new(),
            tools_context: WorkflowToolsContext::new(),
            active_tools: None,
            tool_choice: None,
        }
    }

    /// Sets stream-level generation settings.
    pub fn with_generation_settings(
        mut self,
        generation_settings: WorkflowGenerationSettings,
    ) -> Self {
        self.generation_settings = Some(generation_settings);
        self
    }

    /// Sets runtime context.
    pub fn with_runtime_context(mut self, runtime_context: WorkflowRuntimeContext) -> Self {
        self.runtime_context = runtime_context;
        self
    }

    /// Sets per-tool context.
    pub fn with_tools_context(mut self, tools_context: WorkflowToolsContext) -> Self {
        self.tools_context = tools_context;
        self
    }

    /// Sets stream-level active tools.
    pub fn with_active_tools(mut self, active_tools: impl IntoIterator<Item = String>) -> Self {
        self.active_tools = Some(active_tools.into_iter().collect());
        self
    }

    /// Sets stream-level tool choice.
    pub fn with_tool_choice(mut self, tool_choice: impl Into<JsonValue>) -> Self {
        self.tool_choice = Some(tool_choice.into());
        self
    }
}

/// Result returned by [`WorkflowAgent::stream`].
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowAgentStreamResult {
    /// Final conversation messages observed by the agent loop.
    pub messages: Vec<LanguageModelMessage>,

    /// Completed stream steps.
    pub steps: Vec<WorkflowStreamStep>,

    /// Last unresolved or executed tool calls observed by the loop.
    pub tool_calls: Vec<ParsedToolCall>,

    /// Tool results generated by the last tool-call round.
    pub tool_results: Vec<LanguageModelToolResultPart>,

    /// Final runtime context.
    pub runtime_context: WorkflowRuntimeContext,

    /// Final per-tool context.
    pub tools_context: WorkflowToolsContext,

    /// Provider-executed tool calls that had no matching provider result.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_provider_executed_tool_results: Vec<String>,
}

/// Error returned by workflow-agent execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkflowAgentError {
    /// Stream iterator failed.
    Stream(WorkflowStreamTextError),

    /// A tool-specific context failed validation.
    InvalidToolContext {
        /// Tool whose context failed validation.
        tool_name: String,

        /// Validation message.
        message: String,
    },
}

impl fmt::Display for WorkflowAgentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stream(error) => write!(formatter, "{error}"),
            Self::InvalidToolContext { tool_name, message } => {
                write!(
                    formatter,
                    "invalid context for tool '{tool_name}': {message}"
                )
            }
        }
    }
}

impl Error for WorkflowAgentError {}

#[derive(Clone, Debug, Default)]
struct WorkflowAgentToolExecution {
    tool_results: Vec<LanguageModelToolResultPart>,
    has_unresolved_client_tools: bool,
    missing_provider_executed_tool_results: Vec<String>,
}

async fn execute_local_tool(
    tool: &Tool,
    tool_call: &ParsedToolCall,
    messages: WorkflowPrompt,
    tools_context: &WorkflowToolsContext,
) -> Result<LanguageModelToolResultPart, WorkflowAgentError> {
    let context = validated_tool_context(tool, tool_call, tools_context)?;
    let mut options = ToolExecutionOptions::new(tool_call.tool_call_id.clone(), messages);
    if let Some(context) = context {
        options = options.with_context(context);
    }

    let output = match execute_tool(tool, tool_call.input.clone(), options.clone()).await {
        Ok(outputs) => {
            let raw_output = final_tool_output(outputs).unwrap_or(JsonValue::Null);
            if let Some(model_output) = tool.model_output(ToolModelOutputOptions::new(
                tool_call.tool_call_id.clone(),
                tool_call.input.clone(),
                raw_output.clone(),
            )) {
                model_output.await
            } else {
                json_value_to_tool_result_output(raw_output)
            }
        }
        Err(error) => LanguageModelToolResultOutput::error_text(error.into_message()),
    };

    Ok(LanguageModelToolResultPart::new(
        tool_call.tool_call_id.clone(),
        tool_call.tool_name.clone(),
        output,
    ))
}

fn validated_tool_context(
    tool: &Tool,
    tool_call: &ParsedToolCall,
    tools_context: &WorkflowToolsContext,
) -> Result<Option<JsonValue>, WorkflowAgentError> {
    let context = tools_context
        .get(&tool_call.tool_name)
        .cloned()
        .flatten()
        .map(JsonValue::Object);

    let Some(context_schema) = tool.context_schema() else {
        return Ok(context);
    };

    let value = context.clone().unwrap_or(JsonValue::Null);
    let schema = context_schema.clone().into_schema();
    if let Some(result) = schema.validate(&value) {
        return result.into_result().map(Some).map_err(|message| {
            WorkflowAgentError::InvalidToolContext {
                tool_name: tool_call.tool_name.clone(),
                message,
            }
        });
    }

    Ok(context)
}

fn final_tool_output(outputs: Vec<ExecuteToolOutput>) -> Option<JsonValue> {
    outputs.into_iter().rev().find_map(|output| match output {
        ExecuteToolOutput::Final { output } => Some(output),
        ExecuteToolOutput::Preliminary { .. } => None,
    })
}

fn json_value_to_tool_result_output(value: JsonValue) -> LanguageModelToolResultOutput {
    match value {
        JsonValue::String(value) => LanguageModelToolResultOutput::text(value),
        value => LanguageModelToolResultOutput::json(value),
    }
}

fn provider_executed_tool_result(
    tool_call: &ParsedToolCall,
    result: Option<&ProviderExecutedToolResult>,
    missing_provider_executed_tool_results: &mut Vec<String>,
) -> LanguageModelToolResultPart {
    let Some(result) = result else {
        missing_provider_executed_tool_results.push(tool_call.tool_call_id.clone());
        return LanguageModelToolResultPart::new(
            tool_call.tool_call_id.clone(),
            tool_call.tool_name.clone(),
            LanguageModelToolResultOutput::text(""),
        );
    };

    let output = match (result.is_error == Some(true), &result.result) {
        (true, JsonValue::String(value)) => {
            LanguageModelToolResultOutput::error_text(value.clone())
        }
        (true, value) => LanguageModelToolResultOutput::error_json(value.clone()),
        (false, JsonValue::String(value)) => LanguageModelToolResultOutput::text(value.clone()),
        (false, value) => LanguageModelToolResultOutput::json(value.clone()),
    };

    LanguageModelToolResultPart::new(
        tool_call.tool_call_id.clone(),
        tool_call.tool_name.clone(),
        output,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};

    use ai_sdk_provider::json::JsonObject;
    use ai_sdk_provider::{
        FinishReason, LanguageModelAssistantContentPart, LanguageModelAssistantMessage,
        LanguageModelFinishReason, LanguageModelStreamFinish, LanguageModelStreamPart,
        LanguageModelToolCall, LanguageModelUsage, LanguageModelUserContentPart,
        LanguageModelUserMessage, OutputTokenUsage,
    };
    use ai_sdk_provider_utils::{Schema, ToolExecutionError, ValidationResult};
    use serde_json::json;

    use crate::{DoStreamStepOutput, ScriptedStreamTextStepExecutor, do_stream_step_from_parts};

    fn model() -> WorkflowModelInfo {
        WorkflowModelInfo::new("test", "test-model")
    }

    fn object_schema() -> JsonObject {
        serde_json::from_value(json!({
            "type": "object",
            "additionalProperties": true
        }))
        .expect("schema is an object")
    }

    fn user_prompt() -> WorkflowPrompt {
        vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
            vec![LanguageModelUserContentPart::Text(
                ai_sdk_provider::LanguageModelTextPart::new("test"),
            )],
        ))]
    }

    fn usage() -> LanguageModelUsage {
        LanguageModelUsage {
            input_tokens: Default::default(),
            output_tokens: OutputTokenUsage {
                total: Some(5),
                text: Some(5),
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
        step_number: usize,
    ) -> DoStreamStepOutput {
        do_stream_step_from_parts(
            parts,
            crate::DoStreamStepOptions {
                step_number,
                ..crate::DoStreamStepOptions::default()
            },
        )
    }

    fn tool_call_step(tool_call: LanguageModelToolCall) -> DoStreamStepOutput {
        output_from_parts(
            [
                LanguageModelStreamPart::ToolCall(tool_call),
                finish(FinishReason::ToolCalls),
            ],
            0,
        )
    }

    fn stop_step() -> DoStreamStepOutput {
        output_from_parts([finish(FinishReason::Stop)], 1)
    }

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        struct NoopWake;

        impl Wake for NoopWake {
            fn wake(self: Arc<Self>) {}
        }

        let waker = Waker::from(Arc::new(NoopWake));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);
        match Future::poll(Pin::as_mut(&mut future), &mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("future unexpectedly pending"),
        }
    }

    fn first_tool_result(
        tool_message: &ai_sdk_provider::LanguageModelToolMessage,
    ) -> &LanguageModelToolResultPart {
        match &tool_message.content[0] {
            ai_sdk_provider::LanguageModelToolContentPart::ToolResult(tool_result) => tool_result,
            ai_sdk_provider::LanguageModelToolContentPart::ToolApprovalResponse(_) => {
                panic!("expected tool result")
            }
        }
    }

    #[test]
    fn workflow_agent_upstream_should_expose_id_when_provided_in_constructor() {
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_id("my-agent"));

        assert_eq!(agent.id(), Some("my-agent"));
        assert_eq!(agent.model(), &model());
    }

    #[test]
    fn workflow_agent_upstream_should_have_undefined_id_when_not_provided() {
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));

        assert_eq!(agent.id(), None);
    }

    #[test]
    fn workflow_agent_upstream_should_convert_tool_execution_error_to_error_text_result() {
        let tool = Tool::new("testTool", object_schema())
            .with_execute(|_, _| async { Err(ToolExecutionError::new("This is a generic error")) });
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let executor = ScriptedStreamTextStepExecutor::new([
            tool_call_step(LanguageModelToolCall::new("test-call-id", "testTool", "{}")),
            stop_step(),
        ]);

        let result =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
                .expect("agent stream succeeds");

        assert_eq!(result.steps.len(), 2);
        assert_eq!(result.tool_calls, Vec::<ParsedToolCall>::new());
        let prompt = result.messages;
        let tool_message = prompt
            .iter()
            .find_map(|message| match message {
                LanguageModelMessage::Tool(message) => Some(message),
                _ => None,
            })
            .expect("tool result message is appended");
        assert_eq!(tool_message.content.len(), 1);
        let tool_result = first_tool_result(tool_message);
        assert_eq!(tool_result.tool_call_id, "test-call-id");
        assert_eq!(tool_result.tool_name, "testTool");
        assert_eq!(
            tool_result.output,
            LanguageModelToolResultOutput::error_text("This is a generic error")
        );
    }

    #[test]
    fn workflow_agent_upstream_should_successfully_execute_tools_that_return_normally() {
        let tool = Tool::new("testTool", object_schema()).with_execute(|_, _| async {
            Ok(json!({
                "success": true,
                "data": "test result"
            }))
        });
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let executor = ScriptedStreamTextStepExecutor::new([
            tool_call_step(LanguageModelToolCall::new("test-call-id", "testTool", "{}")),
            stop_step(),
        ]);

        let result =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
                .expect("agent stream succeeds");

        let tool_message = result
            .messages
            .iter()
            .find_map(|message| match message {
                LanguageModelMessage::Tool(message) => Some(message),
                _ => None,
            })
            .expect("tool result message is appended");
        let tool_result = first_tool_result(tool_message);
        assert_eq!(
            tool_result.output,
            LanguageModelToolResultOutput::json(json!({
                "success": true,
                "data": "test result"
            }))
        );
    }

    #[test]
    fn workflow_agent_upstream_should_skip_local_execution_for_provider_executed_tools() {
        let execute_calls = Arc::new(Mutex::new(0usize));
        let execute_calls_for_tool = Arc::clone(&execute_calls);
        let tool = Tool::new("localTool", object_schema()).with_execute(move |_, _| {
            let execute_calls = Arc::clone(&execute_calls_for_tool);
            async move {
                *execute_calls.lock().expect("counter lock succeeds") += 1;
                Ok(json!("should not run"))
            }
        });
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let mut first = tool_call_step(
            LanguageModelToolCall::new("provider-call-id", "WebSearch", r#"{"query":"test"}"#)
                .with_provider_executed(true),
        );
        first.provider_executed_tool_results.insert(
            "provider-call-id".to_string(),
            ProviderExecutedToolResult {
                tool_call_id: "provider-call-id".to_string(),
                tool_name: "WebSearch".to_string(),
                result: json!("Search results for: test query"),
                is_error: Some(false),
            },
        );
        let executor = ScriptedStreamTextStepExecutor::new([first, stop_step()]);

        let result =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
                .expect("agent stream succeeds");

        assert_eq!(*execute_calls.lock().expect("counter lock succeeds"), 0);
        let tool_message = result
            .messages
            .iter()
            .find_map(|message| match message {
                LanguageModelMessage::Tool(message) => Some(message),
                _ => None,
            })
            .expect("tool result message is appended");
        let tool_result = first_tool_result(tool_message);
        assert_eq!(tool_result.tool_call_id, "provider-call-id");
        assert_eq!(tool_result.tool_name, "WebSearch");
        assert_eq!(
            tool_result.output,
            LanguageModelToolResultOutput::text("Search results for: test query")
        );
    }

    #[test]
    fn workflow_agent_upstream_should_handle_provider_executed_tool_errors_with_is_error_flag() {
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));
        let mut first = tool_call_step(
            LanguageModelToolCall::new("provider-call-id", "WebSearch", r#"{"query":"test"}"#)
                .with_provider_executed(true),
        );
        first.provider_executed_tool_results.insert(
            "provider-call-id".to_string(),
            ProviderExecutedToolResult {
                tool_call_id: "provider-call-id".to_string(),
                tool_name: "WebSearch".to_string(),
                result: json!("Search failed: Rate limit exceeded"),
                is_error: Some(true),
            },
        );
        let executor = ScriptedStreamTextStepExecutor::new([first, stop_step()]);

        let result =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
                .expect("agent stream succeeds");

        let tool_message = result
            .messages
            .iter()
            .find_map(|message| match message {
                LanguageModelMessage::Tool(message) => Some(message),
                _ => None,
            })
            .expect("tool result message is appended");
        let tool_result = first_tool_result(tool_message);
        assert_eq!(
            tool_result.output,
            LanguageModelToolResultOutput::error_text("Search failed: Rate limit exceeded")
        );
    }

    #[test]
    fn workflow_agent_upstream_should_return_empty_result_when_provider_executed_tool_result_is_missing()
     {
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()));
        let executor = ScriptedStreamTextStepExecutor::new([
            tool_call_step(
                LanguageModelToolCall::new("missing-result-id", "WebSearch", r#"{"query":"test"}"#)
                    .with_provider_executed(true),
            ),
            stop_step(),
        ]);

        let result =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
                .expect("agent stream succeeds");

        assert_eq!(
            result.missing_provider_executed_tool_results,
            vec!["missing-result-id"]
        );
        let tool_message = result
            .messages
            .iter()
            .find_map(|message| match message {
                LanguageModelMessage::Tool(message) => Some(message),
                _ => None,
            })
            .expect("tool result message is appended");
        let tool_result = first_tool_result(tool_message);
        assert_eq!(tool_result.output, LanguageModelToolResultOutput::text(""));
    }

    #[test]
    fn workflow_agent_upstream_should_stop_the_loop_for_client_side_tools_without_execute() {
        let tool = Tool::new("askUser", object_schema());
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let executor = ScriptedStreamTextStepExecutor::new([tool_call_step(
            LanguageModelToolCall::new("ask-user-call-id", "askUser", r#"{"question":"Name?"}"#),
        )]);

        let result =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
                .expect("agent stream succeeds");

        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].tool_call_id, "ask-user-call-id");
        assert!(result.tool_results.is_empty());
    }

    #[test]
    fn workflow_agent_upstream_should_pass_conversation_messages_to_tool_execute_function() {
        let received_messages = Arc::new(Mutex::new(None));
        let received_tool_call_id = Arc::new(Mutex::new(None));
        let received_messages_for_tool = Arc::clone(&received_messages);
        let received_tool_call_id_for_tool = Arc::clone(&received_tool_call_id);
        let tool = Tool::new("testTool", object_schema()).with_execute(move |_, options| {
            let received_messages = Arc::clone(&received_messages_for_tool);
            let received_tool_call_id = Arc::clone(&received_tool_call_id_for_tool);
            async move {
                *received_messages.lock().expect("messages lock succeeds") = Some(options.messages);
                *received_tool_call_id
                    .lock()
                    .expect("tool call id lock succeeds") = Some(options.tool_call_id);
                Ok(json!({ "result": "success" }))
            }
        });
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let executor = ScriptedStreamTextStepExecutor::new([
            tool_call_step(LanguageModelToolCall::new(
                "test-call-id",
                "testTool",
                r#"{"query":"weather"}"#,
            )),
            stop_step(),
        ]);

        poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
            .expect("agent stream succeeds");

        assert_eq!(
            *received_tool_call_id
                .lock()
                .expect("tool call id lock succeeds"),
            Some("test-call-id".to_string())
        );
        let messages = received_messages
            .lock()
            .expect("messages lock succeeds")
            .clone()
            .expect("messages were passed");
        assert!(matches!(
            messages.last(),
            Some(LanguageModelMessage::Assistant(
                LanguageModelAssistantMessage { .. }
            ))
        ));
        let LanguageModelMessage::Assistant(assistant_message) =
            messages.last().expect("assistant message exists")
        else {
            unreachable!("last message checked above");
        };
        assert!(matches!(
            assistant_message.content.first(),
            Some(LanguageModelAssistantContentPart::ToolCall(tool_call))
                if tool_call.tool_call_id == "test-call-id"
        ));
    }

    #[test]
    fn workflow_agent_upstream_should_pass_per_tool_tools_context_entry_as_execute_context() {
        let received_context = Arc::new(Mutex::new(None));
        let received_context_for_tool = Arc::clone(&received_context);
        let tool = Tool::new("weather", object_schema()).with_execute(move |_, options| {
            let received_context = Arc::clone(&received_context_for_tool);
            async move {
                *received_context.lock().expect("context lock succeeds") = options.context;
                Ok(json!({ "result": "ok" }))
            }
        });
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let executor = ScriptedStreamTextStepExecutor::new([
            tool_call_step(LanguageModelToolCall::new("call-1", "weather", "{}")),
            stop_step(),
        ]);
        let mut tools_context = WorkflowToolsContext::new();
        tools_context.insert(
            "weather".to_string(),
            Some(
                serde_json::from_value(json!({
                    "weatherApiKey": "secret-key",
                    "defaultUnit": "celsius"
                }))
                .expect("context is an object"),
            ),
        );

        poll_ready(
            agent.stream(
                WorkflowAgentStreamOptions::new(user_prompt(), executor)
                    .with_tools_context(tools_context),
            ),
        )
        .expect("agent stream succeeds");

        assert_eq!(
            *received_context.lock().expect("context lock succeeds"),
            Some(json!({
                "weatherApiKey": "secret-key",
                "defaultUnit": "celsius"
            }))
        );
    }

    #[test]
    fn workflow_agent_upstream_should_validate_per_tool_context_against_context_schema() {
        let schema = Schema::new(object_schema()).with_validator(|value| {
            if value.get("apiKey").and_then(JsonValue::as_str).is_some() {
                ValidationResult::success(value.clone())
            } else {
                ValidationResult::failure("apiKey is required")
            }
        });
        let tool = Tool::new("weather", object_schema())
            .with_context_schema(schema)
            .with_execute(|_, _| async { Ok(json!({ "result": "ok" })) });
        let agent = WorkflowAgent::new(WorkflowAgentOptions::new(model()).with_tool(tool));
        let executor = ScriptedStreamTextStepExecutor::new([tool_call_step(
            LanguageModelToolCall::new("call-1", "weather", "{}"),
        )]);

        let error =
            poll_ready(agent.stream(WorkflowAgentStreamOptions::new(user_prompt(), executor)))
                .expect_err("missing tool context fails");

        assert_eq!(
            error,
            WorkflowAgentError::InvalidToolContext {
                tool_name: "weather".to_string(),
                message: "apiKey is required".to_string()
            }
        );
    }
}
