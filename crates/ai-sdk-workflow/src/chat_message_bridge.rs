//! Runtime bridge for Open Agents chat messages.
//!
//! Open Agents persists AI SDK `UIMessage`-style parts, while the runtime model
//! call consumes provider-v4 model messages. This module keeps the Open Agents
//! data-part rule close to the runtime and delegates the rest to the canonical
//! AI SDK UI-message converter.

use ai_sdk_provider::LanguageModelPrompt;
use ai_sdk_rust::chat_transport::{
    ChatTransportError, ConvertUiMessagesToModelMessagesOptions, ConvertedUiMessageDataPart,
    convert_ui_messages_to_model_messages_with_data_part_converter,
};
use ai_sdk_rust::{
    StreamingUiMessageState, UiMessage, UiMessageChunk, UiMessageRole, process_ui_message_stream,
};
use chat_sdk_chat::message::Message as ChatSdkMessage;
use chat_sdk_chat::open_agent_message::{
    OpenAgentMessagePart, OpenAgentMessageRole, OpenAgentUiMessage,
};
use serde_json::{Value, json};

/// Convert a chat-sdk message into a persisted Open Agents user message.
pub fn chat_message_to_open_agent_ui_message(message: &ChatSdkMessage) -> OpenAgentUiMessage {
    OpenAgentUiMessage::from_chat_message(message)
}

/// Convert a persisted Open Agents UI message into the AI SDK UI-message shape.
pub fn open_agent_ui_message_to_ai_ui_message(message: &OpenAgentUiMessage) -> UiMessage {
    UiMessage {
        id: message.id.clone(),
        role: match message.role {
            OpenAgentMessageRole::System => UiMessageRole::System,
            OpenAgentMessageRole::User => UiMessageRole::User,
            OpenAgentMessageRole::Assistant => UiMessageRole::Assistant,
        },
        metadata: message.metadata.clone(),
        parts: message
            .parts
            .iter()
            .map(|part| part.raw().clone())
            .collect(),
    }
}

/// Convert an AI SDK UI message into the persisted Open Agents shape.
pub fn ai_ui_message_to_open_agent_ui_message(message: &UiMessage) -> OpenAgentUiMessage {
    let role = match message.role {
        UiMessageRole::System => OpenAgentMessageRole::System,
        UiMessageRole::User => OpenAgentMessageRole::User,
        UiMessageRole::Assistant => OpenAgentMessageRole::Assistant,
    };

    let mut open_agent_message = OpenAgentUiMessage::new(message.id.clone(), role);
    open_agent_message.metadata = message.metadata.clone();
    open_agent_message.parts = message
        .parts
        .iter()
        .cloned()
        .map(OpenAgentMessagePart::from)
        .collect();
    open_agent_message
}

/// Convert persisted Open Agents UI messages into model messages.
///
/// Matches Open Agents `convertToModelMessages` usage:
///
/// - incomplete tool calls are ignored before replaying history;
/// - `data-snippet` parts become text containing a JSON string with
///   `{ type: "snippet", filename, content }`;
/// - other data parts stay UI-only.
pub fn open_agent_ui_messages_to_model_messages(
    messages: &[OpenAgentUiMessage],
) -> Result<LanguageModelPrompt, ChatTransportError> {
    open_agent_ui_messages_to_model_messages_with_options(
        messages,
        ConvertUiMessagesToModelMessagesOptions::new().with_ignore_incomplete_tool_calls(true),
    )
}

/// Convert persisted Open Agents UI messages into model messages with explicit
/// conversion options.
pub fn open_agent_ui_messages_to_model_messages_with_options(
    messages: &[OpenAgentUiMessage],
    options: ConvertUiMessagesToModelMessagesOptions,
) -> Result<LanguageModelPrompt, ChatTransportError> {
    let ui_messages = messages
        .iter()
        .map(open_agent_ui_message_to_ai_ui_message)
        .collect::<Vec<_>>();

    convert_ui_messages_to_model_messages_with_data_part_converter(
        &ui_messages,
        options,
        open_agent_data_part_to_model_part,
    )
}

/// Convert runtime UI-message stream chunks into one persisted assistant
/// message.
pub fn open_agent_message_from_stream_chunks(
    message_id: impl Into<String>,
    chunks: impl IntoIterator<Item = UiMessageChunk>,
) -> Result<OpenAgentUiMessage, ChatTransportError> {
    let mut state = StreamingUiMessageState::new(message_id, None);
    process_ui_message_stream(&mut state, chunks, false).map_err(|error| {
        ChatTransportError::InvalidMessage(format!("UI message stream processing failed: {error}"))
    })?;
    Ok(ai_ui_message_to_open_agent_ui_message(&state.message))
}

