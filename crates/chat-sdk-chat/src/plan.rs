//! Plan data types + content normalization.
//!
//! 1:1 port (in progress) of `packages/chat/src/plan.ts`.
//!
//! **What this slice ships:** the upstream plan TYPE surface
//! (`PlanTaskStatus`, `PlanTask`, `PlanModel`, `PlanModelTask`,
//! `PlanContent`, `StartPlanOptions`, `AddTaskOptions`,
//! `UpdateTaskInput`, `CompletePlanOptions`) and the
//! [`content_to_plain_text`] normalizer used by the upstream
//! `Plan` class to derive titles/labels from the four `PlanContent`
//! variants.
//!
//! **What is deferred:** the `Plan` class itself (and
//! `StreamingPlan`) — both implement `PostableObject` and bind an
//! `Adapter` lifecycle. They land once those traits are extended
//! beyond the current placeholder.

use serde::{Deserialize, Serialize};

use crate::markdown::{Root, markdown_to_plain_text, to_plain_text};

/// Status of a single [`PlanTask`]. 1:1 port of upstream
/// `export type PlanTaskStatus = "pending" | "in_progress" |
/// "complete" | "error"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanTaskStatus {
    /// Task has not been started.
    Pending,
    /// Task is currently running.
    InProgress,
    /// Task finished successfully.
    Complete,
    /// Task hit an error.
    Error,
}

/// One entry in the user-facing plan view. 1:1 port of upstream
/// `interface PlanTask { id; status; title }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanTask {
    /// Stable task identifier.
    pub id: String,
    /// Current status.
    pub status: PlanTaskStatus,
    /// Display title.
    pub title: String,
}

/// Richer per-task model carried by adapters that render full plans.
/// 1:1 port of upstream `interface PlanModelTask`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanModelTask {
    /// Optional details / substeps for the task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<PlanContent>,
    /// Stable task identifier.
    pub id: String,
    /// Task output / results.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<PlanContent>,
    /// Status.
    pub status: PlanTaskStatus,
    /// Display title.
    pub title: String,
}

/// Full plan model. 1:1 port of upstream
/// `interface PlanModel { tasks; title }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanModel {
    /// Tasks in render order.
    pub tasks: Vec<PlanModelTask>,
    /// Plan title.
    pub title: String,
}

/// Free-form plan content. 1:1 port of upstream
/// `export type PlanContent = string | string[] | { markdown: string }
/// | { ast: Root }`.
///
/// Modeled as a `#[serde(untagged)]` enum: variant order is `String`,
/// `Strings`, `Markdown`, `Ast` so that the more specific shapes match
/// first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PlanContent {
    /// Single string of plain text.
    Text(String),
    /// Multiple strings (joined with `" "` by [`content_to_plain_text`]).
    Strings(Vec<String>),
    /// `{ markdown }` payload — adapter parses to mdast before render.
    Markdown(PlanMarkdownContent),
    /// `{ ast }` payload — pre-parsed mdast root.
    Ast(PlanAstContent),
}

/// Helper struct so [`PlanContent::Markdown`] serializes as
/// `{ "markdown": "..." }`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlanMarkdownContent {
    /// Markdown source.
    pub markdown: String,
}

/// Helper struct so [`PlanContent::Ast`] serializes as
/// `{ "ast": { ... mdast Root ... } }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanAstContent {
    /// mdast Root node.
    pub ast: Root,
}

impl From<&str> for PlanContent {
    fn from(value: &str) -> Self {
        Self::Text(value.to_string())
    }
}

impl From<String> for PlanContent {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<Vec<String>> for PlanContent {
    fn from(value: Vec<String>) -> Self {
        Self::Strings(value)
    }
}

/// Options for upstream `new Plan({ initialMessage })`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartPlanOptions {
    /// Initial plan title and first task title.
    #[serde(rename = "initialMessage")]
    pub initial_message: PlanContent,
}

/// Options for `plan.addTask(...)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AddTaskOptions {
    /// Optional task details / substeps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub children: Option<PlanContent>,
    /// Task title.
    pub title: PlanContent,
}

/// Update payload accepted by `plan.updateTask(...)`. 1:1 port of
/// upstream `type UpdateTaskInput = PlanContent | { id?; output?; status? }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UpdateTaskInput {
    /// Object form with explicit fields.
    Fields(UpdateTaskFields),
    /// Plain content — applied to the last in-progress task as output.
    Content(PlanContent),
}

/// Object variant of [`UpdateTaskInput`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateTaskFields {
    /// Optional task id; when omitted, the last in-progress task is
    /// updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Task output / results.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<PlanContent>,
    /// Optional status override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<PlanTaskStatus>,
}

/// Options for `plan.complete(...)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompletePlanOptions {
    /// Final plan title shown when completed.
    #[serde(rename = "completeMessage")]
    pub complete_message: PlanContent,
}

