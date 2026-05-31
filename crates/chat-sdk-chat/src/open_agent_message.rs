//! Persisted Open Agents UI-message shapes.
//!
//! Open Agents stores chat history as AI SDK `UIMessage`-style JSON: each
//! message has a role and a list of typed parts (`text`, `tool-*`,
//! `dynamic-tool`, `data-*`, and so on). This module keeps that boundary
//! lossless while giving the Slack/runtime bridge typed access to the parts it
//! needs to route.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::message::Message;

/// Role of a persisted Open Agents UI message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpenAgentMessageRole {
    System,
    User,
    Assistant,
}

/// Persisted UI-message shape used by Open Agents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAgentUiMessage {
    pub id: String,
    pub role: OpenAgentMessageRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub parts: Vec<OpenAgentMessagePart>,
}

impl OpenAgentUiMessage {
    pub fn new(id: impl Into<String>, role: OpenAgentMessageRole) -> Self {
        Self {
            id: id.into(),
            role,
            metadata: None,
            parts: Vec::new(),
        }
    }

    pub fn with_metadata(mut self, metadata: impl Into<Value>) -> Self {
        self.metadata = Some(metadata.into());
        self
    }

    pub fn with_part(mut self, part: OpenAgentMessagePart) -> Self {
        self.parts.push(part);
        self
    }

    /// Build a user UI message from a cross-platform chat-sdk message.
    pub fn from_chat_message(message: &Message) -> Self {
        let mut metadata = Map::new();
        metadata.insert(
            "chatThreadId".to_string(),
            Value::String(message.thread_id.clone()),
        );
        metadata.insert(
            "chatMessageId".to_string(),
            Value::String(message.id.clone()),
        );
        if let Some(is_mention) = message.is_mention {
            metadata.insert("isMention".to_string(), Value::Bool(is_mention));
        }
        if let Some(user_key) = &message.user_key {
            metadata.insert("userKey".to_string(), Value::String(user_key.clone()));
        }

        Self::new(message.id.clone(), OpenAgentMessageRole::User)
            .with_metadata(Value::Object(metadata))
            .with_part(OpenAgentMessagePart::text(message.text.clone()))
    }
}

/// One persisted UI message part.
///
/// Serialization is transparent so unknown future AI SDK/Open Agents part
/// fields survive a read/write cycle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OpenAgentMessagePart {
    raw: Value,
}

impl OpenAgentMessagePart {
    pub fn new(raw: impl Into<Value>) -> Self {
        Self { raw: raw.into() }
    }

    pub fn text(text: impl Into<String>) -> Self {
        let mut object = Map::new();
        object.insert("type".to_string(), Value::String("text".to_string()));
        object.insert("text".to_string(), Value::String(text.into()));
        Self::new(Value::Object(object))
    }

    pub fn done_text(text: impl Into<String>) -> Self {
        let mut part = Self::text(text);
        part.insert_string("state", "done");
        part
    }

    pub fn data_snippet(
        id: impl Into<String>,
        filename: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        let mut data = Map::new();
        data.insert("filename".to_string(), Value::String(filename.into()));
        data.insert("content".to_string(), Value::String(content.into()));

        let mut object = Map::new();
        object.insert(
            "type".to_string(),
            Value::String("data-snippet".to_string()),
        );
        object.insert("id".to_string(), Value::String(id.into()));
        object.insert("data".to_string(), Value::Object(data));
        Self::new(Value::Object(object))
    }

    pub fn raw(&self) -> &Value {
        &self.raw
    }

    pub fn into_raw(self) -> Value {
        self.raw
    }

    pub fn part_type(&self) -> Option<&str> {
        self.raw
            .as_object()
            .and_then(|object| object.get("type"))
            .and_then(Value::as_str)
    }

