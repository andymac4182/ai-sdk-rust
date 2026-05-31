use std::collections::VecDeque;

use ai_sdk_provider::json::JsonValue;

use crate::WorkflowModelInfo;

/// Factory boundary for provider models that must cross workflow steps.
pub trait WorkflowModelFactory {
    /// Returns the model identity that would be initialized inside the step.
    fn model(&mut self) -> WorkflowModelInfo;
}

/// Serializable provider factory equivalent to upstream provider wrappers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticWorkflowModelFactory {
    provider: String,
    model_id: String,
}

impl StaticWorkflowModelFactory {
    pub fn new(provider: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model_id: model_id.into(),
        }
    }
}

impl WorkflowModelFactory for StaticWorkflowModelFactory {
    fn model(&mut self) -> WorkflowModelInfo {
        WorkflowModelInfo::new(self.provider.clone(), self.model_id.clone())
    }
}

/// Deterministic mock model response.
#[derive(Clone, Debug, PartialEq)]
pub enum MockWorkflowModelResponse {
    /// Text-only mock response.
    Text(String),

    /// Local tool-call response.
    ToolCall { tool_name: String, input: String },

    /// Provider-executed tool-call response with a provider result.
    ProviderToolCall {
        tool_name: String,
        input: String,
        result: JsonValue,
    },
}

/// Mock model factory for deterministic workflow tests.
#[derive(Clone, Debug, PartialEq)]
pub struct MockWorkflowModelFactory {
    responses: VecDeque<MockWorkflowModelResponse>,
}

impl MockWorkflowModelFactory {
    pub fn new(responses: impl IntoIterator<Item = MockWorkflowModelResponse>) -> Self {
        Self {
            responses: responses.into_iter().collect(),
        }
    }

    pub fn next_response(&mut self) -> Option<MockWorkflowModelResponse> {
        self.responses.pop_front()
    }
}

impl WorkflowModelFactory for MockWorkflowModelFactory {
    fn model(&mut self) -> WorkflowModelInfo {
        WorkflowModelInfo::new("mock", "mock")
    }
}

pub fn openai(model_id: impl Into<String>) -> StaticWorkflowModelFactory {
    StaticWorkflowModelFactory::new("openai", model_id)
}

pub fn anthropic(model_id: impl Into<String>) -> StaticWorkflowModelFactory {
    StaticWorkflowModelFactory::new("anthropic", model_id)
}

pub fn gateway(model_id: impl Into<String>) -> StaticWorkflowModelFactory {
    StaticWorkflowModelFactory::new("gateway", model_id)
}

pub fn google(model_id: impl Into<String>) -> StaticWorkflowModelFactory {
    StaticWorkflowModelFactory::new("google", model_id)
}

pub fn xai(model_id: impl Into<String>) -> StaticWorkflowModelFactory {
    StaticWorkflowModelFactory::new("xai", model_id)
}

pub fn mock_text_model(text: impl Into<String>) -> MockWorkflowModelFactory {
    MockWorkflowModelFactory::new([MockWorkflowModelResponse::Text(text.into())])
}

pub fn mock_sequence_model(
    responses: impl IntoIterator<Item = MockWorkflowModelResponse>,
) -> MockWorkflowModelFactory {
    MockWorkflowModelFactory::new(responses)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn provider_bridge_upstream_should_wrap_named_providers_as_step_safe_factories() {
        assert_eq!(
            openai("gpt-4.1").model(),
            WorkflowModelInfo::new("openai", "gpt-4.1")
        );
        assert_eq!(
            anthropic("claude-sonnet-4.5").model(),
            WorkflowModelInfo::new("anthropic", "claude-sonnet-4.5")
        );
        assert_eq!(
            gateway("openai/gpt-4.1").model(),
            WorkflowModelInfo::new("gateway", "openai/gpt-4.1")
        );
        assert_eq!(
            google("gemini-2.5-pro").model(),
            WorkflowModelInfo::new("google", "gemini-2.5-pro")
        );
        assert_eq!(
            xai("grok-4").model(),
            WorkflowModelInfo::new("xai", "grok-4")
        );
    }

    #[test]
    fn provider_bridge_upstream_mock_text_model_returns_serializable_text_response() {
        let mut factory = mock_text_model("hello");

        assert_eq!(factory.model(), WorkflowModelInfo::new("mock", "mock"));
        assert_eq!(
            factory.next_response(),
            Some(MockWorkflowModelResponse::Text("hello".to_string()))
        );
    }

    #[test]
    fn provider_bridge_upstream_mock_sequence_model_replays_tool_and_provider_tool_responses() {
        let mut factory = mock_sequence_model([
            MockWorkflowModelResponse::ToolCall {
                tool_name: "localTool".to_string(),
                input: "{}".to_string(),
            },
            MockWorkflowModelResponse::ProviderToolCall {
                tool_name: "webSearch".to_string(),
                input: r#"{"query":"rust"}"#.to_string(),
                result: json!({ "answer": "Rust" }),
            },
        ]);

        assert!(matches!(
            factory.next_response(),
            Some(MockWorkflowModelResponse::ToolCall { .. })
        ));
        assert!(matches!(
            factory.next_response(),
            Some(MockWorkflowModelResponse::ProviderToolCall { .. })
        ));
        assert_eq!(factory.next_response(), None);
    }
}
