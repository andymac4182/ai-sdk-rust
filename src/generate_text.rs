use serde::{Deserialize, Serialize};

use crate::headers::Headers;
use crate::language_model::{
    FinishReason, LanguageModel, LanguageModelCallOptions, LanguageModelContent,
    LanguageModelFinishReason, LanguageModelGenerateResult, LanguageModelPrompt,
    LanguageModelReasoningEffort, LanguageModelRequest, LanguageModelResponse,
    LanguageModelResponseFormat, LanguageModelText, LanguageModelTool, LanguageModelToolChoice,
    LanguageModelUsage,
};
use crate::provider::{ProviderMetadata, ProviderOptions};
use crate::provider_utils::{Tool, prepare_tools};
use crate::warning::Warning;

/// Tool input accepted by [`GenerateTextOptions::with_tool`].
#[derive(Clone, Debug)]
pub enum GenerateTextTool {
    /// High-level Rust function tool.
    Rust(Tool),

    /// Already prepared provider-facing language model tool.
    LanguageModel(LanguageModelTool),
}

impl From<Tool> for GenerateTextTool {
    fn from(tool: Tool) -> Self {
        Self::Rust(tool)
    }
}

impl From<LanguageModelTool> for GenerateTextTool {
    fn from(tool: LanguageModelTool) -> Self {
        Self::LanguageModel(tool)
    }
}

/// Options for a high-level non-streaming text generation call.
#[derive(Debug)]
pub struct GenerateTextOptions<'a, M: LanguageModel + ?Sized> {
    /// Language model used for the generation.
    pub model: &'a M,

    /// Provider-level call options sent to the model.
    pub call_options: LanguageModelCallOptions,

    /// High-level Rust tools made available to the model.
    pub tools: Vec<Tool>,
}

impl<'a, M: LanguageModel + ?Sized> GenerateTextOptions<'a, M> {
    /// Creates generation options for a model and standardized prompt.
    pub fn new(model: &'a M, prompt: LanguageModelPrompt) -> Self {
        Self {
            model,
            call_options: LanguageModelCallOptions::new(prompt),
            tools: Vec::new(),
        }
    }

    /// Creates generation options from already prepared provider call options.
    pub fn from_call_options(model: &'a M, call_options: LanguageModelCallOptions) -> Self {
        Self {
            model,
            call_options,
            tools: Vec::new(),
        }
    }

    /// Sets the maximum number of output tokens.
    pub fn with_max_output_tokens(mut self, max_output_tokens: u64) -> Self {
        self.call_options.max_output_tokens = Some(max_output_tokens);
        self
    }

    /// Sets the sampling temperature.
    pub fn with_temperature(mut self, temperature: f64) -> Self {
        self.call_options.temperature = Some(temperature);
        self
    }

    /// Adds a stop sequence.
    pub fn with_stop_sequence(mut self, stop_sequence: impl Into<String>) -> Self {
        self.call_options
            .stop_sequences
            .get_or_insert_with(Vec::new)
            .push(stop_sequence.into());
        self
    }

    /// Sets nucleus sampling.
    pub fn with_top_p(mut self, top_p: f64) -> Self {
        self.call_options.top_p = Some(top_p);
        self
    }

    /// Sets top-k sampling.
    pub fn with_top_k(mut self, top_k: u64) -> Self {
        self.call_options.top_k = Some(top_k);
        self
    }

    /// Sets the presence penalty.
    pub fn with_presence_penalty(mut self, presence_penalty: f64) -> Self {
        self.call_options.presence_penalty = Some(presence_penalty);
        self
    }

    /// Sets the frequency penalty.
    pub fn with_frequency_penalty(mut self, frequency_penalty: f64) -> Self {
        self.call_options.frequency_penalty = Some(frequency_penalty);
        self
    }

    /// Sets the response format.
    pub fn with_response_format(mut self, response_format: LanguageModelResponseFormat) -> Self {
        self.call_options.response_format = Some(response_format);
        self
    }

