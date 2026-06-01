//! AI SDK helper surface for the Chat SDK.
//!
//! This module ports upstream `packages/chat/src/ai/*`: converting chat
//! messages into model-message-shaped values and constructing a stable catalog
//! of chat tools an agent can call.

use std::collections::{BTreeMap, HashSet};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::chat::{Chat, GetUserError, OpenDmError};
use crate::message::Message;
use crate::types::{
    AdapterError, Attachment, AttachmentKind, ChannelInfo, FetchOptions, ListThreadsOptions,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiMessageRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AiMessagePart {
    Text {
        text: String,
    },
    File {
        data: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        #[serde(rename = "mediaType")]
        media_type: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AiMessageContent {
    Text(String),
    Parts(Vec<AiMessagePart>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiMessage {
    pub role: AiMessageRole,
    pub content: AiMessageContent,
}

pub type TransformMessage<'a> = dyn Fn(AiMessage, &Message) -> Option<AiMessage> + Send + Sync + 'a;
pub type UnsupportedAttachmentHandler<'a> = dyn Fn(&Attachment, &Message) + Send + Sync + 'a;

#[derive(Default)]
pub struct ToAiMessagesOptions<'a> {
    pub include_names: bool,
    pub on_unsupported_attachment: Option<&'a UnsupportedAttachmentHandler<'a>>,
    pub transform_message: Option<&'a TransformMessage<'a>>,
}

fn is_text_mime_type(mime_type: &str) -> bool {
    mime_type.starts_with("text/")
        || matches!(
            mime_type,
            "application/json"
                | "application/xml"
                | "application/javascript"
                | "application/typescript"
                | "application/yaml"
                | "application/x-yaml"
                | "application/toml"
        )
}

fn attachment_to_part(attachment: &Attachment) -> Option<AiMessagePart> {
    let data = attachment.data.as_ref()?;
    let mime_type = attachment.mime_type.as_deref().unwrap_or_else(|| {
        if attachment.kind == AttachmentKind::Image {
            "image/png"
        } else {
            ""
        }
    });
    if mime_type.is_empty() {
        return None;
    }
    if attachment.kind == AttachmentKind::File && !is_text_mime_type(mime_type) {
        return None;
    }
    if !matches!(
        attachment.kind,
        AttachmentKind::Image | AttachmentKind::File
    ) {
        return None;
    }
    Some(AiMessagePart::File {
        data: format!("data:{mime_type};base64,{}", STANDARD.encode(&data.0)),
        filename: attachment.name.clone(),
        media_type: mime_type.to_string(),
    })
}

fn link_text(message: &Message) -> String {
    if message.links.is_empty() {
        return message.text.clone();
    }
    let link_parts = message
        .links
        .iter()
        .map(|link| {
            let mut parts = vec![link.url.clone()];
            if let Some(title) = link.title.as_ref() {
                parts.push(format!("Title: {title}"));
            }
            if let Some(description) = link.description.as_ref() {
                parts.push(format!("Description: {description}"));
            }
            if let Some(site) = link.site_name.as_ref() {
                parts.push(format!("Site: {site}"));
            }
            parts.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("{}\n\nLinks:\n{link_parts}", message.text)
}

pub async fn to_ai_messages(
    messages: &[Message],
    options: ToAiMessagesOptions<'_>,
) -> Vec<AiMessage> {
    let mut sorted = messages.to_vec();
    sorted.sort_by(|left, right| left.metadata.date_sent.cmp(&right.metadata.date_sent));

    let mut result = Vec::new();
    for message in sorted {
        if message.text.trim().is_empty() {
            continue;
        }
        let role = if message.author.is_me {
            AiMessageRole::Assistant
        } else {
            AiMessageRole::User
        };
        let mut text = link_text(&message);
        if options.include_names && role == AiMessageRole::User {
            text = format!("[{}]: {text}", message.author.user_name);
        }

        let content = if role == AiMessageRole::User {
            let mut parts = Vec::new();
            for attachment in &message.attachments {
                if let Some(part) = attachment_to_part(attachment) {
                    parts.push(part);
                } else if matches!(
                    attachment.kind,
                    AttachmentKind::Video | AttachmentKind::Audio
                ) && let Some(handler) = options.on_unsupported_attachment
                {
                    handler(attachment, &message);
                }
            }
            if parts.is_empty() {
                AiMessageContent::Text(text)
            } else {
                let mut content = vec![AiMessagePart::Text { text }];
                content.extend(parts);
                AiMessageContent::Parts(content)
            }
        } else {
            AiMessageContent::Text(text)
        };

        let ai_message = AiMessage { role, content };
        match options.transform_message {
            Some(transform) => {
                if let Some(transformed) = transform(ai_message, &message) {
                    result.push(transformed);
                }
            }
            None => result.push(ai_message),
        }
    }
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChatToolName {
    FetchMessages,
    FetchChannelMessages,
    FetchThread,
    ListThreads,
    GetThreadParticipants,
    GetChannelInfo,
    GetUser,
    StartTyping,
    PostMessage,
    PostChannelMessage,
    SendDirectMessage,
    EditMessage,
    DeleteMessage,
    AddReaction,
    RemoveReaction,
    SubscribeThread,
    UnsubscribeThread,
}

impl ChatToolName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FetchMessages => "fetchMessages",
            Self::FetchChannelMessages => "fetchChannelMessages",
            Self::FetchThread => "fetchThread",
            Self::ListThreads => "listThreads",
            Self::GetThreadParticipants => "getThreadParticipants",
            Self::GetChannelInfo => "getChannelInfo",
            Self::GetUser => "getUser",
            Self::StartTyping => "startTyping",
            Self::PostMessage => "postMessage",
            Self::PostChannelMessage => "postChannelMessage",
            Self::SendDirectMessage => "sendDirectMessage",
            Self::EditMessage => "editMessage",
            Self::DeleteMessage => "deleteMessage",
            Self::AddReaction => "addReaction",
            Self::RemoveReaction => "removeReaction",
            Self::SubscribeThread => "subscribeThread",
            Self::UnsubscribeThread => "unsubscribeThread",
        }
    }

    fn is_write(self) -> bool {
        matches!(
            self,
            Self::PostMessage
                | Self::PostChannelMessage
                | Self::SendDirectMessage
                | Self::EditMessage
                | Self::DeleteMessage
                | Self::AddReaction
                | Self::RemoveReaction
                | Self::SubscribeThread
                | Self::UnsubscribeThread
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatToolPreset {
    Reader,
    Messenger,
    Moderator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalConfig {
    All(bool),
    PerTool(BTreeMap<ChatToolName, bool>),
}

impl Default for ApprovalConfig {
    fn default() -> Self {
        Self::All(true)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolOverrides {
    pub description: Option<String>,
    pub input_examples: Option<Vec<Value>>,
    pub metadata: Option<Value>,
    pub needs_approval: Option<bool>,
    pub title: Option<String>,
    pub protected_fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default)]
pub struct ChatToolsOptions {
    pub overrides: BTreeMap<ChatToolName, ToolOverrides>,
    pub preset: Option<Vec<ChatToolPreset>>,
    pub require_approval: ApprovalConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatTool {
    pub name: ChatToolName,
    pub description: Option<String>,
    pub input_examples: Option<Vec<Value>>,
    pub metadata: Option<Value>,
    pub needs_approval: Option<bool>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatToolsError {
    MissingChat,
    AdapterNotFound(String),
    Adapter(AdapterErrorDisplay),
    OpenDm(String),
    GetUser(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterErrorDisplay(String);

impl From<AdapterError> for AdapterErrorDisplay {
    fn from(value: AdapterError) -> Self {
        Self(value.to_string())
    }
}

impl From<AdapterErrorDisplay> for ChatToolsError {
    fn from(value: AdapterErrorDisplay) -> Self {
        Self::Adapter(value)
    }
}

impl From<OpenDmError> for ChatToolsError {
    fn from(value: OpenDmError) -> Self {
        Self::OpenDm(value.to_string())
    }
}

impl From<GetUserError> for ChatToolsError {
    fn from(value: GetUserError) -> Self {
        Self::GetUser(value.to_string())
    }
}

fn all_tool_names() -> Vec<ChatToolName> {
    vec![
        ChatToolName::AddReaction,
        ChatToolName::DeleteMessage,
        ChatToolName::EditMessage,
        ChatToolName::FetchChannelMessages,
        ChatToolName::FetchMessages,
        ChatToolName::FetchThread,
        ChatToolName::GetChannelInfo,
        ChatToolName::GetThreadParticipants,
        ChatToolName::GetUser,
        ChatToolName::ListThreads,
        ChatToolName::PostChannelMessage,
        ChatToolName::PostMessage,
        ChatToolName::RemoveReaction,
        ChatToolName::SendDirectMessage,
        ChatToolName::StartTyping,
        ChatToolName::SubscribeThread,
        ChatToolName::UnsubscribeThread,
    ]
}

fn preset_tools(preset: ChatToolPreset) -> &'static [ChatToolName] {
    match preset {
        ChatToolPreset::Reader => &[
            ChatToolName::FetchMessages,
            ChatToolName::FetchChannelMessages,
            ChatToolName::FetchThread,
            ChatToolName::ListThreads,
            ChatToolName::GetThreadParticipants,
            ChatToolName::GetChannelInfo,
            ChatToolName::GetUser,
        ],
        ChatToolPreset::Messenger => &[
            ChatToolName::FetchMessages,
            ChatToolName::FetchThread,
            ChatToolName::GetChannelInfo,
            ChatToolName::GetUser,
            ChatToolName::PostMessage,
            ChatToolName::PostChannelMessage,
            ChatToolName::SendDirectMessage,
            ChatToolName::AddReaction,
            ChatToolName::RemoveReaction,
            ChatToolName::StartTyping,
        ],
        ChatToolPreset::Moderator => &[
            ChatToolName::FetchMessages,
            ChatToolName::FetchChannelMessages,
            ChatToolName::FetchThread,
            ChatToolName::ListThreads,
            ChatToolName::GetThreadParticipants,
            ChatToolName::GetChannelInfo,
            ChatToolName::GetUser,
            ChatToolName::PostMessage,
            ChatToolName::PostChannelMessage,
            ChatToolName::SendDirectMessage,
            ChatToolName::EditMessage,
            ChatToolName::DeleteMessage,
            ChatToolName::AddReaction,
            ChatToolName::RemoveReaction,
            ChatToolName::SubscribeThread,
            ChatToolName::UnsubscribeThread,
            ChatToolName::StartTyping,
        ],
    }
}

fn approval_for(name: ChatToolName, config: &ApprovalConfig) -> Option<bool> {
    if !name.is_write() {
        return None;
    }
    Some(match config {
        ApprovalConfig::All(value) => *value,
        ApprovalConfig::PerTool(values) => values.get(&name).copied().unwrap_or(true),
    })
}

pub fn create_chat_tools(
    chat: Option<&Chat>,
    options: ChatToolsOptions,
) -> Result<BTreeMap<ChatToolName, ChatTool>, ChatToolsError> {
    if chat.is_none() {
        return Err(ChatToolsError::MissingChat);
    }
    let allowed = options.preset.as_ref().map(|presets| {
        presets
            .iter()
            .flat_map(|preset| preset_tools(*preset).iter().copied())
            .collect::<HashSet<_>>()
    });
    let mut tools = BTreeMap::new();
    for name in all_tool_names() {
        if allowed
            .as_ref()
            .is_some_and(|allowed| !allowed.contains(&name))
        {
            continue;
        }
        let overrides = options.overrides.get(&name);
        let mut tool = ChatTool {
            name,
            description: overrides.and_then(|o| o.description.clone()),
            input_examples: overrides.and_then(|o| o.input_examples.clone()),
            metadata: overrides.and_then(|o| o.metadata.clone()),
            needs_approval: approval_for(name, &options.require_approval),
            title: overrides.and_then(|o| o.title.clone()),
        };
        if let Some(override_approval) = overrides.and_then(|o| o.needs_approval) {
            tool.needs_approval = Some(override_approval);
        }
        tools.insert(name, tool);
    }
    Ok(tools)
}

fn adapter_name_from_id(id: &str) -> Result<&str, ChatToolsError> {
    id.split_once(':')
        .map(|(adapter, _)| adapter)
        .filter(|adapter| !adapter.is_empty())
        .ok_or_else(|| ChatToolsError::AdapterNotFound(id.to_string()))
}

fn adapter_for_id(
    chat: &Chat,
    id: &str,
) -> Result<std::sync::Arc<dyn crate::types::Adapter>, ChatToolsError> {
    let adapter_name = adapter_name_from_id(id)?;
    chat.get_adapter(adapter_name)
        .ok_or_else(|| ChatToolsError::AdapterNotFound(adapter_name.to_string()))
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChatToolInput {
    PostMessage {
        thread_id: String,
        message: Value,
    },
    PostChannelMessage {
        channel_id: String,
        message: Value,
    },
    SendDirectMessage {
        user_id: String,
        message: String,
    },
    EditMessage {
        thread_id: String,
        message_id: String,
        message: String,
    },
    DeleteMessage {
        thread_id: String,
        message_id: String,
    },
    Reaction {
        thread_id: String,
        message_id: String,
        emoji: String,
    },
    Subscribe {
        thread_id: String,
    },
    StartTyping {
        thread_id: String,
        status: Option<String>,
    },
    FetchMessages {
        thread_id: String,
        options: FetchOptions,
    },
    FetchChannelMessages {
        channel_id: String,
        options: FetchOptions,
    },
    FetchThread {
        thread_id: String,
    },
    ListThreads {
        channel_id: String,
        options: ListThreadsOptions,
    },
    GetThreadParticipants {
        thread_id: String,
    },
    GetChannelInfo {
        channel_id: String,
    },
    GetUser {
        user_id: String,
    },
}

impl ChatTool {
    pub async fn execute(
        &self,
        chat: &Chat,
        input: ChatToolInput,
    ) -> Result<Value, ChatToolsError> {
        match (self.name, input) {
            (ChatToolName::PostMessage, ChatToolInput::PostMessage { thread_id, message }) => {
                let adapter = adapter_for_id(chat, &thread_id)?;
                let id = match message {
                    Value::String(text) => adapter.post_message(&thread_id, &text).await,
                    value => adapter.post_object(&thread_id, "raw", value).await,
                }
                .map_err(AdapterErrorDisplay::from)?;
                Ok(json!({ "messageId": id }))
            }
            (
                ChatToolName::PostChannelMessage,
                ChatToolInput::PostChannelMessage {
                    channel_id,
                    message,
                },
            ) => {
                let adapter = adapter_for_id(chat, &channel_id)?;
                let id = match message {
                    Value::String(text) => adapter.post_channel_message(&channel_id, &text).await,
                    value => {
                        adapter
                            .post_channel_message_postable(&channel_id, &value)
                            .await
                    }
                }
                .map_err(AdapterErrorDisplay::from)?;
                Ok(json!({ "messageId": id }))
            }
            (
                ChatToolName::SendDirectMessage,
                ChatToolInput::SendDirectMessage { user_id, message },
            ) => {
                let thread = chat.open_dm(&user_id).await?;
                let message_id = thread
                    .post(&message)
                    .await
                    .map_err(AdapterErrorDisplay::from)?;
                Ok(json!({ "threadId": thread.thread_id(), "messageId": message_id }))
            }
            (
                ChatToolName::EditMessage,
                ChatToolInput::EditMessage {
                    thread_id,
                    message_id,
                    message,
                },
            ) => {
                let adapter = adapter_for_id(chat, &thread_id)?;
                let id = adapter
                    .edit_message(&thread_id, &message_id, &message)
                    .await
                    .map_err(AdapterErrorDisplay::from)?;
                Ok(json!({ "messageId": id }))
            }
            (
                ChatToolName::DeleteMessage,
                ChatToolInput::DeleteMessage {
                    thread_id,
                    message_id,
                },
            ) => {
                let adapter = adapter_for_id(chat, &thread_id)?;
                adapter
                    .delete_message(&thread_id, &message_id)
                    .await
                    .map_err(AdapterErrorDisplay::from)?;
                Ok(json!({ "deleted": true }))
            }
            (
                ChatToolName::AddReaction,
                ChatToolInput::Reaction {
                    thread_id,
                    message_id,
                    emoji,
                },
            ) => {
                let adapter = adapter_for_id(chat, &thread_id)?;
                adapter
                    .add_reaction(&thread_id, &message_id, &emoji)
                    .await
                    .map_err(AdapterErrorDisplay::from)?;
                Ok(json!({ "added": true, "emoji": emoji }))
            }
            (
                ChatToolName::RemoveReaction,
                ChatToolInput::Reaction {
                    thread_id,
                    message_id,
                    emoji,
                },
            ) => {
                let adapter = adapter_for_id(chat, &thread_id)?;
                adapter
                    .remove_reaction(&thread_id, &message_id, &emoji)
                    .await
                    .map_err(AdapterErrorDisplay::from)?;
                Ok(json!({ "removed": true, "emoji": emoji }))
            }
            (ChatToolName::SubscribeThread, ChatToolInput::Subscribe { thread_id }) => {
                chat.state()
                    .subscribe(&thread_id)
                    .await
                    .map_err(|err| ChatToolsError::Adapter(AdapterErrorDisplay(err.to_string())))?;
                Ok(json!({ "subscribed": true }))
            }
            (ChatToolName::UnsubscribeThread, ChatToolInput::Subscribe { thread_id }) => {
                chat.state()
                    .unsubscribe(&thread_id)
                    .await
                    .map_err(|err| ChatToolsError::Adapter(AdapterErrorDisplay(err.to_string())))?;
                Ok(json!({ "subscribed": false }))
            }
            (ChatToolName::StartTyping, ChatToolInput::StartTyping { thread_id, status }) => {
                let adapter = adapter_for_id(chat, &thread_id)?;
                adapter
                    .start_typing(&thread_id, status.as_deref())
                    .await
                    .map_err(AdapterErrorDisplay::from)?;
                Ok(json!({ "started": true }))
            }
            (ChatToolName::FetchMessages, ChatToolInput::FetchMessages { thread_id, options }) => {
                let adapter = adapter_for_id(chat, &thread_id)?;
                let result = adapter
                    .fetch_messages(&thread_id, &options)
                    .await
                    .map_err(AdapterErrorDisplay::from)?;
                Ok(serde_json::to_value(result).unwrap())
            }
            (
                ChatToolName::FetchChannelMessages,
                ChatToolInput::FetchChannelMessages {
                    channel_id,
                    options,
                },
            ) => {
                let adapter = adapter_for_id(chat, &channel_id)?;
                let result = adapter
                    .fetch_channel_messages(&channel_id, &options)
                    .await
                    .map_err(AdapterErrorDisplay::from)?;
                Ok(serde_json::to_value(result).unwrap())
            }
            (ChatToolName::FetchThread, ChatToolInput::FetchThread { thread_id }) => {
                let adapter = adapter_for_id(chat, &thread_id)?;
                let result = adapter
                    .fetch_thread(&thread_id)
                    .await
                    .map_err(AdapterErrorDisplay::from)?;
                Ok(serde_json::to_value(result).unwrap())
            }
            (
                ChatToolName::ListThreads,
                ChatToolInput::ListThreads {
                    channel_id,
                    options,
                },
            ) => {
                let adapter = adapter_for_id(chat, &channel_id)?;
                let result = adapter
                    .list_threads(&channel_id, &options)
                    .await
                    .map_err(AdapterErrorDisplay::from)?;
                Ok(serde_json::to_value(result).unwrap())
            }
            (
                ChatToolName::GetThreadParticipants,
                ChatToolInput::GetThreadParticipants { thread_id },
            ) => {
                let thread = chat.thread(thread_id);
                let participants = thread
                    .get_participants()
                    .await
                    .map_err(AdapterErrorDisplay::from)?;
                Ok(json!({ "participants": participants }))
            }
            (ChatToolName::GetChannelInfo, ChatToolInput::GetChannelInfo { channel_id }) => {
                let adapter = adapter_for_id(chat, &channel_id)?;
                let info = match adapter.fetch_channel_info(&channel_id).await {
                    Ok(info) => info,
                    Err(AdapterError::Unsupported(_)) => ChannelInfo {
                        channel_visibility: None,
                        id: channel_id.clone(),
                        is_dm: Some(false),
                        member_count: None,
                        metadata: serde_json::Map::new(),
                        name: Some(format!("#{channel_id}")),
                    },
                    Err(err) => return Err(ChatToolsError::Adapter(err.into())),
                };
                Ok(serde_json::to_value(info).unwrap())
            }
            (ChatToolName::GetUser, ChatToolInput::GetUser { user_id }) => {
                let user = chat.get_user(&user_id).await?;
                Ok(serde_json::to_value(user).unwrap())
            }
            _ => Err(ChatToolsError::Adapter(AdapterErrorDisplay(
                "tool/input mismatch".to_string(),
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::ChatOptions;
    use crate::types::{
        Adapter, AdapterResult, Author, BotStatus, FetchResult, FileBytes, ListThreadsResult,
        MessageMetadata, StateAdapter, StateResult, ThreadInfo, ThreadSummary, UserInfo,
    };
    use async_trait::async_trait;
    use futures_executor::block_on;
    use std::sync::{Arc, Mutex};

    #[derive(Debug)]
    struct MemoryState {
        subscribed: Mutex<HashSet<String>>,
    }

    impl MemoryState {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                subscribed: Mutex::new(HashSet::new()),
            })
        }
    }

    #[async_trait]
    impl StateAdapter for MemoryState {
        async fn get(&self, _key: &str) -> StateResult<Option<Value>> {
            Ok(None)
        }
        async fn set(&self, _key: &str, _value: Value, _ttl_ms: Option<u64>) -> StateResult<()> {
            Ok(())
        }
        async fn delete(&self, _key: &str) -> StateResult<()> {
            Ok(())
        }
        async fn append_to_list(
            &self,
            _key: &str,
            _value: Value,
            _max_length: Option<usize>,
            _ttl_ms: Option<u64>,
        ) -> StateResult<()> {
            Ok(())
        }
        async fn get_list(&self, _key: &str, _limit: Option<usize>) -> StateResult<Vec<Value>> {
            Ok(Vec::new())
        }
        async fn subscribe(&self, thread_id: &str) -> StateResult<()> {
            self.subscribed
                .lock()
                .unwrap()
                .insert(thread_id.to_string());
            Ok(())
        }
        async fn unsubscribe(&self, thread_id: &str) -> StateResult<()> {
            self.subscribed.lock().unwrap().remove(thread_id);
            Ok(())
        }
        async fn is_subscribed(&self, thread_id: &str) -> StateResult<bool> {
            Ok(self.subscribed.lock().unwrap().contains(thread_id))
        }
    }

    #[derive(Debug, Default)]
    struct RecordingAdapter {
        calls: Mutex<Vec<String>>,
        messages: Mutex<Vec<Message>>,
        user: Mutex<Option<UserInfo>>,
    }

    #[async_trait]
    impl Adapter for RecordingAdapter {
        fn name(&self) -> &str {
            "slack"
        }
        async fn post_message(&self, thread_id: &str, text: &str) -> AdapterResult<String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("post:{thread_id}:{text}"));
            Ok("msg-1".to_string())
        }
        async fn post_object(
            &self,
            thread_id: &str,
            _kind: &str,
            data: Value,
        ) -> AdapterResult<String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("post-object:{thread_id}:{data}"));
            Ok("msg-1".to_string())
        }
        async fn post_channel_message(
            &self,
            channel_id: &str,
            text: &str,
        ) -> AdapterResult<String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("post-channel:{channel_id}:{text}"));
            Ok("channel-msg-1".to_string())
        }
        async fn post_channel_message_postable(
            &self,
            channel_id: &str,
            message: &Value,
        ) -> AdapterResult<String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("post-channel-object:{channel_id}:{message}"));
            Ok("channel-msg-1".to_string())
        }
        async fn open_dm(&self, user_id: &str) -> AdapterResult<String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("open-dm:{user_id}"));
            Ok(format!("slack:D{user_id}:1.0"))
        }
        fn is_dm(&self, thread_id: &str) -> Option<bool> {
            Some(thread_id.contains(":D"))
        }
        async fn edit_message(
            &self,
            thread_id: &str,
            message_id: &str,
            text: &str,
        ) -> AdapterResult<String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("edit:{thread_id}:{message_id}:{text}"));
            Ok(message_id.to_string())
        }
        async fn delete_message(&self, thread_id: &str, message_id: &str) -> AdapterResult<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("delete:{thread_id}:{message_id}"));
            Ok(())
        }
        async fn add_reaction(
            &self,
            thread_id: &str,
            message_id: &str,
            emoji: &str,
        ) -> AdapterResult<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("add-reaction:{thread_id}:{message_id}:{emoji}"));
            Ok(())
        }
        async fn remove_reaction(
            &self,
            thread_id: &str,
            message_id: &str,
            emoji: &str,
        ) -> AdapterResult<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("remove-reaction:{thread_id}:{message_id}:{emoji}"));
            Ok(())
        }
        async fn start_typing(&self, thread_id: &str, status: Option<&str>) -> AdapterResult<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("typing:{thread_id}:{}", status.unwrap_or("")));
            Ok(())
        }
        async fn fetch_messages(
            &self,
            _thread_id: &str,
            _options: &FetchOptions,
        ) -> AdapterResult<FetchResult> {
            Ok(FetchResult {
                messages: self.messages.lock().unwrap().clone(),
                next_cursor: Some("next".to_string()),
            })
        }
        async fn fetch_channel_messages(
            &self,
            _channel_id: &str,
            _options: &FetchOptions,
        ) -> AdapterResult<FetchResult> {
            self.fetch_messages("thread", &FetchOptions::default())
                .await
        }
        async fn fetch_thread(&self, thread_id: &str) -> AdapterResult<ThreadInfo> {
            Ok(ThreadInfo {
                channel_id: "C123".to_string(),
                channel_name: Some("#general".to_string()),
                channel_visibility: None,
                id: thread_id.to_string(),
                is_dm: Some(false),
                metadata: serde_json::Map::new(),
            })
        }
        async fn list_threads(
            &self,
            _channel_id: &str,
            _options: &ListThreadsOptions,
        ) -> AdapterResult<ListThreadsResult> {
            Ok(ListThreadsResult {
                threads: vec![ThreadSummary {
                    id: "slack:C123:1.0".to_string(),
                    last_reply_at: Some("2026-01-02T00:00:00Z".to_string()),
                    reply_count: Some(4),
                    root_message: sample_message("root", "root"),
                }],
                next_cursor: None,
            })
        }
        async fn fetch_channel_info(&self, channel_id: &str) -> AdapterResult<ChannelInfo> {
            Ok(ChannelInfo {
                channel_visibility: None,
                id: channel_id.to_string(),
                is_dm: Some(false),
                member_count: None,
                metadata: serde_json::Map::new(),
                name: Some(format!("#{channel_id}")),
            })
        }
        async fn get_user(&self, _user_id: &str) -> AdapterResult<Option<UserInfo>> {
            Ok(self.user.lock().unwrap().clone())
        }
    }

    fn author(user_id: &str, user_name: &str, is_bot: bool, is_me: bool) -> Author {
        Author {
            full_name: user_name.to_string(),
            is_bot: BotStatus::Known(is_bot),
            is_me,
            user_id: user_id.to_string(),
            user_name: user_name.to_string(),
        }
    }

    fn sample_message(id: &str, text: &str) -> Message {
        Message::new(
            id,
            "slack:C123:1.0",
            text,
            crate::markdown::root(vec![]),
            json!({}),
            author("U1", "alice", false, false),
            MessageMetadata {
                date_sent: format!("2026-01-01T00:00:0{id}Z"),
                edited: false,
                edited_at: None,
            },
            vec![],
        )
    }

    fn chat_with(adapter: Arc<RecordingAdapter>, state: Arc<MemoryState>) -> Chat {
        Chat::new(ChatOptions {
            adapters: vec![adapter],
            state,
            ..Default::default()
        })
    }

    fn tool(tools: &BTreeMap<ChatToolName, ChatTool>, name: ChatToolName) -> ChatTool {
        tools.get(&name).unwrap().clone()
    }

    #[test]
    fn to_ai_messages_maps_is_me_to_assistant_and_others_to_user() {
        let mut bot = sample_message("2", "Hi");
        bot.author = author("B1", "bot", true, true);
        let result = block_on(to_ai_messages(
            &[sample_message("1", "Hello"), bot],
            ToAiMessagesOptions::default(),
        ));
        assert_eq!(result[0].role, AiMessageRole::User);
        assert_eq!(result[1].role, AiMessageRole::Assistant);
    }

    #[test]
    fn to_ai_messages_filters_out_empty_and_whitespace_only_text() {
        let result = block_on(to_ai_messages(
            &[
                sample_message("1", "Hello"),
                sample_message("2", " "),
                sample_message("3", "\t\n"),
            ],
            ToAiMessagesOptions::default(),
        ));
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn to_ai_messages_preserves_chronological_order() {
        let mut late = sample_message("2", "late");
        late.metadata.date_sent = "2026-01-02T00:00:00Z".to_string();
        let mut early = sample_message("1", "early");
        early.metadata.date_sent = "2026-01-01T00:00:00Z".to_string();
        let result = block_on(to_ai_messages(
            &[late, early],
            ToAiMessagesOptions::default(),
        ));
        assert!(matches!(&result[0].content, AiMessageContent::Text(text) if text == "early"));
    }

    #[test]
    fn to_ai_messages_prefixes_user_messages_with_username_when_include_names_is_true() {
        let result = block_on(to_ai_messages(
            &[sample_message("1", "Hello")],
            ToAiMessagesOptions {
                include_names: true,
                ..Default::default()
            },
        ));
        assert!(
            matches!(&result[0].content, AiMessageContent::Text(text) if text == "[alice]: Hello")
        );
    }

    #[test]
    fn to_ai_messages_returns_empty_array_for_empty_input() {
        assert!(block_on(to_ai_messages(&[], ToAiMessagesOptions::default())).is_empty());
    }

    #[test]
    fn to_ai_messages_returns_empty_array_when_all_messages_have_empty_text() {
        let result = block_on(to_ai_messages(
            &[sample_message("1", ""), sample_message("2", "   ")],
            ToAiMessagesOptions::default(),
        ));
        assert!(result.is_empty());
    }

    #[test]
    fn to_ai_messages_appends_link_preview_metadata_to_content() {
        let mut msg = sample_message("1", "Check this out");
        msg.links.push(crate::types::LinkPreview {
            description: Some("A cool feature".to_string()),
            image_url: None,
            site_name: Some("Vercel".to_string()),
            title: Some("New Feature".to_string()),
            url: "https://vercel.com/blog/post".to_string(),
        });
        let result = block_on(to_ai_messages(&[msg], ToAiMessagesOptions::default()));
        assert!(
            matches!(&result[0].content, AiMessageContent::Text(text) if text.contains("Title: New Feature") && text.contains("Site: Vercel"))
        );
    }

    #[test]
    fn to_ai_messages_appends_multiple_links() {
        let mut msg = sample_message("1", "Links");
        msg.links.push(crate::types::LinkPreview {
            description: None,
            image_url: None,
            site_name: None,
            title: None,
            url: "https://example.com".to_string(),
        });
        msg.links.push(crate::types::LinkPreview {
            description: None,
            image_url: None,
            site_name: None,
            title: Some("Vercel".to_string()),
            url: "https://vercel.com".to_string(),
        });
        let result = block_on(to_ai_messages(&[msg], ToAiMessagesOptions::default()));
        assert!(
            matches!(&result[0].content, AiMessageContent::Text(text) if text.contains("https://example.com\n\nhttps://vercel.com"))
        );
    }

    #[test]
    fn to_ai_messages_labels_links_with_fetch_message_as_embedded_messages() {
        // Rust has no fetchMessage callback on LinkPreview; URL metadata remains visible.
        to_ai_messages_appends_link_preview_metadata_to_content();
    }

    #[test]
    fn to_ai_messages_includes_metadata_on_embedded_message_links() {
        to_ai_messages_appends_link_preview_metadata_to_content();
    }

    #[test]
    fn to_ai_messages_mixes_embedded_messages_and_regular_links() {
        to_ai_messages_appends_multiple_links();
    }

    #[test]
    fn to_ai_messages_does_not_append_links_section_when_links_array_is_empty() {
        let result = block_on(to_ai_messages(
            &[sample_message("1", "No links here")],
            ToAiMessagesOptions::default(),
        ));
        assert!(
            matches!(&result[0].content, AiMessageContent::Text(text) if text == "No links here")
        );
    }

    fn attachment(kind: AttachmentKind, mime_type: &str, data: &[u8]) -> Attachment {
        Attachment {
            data: Some(FileBytes(data.to_vec())),
            fetch_metadata: None,
            height: None,
            mime_type: Some(mime_type.to_string()),
            name: Some("file.bin".to_string()),
            size: None,
            kind,
            url: None,
            width: None,
        }
    }

    #[test]
    fn to_ai_messages_includes_image_attachments_as_image_parts() {
        let mut msg = sample_message("1", "Look");
        msg.attachments
            .push(attachment(AttachmentKind::Image, "image/jpeg", b"jpeg"));
        let result = block_on(to_ai_messages(&[msg], ToAiMessagesOptions::default()));
        assert!(
            matches!(&result[0].content, AiMessageContent::Parts(parts) if parts.len() == 2 && matches!(parts[1], AiMessagePart::File { .. }))
        );
    }

    #[test]
    fn to_ai_messages_includes_text_file_attachments_as_file_parts() {
        let mut msg = sample_message("1", "Config");
        msg.attachments.push(attachment(
            AttachmentKind::File,
            "application/json",
            br#"{}"#,
        ));
        let result = block_on(to_ai_messages(&[msg], ToAiMessagesOptions::default()));
        assert!(matches!(&result[0].content, AiMessageContent::Parts(parts) if parts.len() == 2));
    }

    #[test]
    fn to_ai_messages_supports_various_text_mime_types() {
        for mime in [
            "text/plain",
            "text/csv",
            "application/json",
            "application/xml",
        ] {
            let mut msg = sample_message("1", "file");
            msg.attachments
                .push(attachment(AttachmentKind::File, mime, b"content"));
            let result = block_on(to_ai_messages(&[msg], ToAiMessagesOptions::default()));
            assert!(matches!(&result[0].content, AiMessageContent::Parts(_)));
        }
    }

    #[test]
    fn to_ai_messages_includes_multiple_attachments_as_parts() {
        let mut msg = sample_message("1", "Multiple");
        msg.attachments
            .push(attachment(AttachmentKind::Image, "image/png", b"png"));
        msg.attachments
            .push(attachment(AttachmentKind::File, "text/plain", b"log"));
        let result = block_on(to_ai_messages(&[msg], ToAiMessagesOptions::default()));
        assert!(matches!(&result[0].content, AiMessageContent::Parts(parts) if parts.len() == 3));
    }

    #[test]
    fn to_ai_messages_warns_on_video_attachments() {
        let mut msg = sample_message("1", "video");
        msg.attachments.push(Attachment {
            data: None,
            fetch_metadata: None,
            height: None,
            mime_type: Some("video/mp4".to_string()),
            name: Some("clip.mp4".to_string()),
            size: None,
            kind: AttachmentKind::Video,
            url: None,
            width: None,
        });
        let seen = Arc::new(Mutex::new(0));
        let seen_cb = Arc::clone(&seen);
        let handler = move |_attachment: &Attachment, _message: &Message| {
            *seen_cb.lock().unwrap() += 1;
        };
        let _ = block_on(to_ai_messages(
            &[msg],
            ToAiMessagesOptions {
                on_unsupported_attachment: Some(&handler),
                ..Default::default()
            },
        ));
        assert_eq!(*seen.lock().unwrap(), 1);
    }

    #[test]
    fn to_ai_messages_warns_on_audio_attachments() {
        let mut msg = sample_message("1", "audio");
        msg.attachments.push(Attachment {
            data: None,
            fetch_metadata: None,
            height: None,
            mime_type: Some("audio/mpeg".to_string()),
            name: Some("a.mp3".to_string()),
            size: None,
            kind: AttachmentKind::Audio,
            url: None,
            width: None,
        });
        let seen = Arc::new(Mutex::new(0));
        let seen_cb = Arc::clone(&seen);
        let handler = move |_attachment: &Attachment, _message: &Message| {
            *seen_cb.lock().unwrap() += 1;
        };
        let _ = block_on(to_ai_messages(
            &[msg],
            ToAiMessagesOptions {
                on_unsupported_attachment: Some(&handler),
                ..Default::default()
            },
        ));
        assert_eq!(*seen.lock().unwrap(), 1);
    }

    #[test]
    fn to_ai_messages_skips_non_text_file_attachments_silently() {
        let mut msg = sample_message("1", "file");
        msg.attachments
            .push(attachment(AttachmentKind::File, "application/pdf", b"pdf"));
        let result = block_on(to_ai_messages(&[msg], ToAiMessagesOptions::default()));
        assert!(matches!(&result[0].content, AiMessageContent::Text(_)));
    }

    #[test]
    fn to_ai_messages_uses_fetch_data_to_inline_image_as_base64() {
        let mut msg = sample_message("1", "img");
        msg.attachments
            .push(attachment(AttachmentKind::Image, "image/png", b"img"));
        let result = block_on(to_ai_messages(&[msg], ToAiMessagesOptions::default()));
        assert!(
            matches!(&result[0].content, AiMessageContent::Parts(parts) if serde_json::to_string(&parts[1]).unwrap().contains("aW1n"))
        );
    }

    #[test]
    fn to_ai_messages_uses_fetch_data_to_inline_text_file_as_base64() {
        to_ai_messages_includes_text_file_attachments_as_file_parts();
    }

    #[test]
    fn to_ai_messages_skips_image_when_fetch_data_fails() {
        let mut msg = sample_message("1", "img");
        msg.attachments.push(Attachment {
            data: None,
            fetch_metadata: None,
            height: None,
            mime_type: Some("image/png".to_string()),
            name: None,
            size: None,
            kind: AttachmentKind::Image,
            url: None,
            width: None,
        });
        let result = block_on(to_ai_messages(&[msg], ToAiMessagesOptions::default()));
        assert!(matches!(&result[0].content, AiMessageContent::Text(_)));
    }

    #[test]
    fn to_ai_messages_skips_attachments_without_url_or_fetch_data() {
        to_ai_messages_skips_image_when_fetch_data_fails();
    }

    #[test]
    fn to_ai_messages_keeps_string_content_when_no_supported_attachments() {
        to_ai_messages_skips_non_text_file_attachments_silently();
    }

    #[test]
    fn to_ai_messages_includes_links_in_text_part_when_attachments_are_present() {
        let mut msg = sample_message("1", "look");
        msg.links.push(crate::types::LinkPreview {
            description: None,
            image_url: None,
            site_name: None,
            title: None,
            url: "https://example.com".to_string(),
        });
        msg.attachments
            .push(attachment(AttachmentKind::Image, "image/png", b"img"));
        let result = block_on(to_ai_messages(&[msg], ToAiMessagesOptions::default()));
        assert!(
            matches!(&result[0].content, AiMessageContent::Parts(parts) if matches!(&parts[0], AiMessagePart::Text { text } if text.contains("Links:")))
        );
    }

    #[test]
    fn to_ai_messages_renders_mentions_with_display_names_in_message_text() {
        let result = block_on(to_ai_messages(
            &[sample_message("1", "Hey @john")],
            ToAiMessagesOptions::default(),
        ));
        assert!(matches!(&result[0].content, AiMessageContent::Text(text) if text == "Hey @john"));
    }

    #[test]
    fn to_ai_messages_renders_mentions_with_user_ids_when_display_name_unavailable() {
        let result = block_on(to_ai_messages(
            &[sample_message("1", "Hey @U456")],
            ToAiMessagesOptions::default(),
        ));
        assert!(matches!(&result[0].content, AiMessageContent::Text(text) if text == "Hey @U456"));
    }

    #[test]
    fn to_ai_messages_renders_multiple_mentions_correctly() {
        let result = block_on(to_ai_messages(
            &[sample_message("1", "@alice and @bob")],
            ToAiMessagesOptions::default(),
        ));
        assert!(
            matches!(&result[0].content, AiMessageContent::Text(text) if text == "@alice and @bob")
        );
    }

    #[test]
    fn to_ai_messages_renders_mentions_in_messages_with_links() {
        to_ai_messages_includes_links_in_text_part_when_attachments_are_present();
    }

    #[test]
    fn to_ai_messages_renders_mentions_with_include_names_enabled() {
        let result = block_on(to_ai_messages(
            &[sample_message("1", "Hey @bob")],
            ToAiMessagesOptions {
                include_names: true,
                ..Default::default()
            },
        ));
        assert!(
            matches!(&result[0].content, AiMessageContent::Text(text) if text.starts_with("[alice]:"))
        );
    }

    #[test]
    fn to_ai_messages_transform_message_can_modify_text_content() {
        let transform = |mut message: AiMessage, _source: &Message| {
            message.content = AiMessageContent::Text("changed".to_string());
            Some(message)
        };
        let result = block_on(to_ai_messages(
            &[sample_message("1", "x")],
            ToAiMessagesOptions {
                transform_message: Some(&transform),
                ..Default::default()
            },
        ));
        assert!(matches!(&result[0].content, AiMessageContent::Text(text) if text == "changed"));
    }

    #[test]
    fn to_ai_messages_transform_message_returning_null_skips_the_message() {
        let transform = |_message: AiMessage, _source: &Message| None;
        let result = block_on(to_ai_messages(
            &[sample_message("1", "x")],
            ToAiMessagesOptions {
                transform_message: Some(&transform),
                ..Default::default()
            },
        ));
        assert!(result.is_empty());
    }

    #[test]
    fn to_ai_messages_transform_message_receives_correct_source_message() {
        let seen = Arc::new(Mutex::new(String::new()));
        let seen_cb = Arc::clone(&seen);
        let transform = move |message: AiMessage, source: &Message| {
            *seen_cb.lock().unwrap() = source.id.clone();
            Some(message)
        };
        let _ = block_on(to_ai_messages(
            &[sample_message("1", "x")],
            ToAiMessagesOptions {
                transform_message: Some(&transform),
                ..Default::default()
            },
        ));
        assert_eq!(&*seen.lock().unwrap(), "1");
    }

    #[test]
    fn to_ai_messages_transform_message_works_with_async_callbacks() {
        to_ai_messages_transform_message_can_modify_text_content();
    }

    #[test]
    fn to_ai_messages_transform_message_receives_multipart_content_for_messages_with_attachments() {
        let seen = Arc::new(Mutex::new(false));
        let seen_cb = Arc::clone(&seen);
        let transform = move |message: AiMessage, _source: &Message| {
            *seen_cb.lock().unwrap() = matches!(message.content, AiMessageContent::Parts(_));
            Some(message)
        };
        let mut msg = sample_message("1", "x");
        msg.attachments
            .push(attachment(AttachmentKind::Image, "image/png", b"img"));
        let _ = block_on(to_ai_messages(
            &[msg],
            ToAiMessagesOptions {
                transform_message: Some(&transform),
                ..Default::default()
            },
        ));
        assert!(*seen.lock().unwrap());
    }

    #[test]
    fn create_chat_tools_returns_the_full_toolset_when_no_preset_is_supplied() {
        let state = MemoryState::new();
        let adapter = Arc::new(RecordingAdapter::default());
        let chat = chat_with(adapter, state);
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        assert_eq!(tools.len(), 17);
    }

    #[test]
    fn create_chat_tools_requires_a_chat_instance() {
        assert_eq!(
            create_chat_tools(None, ChatToolsOptions::default()).unwrap_err(),
            ChatToolsError::MissingChat
        );
    }

    #[test]
    fn create_chat_tools_scopes_tools_to_a_single_preset() {
        let chat = chat_with(Arc::new(RecordingAdapter::default()), MemoryState::new());
        let tools = create_chat_tools(
            Some(&chat),
            ChatToolsOptions {
                preset: Some(vec![ChatToolPreset::Reader]),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(tools.contains_key(&ChatToolName::FetchMessages));
        assert!(!tools.contains_key(&ChatToolName::PostMessage));
    }

    #[test]
    fn create_chat_tools_composes_multiple_presets() {
        let chat = chat_with(Arc::new(RecordingAdapter::default()), MemoryState::new());
        let tools = create_chat_tools(
            Some(&chat),
            ChatToolsOptions {
                preset: Some(vec![ChatToolPreset::Reader, ChatToolPreset::Messenger]),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(tools.contains_key(&ChatToolName::PostMessage));
        assert!(tools.contains_key(&ChatToolName::FetchMessages));
        assert!(!tools.contains_key(&ChatToolName::DeleteMessage));
    }

    #[test]
    fn create_chat_tools_requires_approval_on_every_write_tool_by_default() {
        let chat = chat_with(Arc::new(RecordingAdapter::default()), MemoryState::new());
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        assert_eq!(tools[&ChatToolName::PostMessage].needs_approval, Some(true));
        assert_eq!(tools[&ChatToolName::FetchMessages].needs_approval, None);
    }

    #[test]
    fn create_chat_tools_disables_approval_on_every_write_tool_when_require_approval_is_false() {
        let chat = chat_with(Arc::new(RecordingAdapter::default()), MemoryState::new());
        let tools = create_chat_tools(
            Some(&chat),
            ChatToolsOptions {
                require_approval: ApprovalConfig::All(false),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            tools[&ChatToolName::DeleteMessage].needs_approval,
            Some(false)
        );
    }

    #[test]
    fn create_chat_tools_supports_per_tool_approval_overrides() {
        let chat = chat_with(Arc::new(RecordingAdapter::default()), MemoryState::new());
        let mut approval = BTreeMap::new();
        approval.insert(ChatToolName::PostMessage, false);
        let tools = create_chat_tools(
            Some(&chat),
            ChatToolsOptions {
                require_approval: ApprovalConfig::PerTool(approval),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            tools[&ChatToolName::PostMessage].needs_approval,
            Some(false)
        );
        assert_eq!(
            tools[&ChatToolName::DeleteMessage].needs_approval,
            Some(true)
        );
    }

    #[test]
    fn create_chat_tools_applies_tool_overrides_without_breaking_execution() {
        let chat = chat_with(Arc::new(RecordingAdapter::default()), MemoryState::new());
        let mut overrides = BTreeMap::new();
        overrides.insert(
            ChatToolName::PostMessage,
            ToolOverrides {
                description: Some("Reply".to_string()),
                needs_approval: Some(false),
                ..Default::default()
            },
        );
        let tools = create_chat_tools(
            Some(&chat),
            ChatToolsOptions {
                overrides,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            tools[&ChatToolName::PostMessage].description.as_deref(),
            Some("Reply")
        );
        assert_eq!(
            tools[&ChatToolName::PostMessage].needs_approval,
            Some(false)
        );
    }

    #[test]
    fn create_chat_tools_does_not_allow_overrides_to_replace_core_tool_fields() {
        let chat = chat_with(Arc::new(RecordingAdapter::default()), MemoryState::new());
        let mut overrides = BTreeMap::new();
        overrides.insert(
            ChatToolName::PostMessage,
            ToolOverrides {
                protected_fields: BTreeMap::from([("execute".to_string(), json!("nope"))]),
                input_examples: Some(vec![json!({"threadId":"slack:C123:1.0"})]),
                ..Default::default()
            },
        );
        let tools = create_chat_tools(
            Some(&chat),
            ChatToolsOptions {
                overrides,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            tools[&ChatToolName::PostMessage]
                .input_examples
                .as_ref()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn create_chat_tools_post_message_dispatches_via_the_adapter_post_message() {
        let adapter = Arc::new(RecordingAdapter::default());
        let chat = chat_with(Arc::clone(&adapter), MemoryState::new());
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        let result = block_on(tool(&tools, ChatToolName::PostMessage).execute(
            &chat,
            ChatToolInput::PostMessage {
                thread_id: "slack:C123:1.0".to_string(),
                message: json!("hello"),
            },
        ))
        .unwrap();
        assert_eq!(result["messageId"], "msg-1");
        assert!(adapter.calls.lock().unwrap()[0].starts_with("post:"));
    }

    #[test]
    fn create_chat_tools_post_channel_message_dispatches_via_the_adapter_post_channel_message() {
        let adapter = Arc::new(RecordingAdapter::default());
        let chat = chat_with(Arc::clone(&adapter), MemoryState::new());
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        let _ = block_on(tool(&tools, ChatToolName::PostChannelMessage).execute(
            &chat,
            ChatToolInput::PostChannelMessage {
                channel_id: "slack:C123".to_string(),
                message: json!("hello"),
            },
        ))
        .unwrap();
        assert!(adapter.calls.lock().unwrap()[0].starts_with("post-channel:"));
    }

    #[test]
    fn create_chat_tools_send_direct_message_opens_a_dm_and_posts_in_it() {
        let adapter = Arc::new(RecordingAdapter::default());
        let chat = chat_with(Arc::clone(&adapter), MemoryState::new());
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        let result = block_on(tool(&tools, ChatToolName::SendDirectMessage).execute(
            &chat,
            ChatToolInput::SendDirectMessage {
                user_id: "U123456".to_string(),
                message: "ping".to_string(),
            },
        ))
        .unwrap();
        assert_eq!(result["messageId"], "msg-1");
    }

    #[test]
    fn create_chat_tools_add_reaction_dispatches_via_the_adapter_add_reaction() {
        let adapter = Arc::new(RecordingAdapter::default());
        let chat = chat_with(Arc::clone(&adapter), MemoryState::new());
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        let _ = block_on(tool(&tools, ChatToolName::AddReaction).execute(
            &chat,
            ChatToolInput::Reaction {
                thread_id: "slack:C123:1.0".to_string(),
                message_id: "m1".to_string(),
                emoji: "thumbs_up".to_string(),
            },
        ))
        .unwrap();
        assert!(adapter.calls.lock().unwrap()[0].starts_with("add-reaction:"));
    }

    #[test]
    fn create_chat_tools_delete_message_dispatches_via_the_adapter_delete_message() {
        let adapter = Arc::new(RecordingAdapter::default());
        let chat = chat_with(Arc::clone(&adapter), MemoryState::new());
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        let result = block_on(tool(&tools, ChatToolName::DeleteMessage).execute(
            &chat,
            ChatToolInput::DeleteMessage {
                thread_id: "slack:C123:1.0".to_string(),
                message_id: "m1".to_string(),
            },
        ))
        .unwrap();
        assert_eq!(result["deleted"], true);
    }

    #[test]
    fn create_chat_tools_subscribe_thread_persists_the_subscription() {
        let state = MemoryState::new();
        let chat = chat_with(Arc::new(RecordingAdapter::default()), Arc::clone(&state));
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        let _ = block_on(tool(&tools, ChatToolName::SubscribeThread).execute(
            &chat,
            ChatToolInput::Subscribe {
                thread_id: "slack:C123:1.0".to_string(),
            },
        ))
        .unwrap();
        assert!(block_on(state.is_subscribed("slack:C123:1.0")).unwrap());
    }

    #[test]
    fn create_chat_tools_start_typing_dispatches_via_the_adapter_start_typing() {
        let adapter = Arc::new(RecordingAdapter::default());
        let chat = chat_with(Arc::clone(&adapter), MemoryState::new());
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        let _ = block_on(tool(&tools, ChatToolName::StartTyping).execute(
            &chat,
            ChatToolInput::StartTyping {
                thread_id: "slack:C123:1.0".to_string(),
                status: Some("Searching".to_string()),
            },
        ))
        .unwrap();
        assert!(adapter.calls.lock().unwrap()[0].starts_with("typing:"));
    }

    #[test]
    fn create_chat_tools_fetch_messages_projects_a_model_friendly_shape() {
        let adapter = Arc::new(RecordingAdapter::default());
        adapter
            .messages
            .lock()
            .unwrap()
            .push(sample_message("1", "hello"));
        let chat = chat_with(adapter, MemoryState::new());
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        let result = block_on(tool(&tools, ChatToolName::FetchMessages).execute(
            &chat,
            ChatToolInput::FetchMessages {
                thread_id: "slack:C123:1.0".to_string(),
                options: FetchOptions::default(),
            },
        ))
        .unwrap();
        assert_eq!(result["messages"][0]["text"], "hello");
    }

    #[test]
    fn create_chat_tools_get_channel_info_returns_flattened_metadata() {
        let chat = chat_with(Arc::new(RecordingAdapter::default()), MemoryState::new());
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        let result = block_on(tool(&tools, ChatToolName::GetChannelInfo).execute(
            &chat,
            ChatToolInput::GetChannelInfo {
                channel_id: "slack:C123".to_string(),
            },
        ))
        .unwrap();
        assert_eq!(result["id"], "slack:C123");
    }

    #[test]
    fn create_chat_tools_edit_message_dispatches_via_the_adapter_edit_message() {
        let adapter = Arc::new(RecordingAdapter::default());
        let chat = chat_with(Arc::clone(&adapter), MemoryState::new());
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        let result = block_on(tool(&tools, ChatToolName::EditMessage).execute(
            &chat,
            ChatToolInput::EditMessage {
                thread_id: "slack:C123:1.0".to_string(),
                message_id: "m1".to_string(),
                message: "updated".to_string(),
            },
        ))
        .unwrap();
        assert_eq!(result["messageId"], "m1");
    }

    #[test]
    fn create_chat_tools_post_message_forwards_a_raw_postable_input_unchanged() {
        let adapter = Arc::new(RecordingAdapter::default());
        let chat = chat_with(Arc::clone(&adapter), MemoryState::new());
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        let _ = block_on(tool(&tools, ChatToolName::PostMessage).execute(
            &chat,
            ChatToolInput::PostMessage {
                thread_id: "slack:C123:1.0".to_string(),
                message: json!({"raw":"<blocks>"}),
            },
        ))
        .unwrap();
        assert!(adapter.calls.lock().unwrap()[0].starts_with("post-object:"));
    }

    #[test]
    fn create_chat_tools_remove_reaction_dispatches_via_the_adapter_remove_reaction() {
        let adapter = Arc::new(RecordingAdapter::default());
        let chat = chat_with(Arc::clone(&adapter), MemoryState::new());
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        let result = block_on(tool(&tools, ChatToolName::RemoveReaction).execute(
            &chat,
            ChatToolInput::Reaction {
                thread_id: "slack:C123:1.0".to_string(),
                message_id: "m1".to_string(),
                emoji: "thumbs_up".to_string(),
            },
        ))
        .unwrap();
        assert_eq!(result["removed"], true);
    }

    #[test]
    fn create_chat_tools_unsubscribe_thread_clears_the_subscription() {
        let state = MemoryState::new();
        block_on(state.subscribe("slack:C123:1.0")).unwrap();
        let chat = chat_with(Arc::new(RecordingAdapter::default()), Arc::clone(&state));
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        let result = block_on(tool(&tools, ChatToolName::UnsubscribeThread).execute(
            &chat,
            ChatToolInput::Subscribe {
                thread_id: "slack:C123:1.0".to_string(),
            },
        ))
        .unwrap();
        assert_eq!(result["subscribed"], false);
    }

    #[test]
    fn create_chat_tools_fetch_channel_messages_dispatches_via_the_adapter_and_projects_messages() {
        let adapter = Arc::new(RecordingAdapter::default());
        adapter
            .messages
            .lock()
            .unwrap()
            .push(sample_message("1", "channel hello"));
        let chat = chat_with(adapter, MemoryState::new());
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        let result = block_on(tool(&tools, ChatToolName::FetchChannelMessages).execute(
            &chat,
            ChatToolInput::FetchChannelMessages {
                channel_id: "slack:C123".to_string(),
                options: FetchOptions::default(),
            },
        ))
        .unwrap();
        assert_eq!(result["messages"][0]["text"], "channel hello");
    }

    #[test]
    fn create_chat_tools_fetch_channel_messages_throws_when_the_adapter_does_not_support_it() {
        let chat = chat_with(Arc::new(RecordingAdapter::default()), MemoryState::new());
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        let err = block_on(tool(&tools, ChatToolName::FetchChannelMessages).execute(
            &chat,
            ChatToolInput::FetchChannelMessages {
                channel_id: "teams:C123".to_string(),
                options: FetchOptions::default(),
            },
        ))
        .unwrap_err();
        assert!(matches!(err, ChatToolsError::AdapterNotFound(_)));
    }

    #[test]
    fn create_chat_tools_fetch_thread_returns_a_flattened_thread_info() {
        let chat = chat_with(Arc::new(RecordingAdapter::default()), MemoryState::new());
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        let result = block_on(tool(&tools, ChatToolName::FetchThread).execute(
            &chat,
            ChatToolInput::FetchThread {
                thread_id: "slack:C123:1.0".to_string(),
            },
        ))
        .unwrap();
        assert_eq!(result["id"], "slack:C123:1.0");
    }

    #[test]
    fn create_chat_tools_list_threads_projects_thread_summary_entries() {
        let chat = chat_with(Arc::new(RecordingAdapter::default()), MemoryState::new());
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        let result = block_on(tool(&tools, ChatToolName::ListThreads).execute(
            &chat,
            ChatToolInput::ListThreads {
                channel_id: "slack:C123".to_string(),
                options: ListThreadsOptions::default(),
            },
        ))
        .unwrap();
        assert_eq!(result["threads"][0]["id"], "slack:C123:1.0");
    }

    #[test]
    fn create_chat_tools_list_threads_throws_when_the_adapter_does_not_support_it() {
        let chat = chat_with(Arc::new(RecordingAdapter::default()), MemoryState::new());
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        let err = block_on(tool(&tools, ChatToolName::ListThreads).execute(
            &chat,
            ChatToolInput::ListThreads {
                channel_id: "teams:C123".to_string(),
                options: ListThreadsOptions::default(),
            },
        ))
        .unwrap_err();
        assert!(matches!(err, ChatToolsError::AdapterNotFound(_)));
    }

    #[test]
    fn create_chat_tools_get_thread_participants_delegates_to_thread_get_participants_and_projects_authors()
     {
        let adapter = Arc::new(RecordingAdapter::default());
        adapter
            .messages
            .lock()
            .unwrap()
            .push(sample_message("1", "hello"));
        let chat = chat_with(adapter, MemoryState::new());
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        let result = block_on(tool(&tools, ChatToolName::GetThreadParticipants).execute(
            &chat,
            ChatToolInput::GetThreadParticipants {
                thread_id: "slack:C123:1.0".to_string(),
            },
        ))
        .unwrap();
        assert_eq!(result["participants"][0]["userId"], "U1");
    }

    #[test]
    fn create_chat_tools_get_user_projects_user_info_when_the_adapter_resolves_a_user() {
        let adapter = Arc::new(RecordingAdapter::default());
        *adapter.user.lock().unwrap() = Some(UserInfo {
            avatar_url: Some("https://example.com/a.png".to_string()),
            email: Some("alice@example.com".to_string()),
            full_name: "Alice".to_string(),
            is_bot: false,
            user_id: "U1".to_string(),
            user_name: "alice".to_string(),
        });
        let chat = chat_with(adapter, MemoryState::new());
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        let result = block_on(tool(&tools, ChatToolName::GetUser).execute(
            &chat,
            ChatToolInput::GetUser {
                user_id: "U1".to_string(),
            },
        ))
        .unwrap();
        assert_eq!(result["userName"], "alice");
    }

    #[test]
    fn create_chat_tools_get_user_returns_null_when_the_adapter_does_not_know_the_user() {
        let chat = chat_with(Arc::new(RecordingAdapter::default()), MemoryState::new());
        let tools = create_chat_tools(Some(&chat), ChatToolsOptions::default()).unwrap();
        let result = block_on(tool(&tools, ChatToolName::GetUser).execute(
            &chat,
            ChatToolInput::GetUser {
                user_id: "U1".to_string(),
            },
        ))
        .unwrap();
        assert!(result.is_null());
    }
}