/// Convert [`PlanContent`] to plain text for titles/labels. 1:1 port
/// of upstream `contentToPlainText`. Passing `None` returns `""`,
/// matching upstream's `if (!content) return ""` guard.
pub fn content_to_plain_text(content: Option<&PlanContent>) -> String {
    match content {
        None => String::new(),
        Some(PlanContent::Text(s)) => s.clone(),
        Some(PlanContent::Strings(items)) => items.join(" ").trim().to_string(),
        Some(PlanContent::Markdown(m)) => markdown_to_plain_text(&m.markdown),
        Some(PlanContent::Ast(a)) => to_plain_text(&crate::markdown::Node::Root(a.ast.clone())),
    }
}

#[cfg(test)]
mod tests {
    //! Additive coverage for the plan types. Upstream ships no
    //! `plan.test.ts`; the Plan class is exercised through integration
    //! tests. These Rust tests lock in the wire shape + the four
    //! `content_to_plain_text` branches.
    use super::*;
    use serde_json::json;

    #[test]
    fn plan_task_status_uses_upstream_snake_case_strings() {
        for (value, wire) in [
            (PlanTaskStatus::Pending, "\"pending\""),
            (PlanTaskStatus::InProgress, "\"in_progress\""),
            (PlanTaskStatus::Complete, "\"complete\""),
            (PlanTaskStatus::Error, "\"error\""),
        ] {
            assert_eq!(serde_json::to_string(&value).unwrap(), wire);
        }
    }

    #[test]
    fn plan_task_round_trips_through_serde() {
        let task = PlanTask {
            id: "t1".to_string(),
            status: PlanTaskStatus::InProgress,
            title: "Step 1".to_string(),
        };
        let json = serde_json::to_string(&task).unwrap();
        assert!(json.contains("\"in_progress\""));
        let back: PlanTask = serde_json::from_str(&json).unwrap();
        assert_eq!(back, task);
    }

    #[test]
    fn plan_content_dispatches_each_variant_through_untagged_serde() {
        let text: PlanContent = serde_json::from_str("\"hello\"").unwrap();
        assert!(matches!(text, PlanContent::Text(_)));

        let strings: PlanContent = serde_json::from_str("[\"a\", \"b\"]").unwrap();
        assert!(matches!(strings, PlanContent::Strings(_)));

        let markdown: PlanContent = serde_json::from_value(json!({"markdown": "**hi**"})).unwrap();
        assert!(matches!(markdown, PlanContent::Markdown(_)));

        let ast = json!({"ast": {"type": "root", "children": []}});
        let parsed: PlanContent = serde_json::from_value(ast).unwrap();
        assert!(matches!(parsed, PlanContent::Ast(_)));
    }

    #[test]
    fn content_to_plain_text_returns_empty_when_content_is_none() {
        assert_eq!(content_to_plain_text(None), "");
    }

    #[test]
    fn content_to_plain_text_passes_through_plain_strings() {
        let c = PlanContent::Text("hello".to_string());
        assert_eq!(content_to_plain_text(Some(&c)), "hello");
    }

    #[test]
    fn content_to_plain_text_joins_string_arrays_with_spaces() {
        let c = PlanContent::Strings(vec!["  step one ".to_string(), "step two".to_string()]);
        // Upstream joins with " " then `.trim()`s the whole result.
        assert_eq!(content_to_plain_text(Some(&c)), "step one  step two");
    }

    #[test]
    fn content_to_plain_text_parses_markdown_into_plain_text() {
        let c = PlanContent::Markdown(PlanMarkdownContent {
            markdown: "**bold** plain".to_string(),
        });
        let plain = content_to_plain_text(Some(&c));
        assert!(plain.contains("bold"));
        assert!(plain.contains("plain"));
    }

    #[test]
    fn content_to_plain_text_extracts_ast_text() {
        let ast_json = json!({
            "type": "root",
            "children": [
                {
                    "type": "paragraph",
                    "children": [
                        { "type": "text", "value": "hello AST" }
                    ]
                }
            ]
        });
        let ast: Root = serde_json::from_value(ast_json).unwrap();
        let c = PlanContent::Ast(PlanAstContent { ast });
        assert_eq!(content_to_plain_text(Some(&c)), "hello AST");
    }

    #[test]
    fn update_task_input_supports_both_content_and_object_forms() {
        let content: UpdateTaskInput = serde_json::from_str("\"just text\"").unwrap();
        assert!(matches!(content, UpdateTaskInput::Content(_)));

        let fields: UpdateTaskInput =
            serde_json::from_value(json!({"id": "t1", "status": "complete"})).unwrap();
        assert!(matches!(fields, UpdateTaskInput::Fields(_)));
    }
}