    /// Sets the deterministic sampling seed.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.call_options.seed = Some(seed);
        self
    }

    /// Adds a tool that is available to the model.
    pub fn with_tool(mut self, tool: impl Into<GenerateTextTool>) -> Self {
        match tool.into() {
            GenerateTextTool::Rust(tool) => self.tools.push(tool),
            GenerateTextTool::LanguageModel(tool) => self
                .call_options
                .tools
                .get_or_insert_with(Vec::new)
                .push(tool),
        }

        self
    }

    /// Sets the tool selection strategy.
    pub fn with_tool_choice(mut self, tool_choice: LanguageModelToolChoice) -> Self {
        self.call_options.tool_choice = Some(tool_choice);
        self
    }

    /// Sets whether raw stream chunks should be included.
    pub fn with_include_raw_chunks(mut self, include_raw_chunks: bool) -> Self {
        self.call_options.include_raw_chunks = Some(include_raw_chunks);
        self
    }

    /// Adds an HTTP header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.call_options
            .headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }

    /// Sets the reasoning effort.
    pub fn with_reasoning(mut self, reasoning: LanguageModelReasoningEffort) -> Self {
        self.call_options.reasoning = Some(reasoning);
        self
    }

    /// Adds provider-specific options.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.call_options.provider_options = Some(provider_options);
        self
    }
}

/// Information about the model that produced a generate-text step.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateTextModelInfo {
    /// Provider identifier.
    pub provider: String,

    /// Provider-specific model id.
    pub model_id: String,
}

impl GenerateTextModelInfo {
    /// Creates model information for a step result.
    pub fn new(provider: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model_id: model_id.into(),
        }
    }
}

/// Result of a single non-streaming generate-text step.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateTextStep {
    /// Zero-based index of this step.
    pub step_number: usize,

    /// Model that produced this step.
    pub model: GenerateTextModelInfo,

    /// Content generated in this step.
    pub content: Vec<LanguageModelContent>,

    /// Text content generated in this step, formed by concatenating all text parts.
    pub text: String,

    /// Unified reason why this step finished.
    pub finish_reason: FinishReason,

    /// Raw provider finish reason, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_finish_reason: Option<String>,

    /// Usage reported for this step.
    pub usage: LanguageModelUsage,

    /// Warnings reported by the provider for this step.
    pub warnings: Vec<Warning>,

    /// Optional request information for telemetry and debugging.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<LanguageModelRequest>,

    /// Optional response information for telemetry and debugging.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<LanguageModelResponse>,

    /// Provider-specific metadata returned by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl GenerateTextStep {
    fn from_language_model_result(
        step_number: usize,
        model: GenerateTextModelInfo,
        result: LanguageModelGenerateResult,
    ) -> Self {
        let LanguageModelGenerateResult {
            content,
            finish_reason:
                LanguageModelFinishReason {
                    unified,
                    raw: raw_finish_reason,
                },
            usage,
            provider_metadata,
            request,
            response,
            warnings,
        } = result;

        let text = extract_text(&content);

        Self {
            step_number,
            model,
            content,
            text,
            finish_reason: unified,
            raw_finish_reason,
            usage,
            warnings,
            request,
            response,
            provider_metadata,
        }
    }
}

/// Result of a high-level non-streaming text generation call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateTextResult {
    /// Content generated across all steps.
    pub content: Vec<LanguageModelContent>,

    /// Text generated in the final step.
    pub text: String,

    /// Unified reason why the final step finished.
    pub finish_reason: FinishReason,

    /// Raw provider finish reason from the final step, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_finish_reason: Option<String>,

    /// Total usage across all steps.
    pub usage: LanguageModelUsage,

    /// Warnings reported across all steps.
    pub warnings: Vec<Warning>,

    /// Optional request information from the final step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<LanguageModelRequest>,

    /// Optional response information from the final step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<LanguageModelResponse>,

    /// Provider-specific metadata from the final step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Details for all generation steps.
    pub steps: Vec<GenerateTextStep>,
}