    pub fn classify(&self) -> OpenAgentMessagePartKind {
        let Some(part_type) = self.part_type() else {
            return OpenAgentMessagePartKind::Other(self.raw.clone());
        };

        if part_type == "text" {
            return OpenAgentMessagePartKind::Text(OpenAgentTextPart {
                text: string_field(&self.raw, "text").unwrap_or_default(),
                state: optional_string_field(&self.raw, "state"),
            });
        }

        if part_type.starts_with("data-") {
            return OpenAgentMessagePartKind::Data(classify_data_part(part_type, &self.raw));
        }

        if part_type == "dynamic-tool" || part_type.starts_with("tool-") {
            let tool = classify_tool_part(part_type, &self.raw);
            if tool.tool_name.as_deref() == Some("ask_user_question") {
                return OpenAgentMessagePartKind::Question(OpenAgentQuestionPart { tool });
            }
            if tool.approval.is_some()
                || matches!(
                    tool.state.as_deref(),
                    Some("approval-requested" | "approval-responded")
                )
            {
                return OpenAgentMessagePartKind::Approval(OpenAgentApprovalPart { tool });
            }
            return OpenAgentMessagePartKind::Tool(tool);
        }

        OpenAgentMessagePartKind::Other(self.raw.clone())
    }

    fn insert_string(&mut self, key: &str, value: &str) {
        if let Value::Object(object) = &mut self.raw {
            object.insert(key.to_string(), Value::String(value.to_string()));
        }
    }
}

impl From<Value> for OpenAgentMessagePart {
    fn from(value: Value) -> Self {
        Self::new(value)
    }
}

impl From<OpenAgentMessagePart> for Value {
    fn from(part: OpenAgentMessagePart) -> Self {
        part.raw
    }
}

