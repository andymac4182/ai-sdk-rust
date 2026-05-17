use std::cell::RefCell;
use std::collections::BTreeMap;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

use ai_sdk_rust::{
    FinishReason, GenerateTextOptions, InputTokenUsage, LanguageModel, LanguageModelCallOptions,
    LanguageModelContent, LanguageModelFinishReason, LanguageModelGenerateResult,
    LanguageModelMessage, LanguageModelRequest, LanguageModelResponse, LanguageModelStreamPart,
    LanguageModelStreamResult, LanguageModelSupportedUrls, LanguageModelText,
    LanguageModelToolCall, LanguageModelToolContentPart, LanguageModelToolResultOutput,
    LanguageModelUsage, OutputTokenUsage, Prompt, Tool, generate_text,
};
use serde_json::{Value, json};

fn main() {
    let model = DeterministicToolLoopModel::new();
    let input_schema = json_schema(json!({
        "type": "object",
        "properties": {
            "city": { "type": "string" }
        },
        "required": ["city"]
    }));

    let result = poll_ready(generate_text(
        GenerateTextOptions::from_prompt(
            &model,
            Prompt::from_prompt("What is the weather in Brisbane?")
                .with_instructions("Use tools when current weather is requested."),
        )
        .expect("prompt should standardize")
        .with_temperature(0.0)
        .with_seed(42)
        .with_tool(
            Tool::new("weather", input_schema)
                .with_description("Look up current weather for a city.")
                .with_execute(|input, options| async move {
                    Ok(json!({
                        "city": input["city"],
                        "forecast": "sunny",
                        "toolCallId": options.tool_call_id
                    }))
                })
                .with_to_model_output(|options| async move {
                    LanguageModelToolResultOutput::json(json!({
                        "city": options.output["city"],
                        "forecast": options.output["forecast"]
                    }))
                }),
        )
        .with_max_steps(2),
    ));

    println!("text: {}", result.text);
    println!("finish reason: {:?}", result.finish_reason);
    println!("model calls: {}", model.calls.borrow().len());
    println!("tool calls: {}", result.tool_calls.len());
    println!("tool results: {}", result.tool_results.len());
    println!("response messages: {}", result.response_messages.len());
    println!(
        "final response id: {}",
        result
            .response
            .as_ref()
            .and_then(|response| response.id.as_deref())
            .unwrap_or("missing")
    );
}

struct DeterministicToolLoopModel {
    calls: RefCell<Vec<LanguageModelCallOptions>>,
}

impl DeterministicToolLoopModel {
    fn new() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
        }
    }

    fn forecast_from_prompt(prompt: &[LanguageModelMessage]) -> String {
        prompt
            .iter()
            .filter_map(|message| match message {
                LanguageModelMessage::Tool(message) => Some(&message.content),
                _ => None,
            })
            .flatten()
            .find_map(|part| match part {
                LanguageModelToolContentPart::ToolResult(part) => match &part.output {
                    LanguageModelToolResultOutput::Json { value, .. } => value
                        .get("forecast")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    LanguageModelToolResultOutput::Text { value, .. } => Some(value.clone()),
                    _ => None,
                },
                LanguageModelToolContentPart::ToolApprovalResponse(_) => None,
            })
            .unwrap_or_else(|| "unknown".to_string())
    }
}

impl LanguageModel for DeterministicToolLoopModel {
    type SupportedUrlsFuture<'a>
        = Ready<LanguageModelSupportedUrls>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = Ready<LanguageModelGenerateResult>
    where
        Self: 'a;

    type Stream = Vec<LanguageModelStreamPart>;

    type StreamFuture<'a>
        = Ready<LanguageModelStreamResult<Self::Stream>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        "example"
    }

    fn model_id(&self) -> &str {
        "deterministic-tool-loop"
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        ready(BTreeMap::new())
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        let step_number = self.calls.borrow().len();
        let prompt = options.prompt.clone();
        self.calls.borrow_mut().push(options);

        if step_number == 0 {
            return ready(
                LanguageModelGenerateResult::new(
                    vec![LanguageModelContent::ToolCall(LanguageModelToolCall::new(
                        "call_weather_1",
                        "weather",
                        r#"{"city":"Brisbane"}"#,
                    ))],
                    LanguageModelFinishReason {
                        unified: FinishReason::ToolCalls,
                        raw: Some("tool_calls".to_string()),
                    },
                    LanguageModelUsage {
                        input_tokens: InputTokenUsage {
                            total: Some(12),
                            ..InputTokenUsage::default()
                        },
                        output_tokens: OutputTokenUsage {
                            total: Some(1),
                            ..OutputTokenUsage::default()
                        },
                        raw: None,
                    },
                )
                .with_request(LanguageModelRequest::new().with_messages(prompt))
                .with_response(LanguageModelResponse::new().with_id("response_tool_call")),
            );
        }

        let forecast = Self::forecast_from_prompt(&prompt);
        ready(
            LanguageModelGenerateResult::new(
                vec![LanguageModelContent::Text(LanguageModelText::new(format!(
                    "The weather in Brisbane is {forecast}."
                )))],
                LanguageModelFinishReason {
                    unified: FinishReason::Stop,
                    raw: Some("stop".to_string()),
                },
                LanguageModelUsage {
                    input_tokens: InputTokenUsage {
                        total: Some(24),
                        ..InputTokenUsage::default()
                    },
                    output_tokens: OutputTokenUsage {
                        total: Some(8),
                        text: Some(8),
                        ..OutputTokenUsage::default()
                    },
                    raw: None,
                },
            )
            .with_request(LanguageModelRequest::new().with_messages(prompt))
            .with_response(LanguageModelResponse::new().with_id("response_final")),
        )
    }

    fn do_stream(&self, _options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        ready(LanguageModelStreamResult::new(Vec::new()))
    }
}

fn json_schema(value: Value) -> ai_sdk_rust::JsonSchema {
    value
        .as_object()
        .expect("schema should be a JSON object")
        .clone()
}

fn poll_ready<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    let mut future = Box::pin(future);

    match Pin::new(&mut future).poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => unreachable!("example uses only ready futures"),
    }
}