impl GenerateTextResult {
    fn from_steps(steps: Vec<GenerateTextStep>) -> Self {
        let final_step = steps
            .last()
            .expect("generate_text always creates at least one step");

        Self {
            content: steps
                .iter()
                .flat_map(|step| step.content.iter().cloned())
                .collect(),
            text: final_step.text.clone(),
            finish_reason: final_step.finish_reason.clone(),
            raw_finish_reason: final_step.raw_finish_reason.clone(),
            usage: final_step.usage.clone(),
            warnings: steps
                .iter()
                .flat_map(|step| step.warnings.iter().cloned())
                .collect(),
            request: final_step.request.clone(),
            response: final_step.response.clone(),
            provider_metadata: final_step.provider_metadata.clone(),
            steps,
        }
    }

    /// Returns the final step, when the result contains at least one step.
    pub fn final_step(&self) -> Option<&GenerateTextStep> {
        self.steps.last()
    }
}

/// Runs a non-streaming text generation call against a language model.
pub async fn generate_text<M: LanguageModel + ?Sized>(
    options: GenerateTextOptions<'_, M>,
) -> GenerateTextResult {
    let GenerateTextOptions {
        model,
        mut call_options,
        tools,
    } = options;
    let model_info = GenerateTextModelInfo::new(model.provider(), model.model_id());

    if let Some(mut prepared_tools) = prepare_tools(&tools) {
        call_options
            .tools
            .get_or_insert_with(Vec::new)
            .append(&mut prepared_tools);
    }

    let result = model.do_generate(call_options).await;
    let step = GenerateTextStep::from_language_model_result(0, model_info, result);

    GenerateTextResult::from_steps(vec![step])
}