/// Open Agents data-part converter used when reconstructing model context.
pub fn open_agent_data_part_to_model_part(
    part: &Value,
) -> Result<Option<ConvertedUiMessageDataPart>, ChatTransportError> {
    let Some("data-snippet") = part
        .as_object()
        .and_then(|object| object.get("type"))
        .and_then(Value::as_str)
    else {
        return Ok(None);
    };

    let data = part.get("data").and_then(Value::as_object).ok_or_else(|| {
        ChatTransportError::InvalidMessage(
            "Open Agents data-snippet part must include a data object.".to_string(),
        )
    })?;
    let filename = data
        .get("filename")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ChatTransportError::InvalidMessage(
                "Open Agents data-snippet data.filename must be a string.".to_string(),
            )
        })?;
    let content = data.get("content").and_then(Value::as_str).ok_or_else(|| {
        ChatTransportError::InvalidMessage(
            "Open Agents data-snippet data.content must be a string.".to_string(),
        )
    })?;

    Ok(Some(ConvertedUiMessageDataPart::text(
        json!({
            "type": "snippet",
            "filename": filename,
            "content": content,
        })
        .to_string(),
    )))
}

#[cfg(test)]
mod tests {
    use ai_sdk_provider::{
        LanguageModelAssistantContentPart, LanguageModelMessage, LanguageModelToolContentPart,
        LanguageModelUserContentPart,
    };
    use ai_sdk_rust::{FinishReason, UiMessageChunk};
    use chat_sdk_chat::open_agent_message::{OpenAgentMessageRole, OpenAgentUiMessage};

    use crate::chat_message_bridge::{
        ai_ui_message_to_open_agent_ui_message, open_agent_message_from_stream_chunks,
        open_agent_ui_message_to_ai_ui_message, open_agent_ui_messages_to_model_messages,
    };

    #[test]
    fn open_agent_persisted_fixture_converts_to_model_messages_with_snippets_and_tools() {
        let messages: Vec<OpenAgentUiMessage> = serde_json::from_str(include_str!(
            "../../chat-sdk-chat/src/fixtures/open-agent-persisted-messages.json"
        ))
        .unwrap();

        let model_messages = open_agent_ui_messages_to_model_messages(&messages).unwrap();
        let model_json = serde_json::to_string(&model_messages).unwrap();

        assert!(model_json.contains("fn main() {}"));
        assert!(model_json.contains("call-bash-1"));
        assert!(model_json.contains("approval-1"));
        assert!(!model_json.contains("call-question-1"));

        let LanguageModelMessage::User(user_message) = &model_messages[0] else {
            panic!("expected user model message");
        };
        assert!(matches!(
            user_message.content.as_slice(),
            [
                LanguageModelUserContentPart::Text(_),
                LanguageModelUserContentPart::Text(_)
            ]
        ));

        assert!(model_messages.iter().any(|message| matches!(
            message,
            LanguageModelMessage::Assistant(assistant)
                if assistant.content.iter().any(|part| matches!(
                    part,
                    LanguageModelAssistantContentPart::ToolCall(call)
                        if call.tool_call_id == "call-bash-1"
                ))
        )));
        assert!(model_messages.iter().any(|message| matches!(
            message,
            LanguageModelMessage::Tool(tool)
                if tool.content.iter().any(|part| matches!(
                    part,
                    LanguageModelToolContentPart::ToolResult(result)
                        if result.tool_call_id == "call-bash-1"
                ))
        )));
    }

    #[test]
    fn open_agent_ui_message_round_trips_through_ai_ui_message_shape() {
        let messages: Vec<OpenAgentUiMessage> = serde_json::from_str(include_str!(
            "../../chat-sdk-chat/src/fixtures/open-agent-persisted-messages.json"
        ))
        .unwrap();

        let ai_message = open_agent_ui_message_to_ai_ui_message(&messages[3]);
        let roundtrip = ai_ui_message_to_open_agent_ui_message(&ai_message);

        assert_eq!(roundtrip, messages[3]);
    }

    #[test]
    fn stream_chunks_become_persisted_final_open_agent_message() {
        let message = open_agent_message_from_stream_chunks(
            "assistant-final",
            [
                UiMessageChunk::start_with_message_id("assistant-final"),
                UiMessageChunk::start_step(),
                UiMessageChunk::text_start("text-1"),
                UiMessageChunk::text_delta("text-1", "Done."),
                UiMessageChunk::text_end("text-1"),
                UiMessageChunk::finish_step(),
                UiMessageChunk::finish_with_reason(FinishReason::Stop),
            ],
        )
        .unwrap();

        assert_eq!(message.id, "assistant-final");
        assert_eq!(message.role, OpenAgentMessageRole::Assistant);
        assert_eq!(message.parts.len(), 2);
        assert_eq!(message.parts[1].raw()["text"], "Done.");
        assert_eq!(message.parts[1].raw()["state"], "done");
    }
}