/// Classified view of a persisted UI part.
#[derive(Debug, Clone, PartialEq)]
pub enum OpenAgentMessagePartKind {
    Text(OpenAgentTextPart),
    Tool(OpenAgentToolPart),
    Question(OpenAgentQuestionPart),
    Approval(OpenAgentApprovalPart),
    Data(OpenAgentDataPart),
    Other(Value),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAgentTextPart {
    pub text: String,
    pub state: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpenAgentQuestionPart {
    pub tool: OpenAgentToolPart,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpenAgentApprovalPart {
    pub tool: OpenAgentToolPart,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpenAgentToolPart {
    pub part_type: String,
    pub tool_name: Option<String>,
    pub tool_call_id: Option<String>,
    pub state: Option<String>,
    pub input: Option<Value>,
    pub output: Option<Value>,
    pub error_text: Option<String>,
    pub approval: Option<OpenAgentToolApproval>,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAgentToolApproval {
    pub id: String,
    pub approved: Option<bool>,
    pub reason: Option<String>,
    pub is_automatic: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OpenAgentDataPart {
    Commit(OpenAgentCommitData),
    PullRequest(OpenAgentPrData),
    Snippet(OpenAgentSnippetData),
    WorkspaceStatus(OpenAgentWorkspaceStatusData),
    Other {
        data_type: String,
        id: Option<String>,
        data: Option<Value>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAgentCommitData {
    pub status: OpenAgentGitDataStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub committed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pushed: Option<bool>,
    #[serde(
        rename = "commitMessage",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub commit_message: Option<String>,
    #[serde(rename = "commitSha", default, skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpenAgentGitDataStatus {
    Pending,
    Success,
    Error,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAgentPrData {
    pub status: OpenAgentGitDataStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<bool>,
    #[serde(
        rename = "syncedExisting",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub synced_existing: Option<bool>,
    #[serde(rename = "prNumber", default, skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(
        rename = "skipReason",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub skip_reason: Option<String>,
    #[serde(
        rename = "requiresManualCreation",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub requires_manual_creation: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAgentSnippetData {
    pub content: String,
    pub filename: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAgentWorkspaceStatusData {
    pub status: OpenAgentWorkspaceStatus,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpenAgentWorkspaceStatus {
    SettingUp,
}

fn classify_tool_part(part_type: &str, raw: &Value) -> OpenAgentToolPart {
    let tool_name = if part_type == "dynamic-tool" {
        optional_string_field(raw, "toolName")
    } else {
        part_type.strip_prefix("tool-").map(ToString::to_string)
    };

    OpenAgentToolPart {
        part_type: part_type.to_string(),
        tool_name,
        tool_call_id: optional_string_field(raw, "toolCallId"),
        state: optional_string_field(raw, "state"),
        input: raw.get("input").cloned(),
        output: raw.get("output").cloned(),
        error_text: optional_string_field(raw, "errorText"),
        approval: classify_tool_approval(raw),
        raw: raw.clone(),
    }
}

fn classify_tool_approval(raw: &Value) -> Option<OpenAgentToolApproval> {
    let object = raw.get("approval")?.as_object()?;
    let id = object.get("id")?.as_str()?.to_string();
    Some(OpenAgentToolApproval {
        id,
        approved: object.get("approved").and_then(Value::as_bool),
        reason: object
            .get("reason")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        is_automatic: object.get("isAutomatic").and_then(Value::as_bool),
    })
}

fn classify_data_part(part_type: &str, raw: &Value) -> OpenAgentDataPart {
    let data = raw.get("data").cloned();
    match (part_type, data.clone()) {
        ("data-commit", Some(value)) => serde_json::from_value(value)
            .map(OpenAgentDataPart::Commit)
            .unwrap_or_else(|_| other_data_part(part_type, raw, data)),
        ("data-pr", Some(value)) => serde_json::from_value(value)
            .map(OpenAgentDataPart::PullRequest)
            .unwrap_or_else(|_| other_data_part(part_type, raw, data)),
        ("data-snippet", Some(value)) => serde_json::from_value(value)
            .map(OpenAgentDataPart::Snippet)
            .unwrap_or_else(|_| other_data_part(part_type, raw, data)),
        ("data-workspace-status", Some(value)) => serde_json::from_value(value)
            .map(OpenAgentDataPart::WorkspaceStatus)
            .unwrap_or_else(|_| other_data_part(part_type, raw, data)),
        _ => other_data_part(part_type, raw, data),
    }
}

fn other_data_part(part_type: &str, raw: &Value, data: Option<Value>) -> OpenAgentDataPart {
    OpenAgentDataPart::Other {
        data_type: part_type.to_string(),
        id: optional_string_field(raw, "id"),
        data,
    }
}

fn string_field(raw: &Value, field: &str) -> Option<String> {
    raw.get(field)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn optional_string_field(raw: &Value, field: &str) -> Option<String> {
    string_field(raw, field).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{
        OpenAgentDataPart, OpenAgentMessagePartKind, OpenAgentMessageRole, OpenAgentUiMessage,
    };

    #[test]
    fn open_agent_persisted_message_fixture_round_trips_losslessly() {
        let fixture = include_str!("fixtures/open-agent-persisted-messages.json");
        let original: serde_json::Value = serde_json::from_str(fixture).unwrap();
        let messages: Vec<OpenAgentUiMessage> = serde_json::from_str(fixture).unwrap();
        let roundtrip = serde_json::to_value(&messages).unwrap();

        assert_eq!(roundtrip, original);
    }

    #[test]
    fn open_agent_persisted_parts_classify_tools_questions_approvals_and_data() {
        let fixture = include_str!("fixtures/open-agent-persisted-messages.json");
        let messages: Vec<OpenAgentUiMessage> = serde_json::from_str(fixture).unwrap();

        assert_eq!(messages[0].role, OpenAgentMessageRole::User);
        assert!(matches!(
            messages[0].parts[0].classify(),
            OpenAgentMessagePartKind::Text(_)
        ));
        assert!(matches!(
            messages[0].parts[1].classify(),
            OpenAgentMessagePartKind::Data(OpenAgentDataPart::Snippet(_))
        ));

        assert!(matches!(
            messages[1].parts[0].classify(),
            OpenAgentMessagePartKind::Tool(_)
        ));
        assert!(matches!(
            messages[1].parts[1].classify(),
            OpenAgentMessagePartKind::Question(_)
        ));
        assert!(matches!(
            messages[2].parts[0].classify(),
            OpenAgentMessagePartKind::Approval(_)
        ));
        assert!(matches!(
            messages[3].parts[1].classify(),
            OpenAgentMessagePartKind::Data(OpenAgentDataPart::Commit(_))
        ));
        assert!(matches!(
            messages[3].parts[2].classify(),
            OpenAgentMessagePartKind::Data(OpenAgentDataPart::PullRequest(_))
        ));
        assert!(matches!(
            messages[3].parts[3].classify(),
            OpenAgentMessagePartKind::Data(OpenAgentDataPart::WorkspaceStatus(_))
        ));
    }
}