fn extract_text(content: &[LanguageModelContent]) -> String {
    content
        .iter()
        .filter_map(|part| match part {
            LanguageModelContent::Text(LanguageModelText { text, .. }) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        GenerateTextModelInfo, GenerateTextOptions, GenerateTextResult, GenerateTextStep,
        generate_text,
    };
    use crate::language_model::{
        FinishReason, InputTokenUsage, LanguageModel, LanguageModelAssistantContentPart,
        LanguageModelCallOptions, LanguageModelContent, LanguageModelFinishReason,
        LanguageModelFunctionTool, LanguageModelGenerateResult, LanguageModelMessage,
        LanguageModelStreamPart, LanguageModelStreamResult, LanguageModelSupportedUrls,
        LanguageModelText, LanguageModelTextPart, LanguageModelTool, LanguageModelUsage,
        LanguageModelUserContentPart, LanguageModelUserMessage, OutputTokenUsage,
    };
    use crate::provider::SpecificationVersion;
    use crate::provider_utils::Tool;
    use serde_json::json;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    struct FakeLanguageModel {
        calls: RefCell<Vec<LanguageModelCallOptions>>,
    }

    impl FakeLanguageModel {
        fn new() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl LanguageModel for FakeLanguageModel {
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
            "test-provider"
        }

        fn model_id(&self) -> &str {
            "test-model"
        }

        fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
            ready(BTreeMap::new())
        }

        fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
            self.calls.borrow_mut().push(options);

            ready(LanguageModelGenerateResult::new(
                vec![
                    LanguageModelContent::Text(LanguageModelText::new("Hello ")),
                    LanguageModelContent::Text(LanguageModelText::new("world")),
                ],
                LanguageModelFinishReason {
                    unified: FinishReason::Stop,
                    raw: Some("stop".to_string()),
                },
                LanguageModelUsage {
                    input_tokens: InputTokenUsage {
                        total: Some(5),
                        ..InputTokenUsage::default()
                    },
                    output_tokens: OutputTokenUsage {
                        total: Some(2),
                        text: Some(2),
                        ..OutputTokenUsage::default()
                    },
                    raw: None,
                },
            ))
        }

        fn do_stream(&self, _options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
            ready(LanguageModelStreamResult::new(Vec::new()))
        }
    }

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("test futures should be ready"),
        }
    }

    fn user_message(text: &str) -> LanguageModelMessage {
        LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(LanguageModelTextPart::new(text)),
        ]))
    }

    #[test]
    fn generate_text_calls_language_model_and_returns_plain_text_result() {
        let model = FakeLanguageModel::new();
        let prompt = vec![user_message("Say hello")];

        let result = poll_ready(generate_text(
            GenerateTextOptions::new(&model, prompt.clone())
                .with_max_output_tokens(20)
                .with_temperature(0.2),
        ));

        assert_eq!(model.specification_version(), SpecificationVersion::V4);
        assert_eq!(model.calls.borrow().len(), 1);
        assert_eq!(model.calls.borrow()[0].prompt, prompt);
        assert_eq!(model.calls.borrow()[0].max_output_tokens, Some(20));
        assert_eq!(model.calls.borrow()[0].temperature, Some(0.2));

        assert_eq!(result.text, "Hello world");
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.raw_finish_reason.as_deref(), Some("stop"));
        assert_eq!(result.usage.input_tokens.total, Some(5));
        assert_eq!(result.usage.output_tokens.text, Some(2));
        assert_eq!(result.warnings, Vec::new());
        assert_eq!(result.content.len(), 2);
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.final_step().expect("step exists").step_number, 0);
        assert_eq!(
            result.final_step().expect("step exists").model,
            GenerateTextModelInfo::new("test-provider", "test-model")
        );
    }

    #[test]
    fn generate_text_result_serializes_as_camel_case_step_record() {
        let result = GenerateTextResult::from_steps(vec![GenerateTextStep {
            step_number: 0,
            model: GenerateTextModelInfo::new("test-provider", "test-model"),
            content: vec![LanguageModelContent::Text(LanguageModelText::new("Hello"))],
            text: "Hello".to_string(),
            finish_reason: FinishReason::Stop,
            raw_finish_reason: Some("stop".to_string()),
            usage: LanguageModelUsage {
                input_tokens: InputTokenUsage {
                    total: Some(3),
                    ..InputTokenUsage::default()
                },
                output_tokens: OutputTokenUsage {
                    total: Some(1),
                    ..OutputTokenUsage::default()
                },
                raw: None,
            },
            warnings: Vec::new(),
            request: None,
            response: None,
            provider_metadata: None,
        }]);

        assert_eq!(
            serde_json::to_value(result).expect("result serializes"),
            json!({
                "content": [
                    {
                        "type": "text",
                        "text": "Hello"
                    }
                ],
                "text": "Hello",
                "finishReason": "stop",
                "rawFinishReason": "stop",
                "usage": {
                    "inputTokens": {
                        "total": 3
                    },
                    "outputTokens": {
                        "total": 1
                    }
                },
                "warnings": [],
                "steps": [
                    {
                        "stepNumber": 0,
                        "model": {
                            "provider": "test-provider",
                            "modelId": "test-model"
                        },
                        "content": [
                            {
                                "type": "text",
                                "text": "Hello"
                            }
                        ],
                        "text": "Hello",
                        "finishReason": "stop",
                        "rawFinishReason": "stop",
                        "usage": {
                            "inputTokens": {
                                "total": 3
                            },
                            "outputTokens": {
                                "total": 1
                            }
                        },
                        "warnings": []
                    }
                ]
            })
        );
    }

    #[test]
    fn generate_text_result_deserializes_minimal_contract() {
        let result: GenerateTextResult = serde_json::from_value(json!({
            "content": [],
            "text": "",
            "finishReason": "length",
            "usage": {
                "inputTokens": {},
                "outputTokens": {}
            },
            "warnings": [],
            "steps": [
                {
                    "stepNumber": 0,
                    "model": {
                        "provider": "test-provider",
                        "modelId": "test-model"
                    },
                    "content": [],
                    "text": "",
                    "finishReason": "length",
                    "usage": {
                        "inputTokens": {},
                        "outputTokens": {}
                    },
                    "warnings": []
                }
            ]
        }))
        .expect("result deserializes");

        assert_eq!(result.text, "");
        assert_eq!(result.finish_reason, FinishReason::Length);
        assert_eq!(result.raw_finish_reason, None);
        assert_eq!(result.steps[0].raw_finish_reason, None);
        assert_eq!(
            result.steps[0].model,
            GenerateTextModelInfo::new("test-provider", "test-model")
        );
    }

    #[test]
    fn generate_text_concatenates_only_final_step_text_parts() {
        let step = GenerateTextStep::from_language_model_result(
            0,
            GenerateTextModelInfo::new("test-provider", "test-model"),
            LanguageModelGenerateResult::new(
                vec![
                    LanguageModelContent::Text(LanguageModelText::new("visible")),
                    LanguageModelContent::Text(LanguageModelText::new(" text")),
                ],
                LanguageModelFinishReason {
                    unified: FinishReason::Stop,
                    raw: None,
                },
                LanguageModelUsage::default(),
            ),
        );

        assert_eq!(step.text, "visible text");
    }

    #[test]
    fn generate_text_options_can_wrap_prepared_call_options() {
        let model = FakeLanguageModel::new();
        let call_options = LanguageModelCallOptions::new(vec![user_message("Hello")])
            .with_seed(7)
            .with_response_format(crate::language_model::LanguageModelResponseFormat::text());

        let result = poll_ready(generate_text(GenerateTextOptions::from_call_options(
            &model,
            call_options,
        )));

        assert_eq!(result.text, "Hello world");
        assert_eq!(model.calls.borrow()[0].seed, Some(7));
        assert_eq!(
            model.calls.borrow()[0].response_format,
            Some(crate::language_model::LanguageModelResponseFormat::text())
        );
    }

    #[test]
    fn generate_text_passes_high_level_rust_tools_to_language_model() {
        let model = FakeLanguageModel::new();
        let input_schema = json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            },
            "required": ["city"]
        })
        .as_object()
        .expect("schema is an object")
        .clone();

        let _ = poll_ready(generate_text(
            GenerateTextOptions::new(&model, vec![user_message("Weather?")]).with_tool(
                Tool::new("weather", input_schema.clone())
                    .with_description("Look up weather.")
                    .with_strict(true),
            ),
        ));

        assert_eq!(
            model.calls.borrow()[0].tools,
            Some(vec![LanguageModelTool::Function(
                LanguageModelFunctionTool::new("weather", input_schema)
                    .with_description("Look up weather.")
                    .with_strict(true)
            )])
        );
    }

    #[test]
    fn generate_text_includes_non_text_content_in_content_but_not_text() {
        let content = vec![LanguageModelContent::ToolCall(
            crate::language_model::LanguageModelToolCall::new("call-1", "lookup", "{}"),
        )];
        let step = GenerateTextStep::from_language_model_result(
            0,
            GenerateTextModelInfo::new("test-provider", "test-model"),
            LanguageModelGenerateResult::new(
                content,
                LanguageModelFinishReason {
                    unified: FinishReason::ToolCalls,
                    raw: Some("tool_calls".to_string()),
                },
                LanguageModelUsage::default(),
            ),
        );

        assert_eq!(step.text, "");
        assert_eq!(step.content.len(), 1);
        assert_eq!(step.finish_reason, FinishReason::ToolCalls);
    }

    #[test]
    fn generate_text_allows_assistant_prompt_messages_for_continuations() {
        let model = FakeLanguageModel::new();
        let prompt = vec![LanguageModelMessage::Assistant(
            crate::language_model::LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new("previous")),
            ]),
        )];

        let _ = poll_ready(generate_text(GenerateTextOptions::new(
            &model,
            prompt.clone(),
        )));

        assert_eq!(model.calls.borrow()[0].prompt, prompt);
    }
}
