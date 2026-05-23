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

    // ---------- slice 93: Plan data struct (non-adapter portions) ----------

    #[test]
    fn plan_new_creates_a_plan_with_initial_first_task_in_progress() {
        let plan = Plan::new(StartPlanOptions {
            initial_message: "Investigate the bug".into(),
        });
        assert_eq!(plan.title(), "Investigate the bug");
        assert_eq!(plan.tasks().len(), 1);
        assert_eq!(plan.tasks()[0].status, PlanTaskStatus::InProgress);
    }

    #[test]
    fn plan_new_with_empty_initial_message_falls_back_to_default_title() {
        let plan = Plan::new(StartPlanOptions {
            initial_message: "".into(),
        });
        assert_eq!(plan.title(), "Plan");
    }

    #[test]
    fn plan_fallback_text_emits_clipboard_icon_plus_per_task_status_icon() {
        let plan = Plan::new(StartPlanOptions {
            initial_message: "Ship the release".into(),
        });
        let text = plan.fallback_text();
        assert!(text.starts_with("\u{1F4CB} Ship the release"));
        // First task is in_progress -> shows the rotation icon.
        assert!(text.contains("\u{1F504}"));
    }

    #[test]
    fn plan_current_task_returns_the_last_in_progress_task() {
        let plan = Plan::new(StartPlanOptions {
            initial_message: "Plan title".into(),
        });
        let current = plan.current_task().expect("plan always has a task");
        assert_eq!(current.status, PlanTaskStatus::InProgress);
        assert_eq!(current.title, "Plan title");
    }

    #[test]
    fn plan_post_data_returns_the_full_plan_model() {
        let plan = Plan::new(StartPlanOptions {
            initial_message: "Investigate".into(),
        });
        let data = plan.post_data();
        assert_eq!(data.title, "Investigate");
        assert_eq!(data.tasks.len(), 1);
    }

    #[test]
    fn plan_add_task_appends_pending_task_with_derived_title() {
        let mut plan = Plan::new(StartPlanOptions {
            initial_message: "Plan".into(),
        });
        plan.add_task(AddTaskOptions {
            children: None,
            title: "Step 2".into(),
        });
        assert_eq!(plan.tasks().len(), 2);
        assert_eq!(plan.tasks()[1].title, "Step 2");
        assert_eq!(plan.tasks()[1].status, PlanTaskStatus::Pending);
    }

    #[test]
    fn plan_update_task_in_model_targets_in_progress_task_by_default() {
        let mut plan = Plan::new(StartPlanOptions {
            initial_message: "Plan".into(),
        });
        plan.add_task(AddTaskOptions {
            children: None,
            title: "Step 2".into(),
        });
        let updated = plan
            .update_task_in_model(UpdateTaskInput::Content("output text".into()))
            .expect("found task");
        assert_eq!(updated.title, "Plan");
        assert!(updated.output.is_some());
    }

    #[test]
    fn plan_update_task_in_model_targets_specific_id_when_provided() {
        let mut plan = Plan::new(StartPlanOptions {
            initial_message: "Plan".into(),
        });
        let added_id = plan
            .add_task(AddTaskOptions {
                children: None,
                title: "Step 2".into(),
            })
            .id
            .clone();
        let updated = plan
            .update_task_in_model(UpdateTaskInput::Fields(UpdateTaskFields {
                id: Some(added_id.clone()),
                output: None,
                status: Some(PlanTaskStatus::Complete),
            }))
            .expect("found task by id");
        assert_eq!(updated.id, added_id);
        assert_eq!(updated.status, PlanTaskStatus::Complete);
    }

    #[test]
    fn streaming_plan_new_stores_stream_and_options() {
        let events = vec![serde_json::json!({"type": "text-delta", "textDelta": "hi"})];
        let plan = StreamingPlan::new(
            events.clone(),
            StreamingPlanOptions {
                group_tasks: Some(GroupTasksMode::Plan),
                update_interval_ms: Some(500),
                ..Default::default()
            },
        );
        assert_eq!(plan.kind(), "stream");
        assert_eq!(plan.fallback_text(), "");
        assert_eq!(plan.stream(), events.as_slice());
        assert_eq!(plan.options().group_tasks, Some(GroupTasksMode::Plan));
        assert_eq!(plan.options().update_interval_ms, Some(500));
    }

    #[test]
    fn group_tasks_mode_serializes_to_upstream_lowercase_strings() {
        assert_eq!(
            serde_json::to_string(&GroupTasksMode::Plan).unwrap(),
            "\"plan\""
        );
        assert_eq!(
            serde_json::to_string(&GroupTasksMode::Timeline).unwrap(),
            "\"timeline\""
        );
    }

    #[test]
    fn plan_complete_in_model_marks_non_error_tasks_as_complete() {
        let mut plan = Plan::new(StartPlanOptions {
            initial_message: "Plan".into(),
        });
        plan.add_task(AddTaskOptions {
            children: None,
            title: "Step 2".into(),
        });
        plan.complete_in_model(CompletePlanOptions {
            complete_message: "All done".into(),
        });
        assert_eq!(plan.title(), "All done");
        for task in plan.tasks() {
            assert_eq!(task.status, PlanTaskStatus::Complete);
        }
    }
}

/// In-memory plan with a task list. 1:1 port (in progress) of upstream
/// `class Plan implements PostableObject<PlanModel>`. The adapter-bound
/// dispatch (post / edit / add_task / update_task / complete) lives
/// behind the not-yet-extended Adapter trait and will land in a
/// follow-up slice. The portable surface — constructor, model
/// accessors, fallback text — ships now.
#[derive(Debug, Clone)]
pub struct Plan {
    model: PlanModel,
}

impl Plan {
    /// 1:1 port of upstream `new Plan(options): Plan`. Derives the
    /// plan title from `options.initial_message` via
    /// [`content_to_plain_text`], falling back to `"Plan"` for empty
    /// input. Creates a single seed task with the same title, status
    /// `in_progress`, and a stable identifier.
    pub fn new(options: StartPlanOptions) -> Self {
        let derived_title = content_to_plain_text(Some(&options.initial_message));
        let title = if derived_title.is_empty() {
            "Plan".to_string()
        } else {
            derived_title
        };
        let first_task = PlanModelTask {
            details: None,
            id: generate_task_id(),
            output: None,
            status: PlanTaskStatus::InProgress,
            title: title.clone(),
        };
        Self {
            model: PlanModel {
                tasks: vec![first_task],
                title,
            },
        }
    }

    /// Title of the plan. 1:1 with upstream `get title(): string`.
    pub fn title(&self) -> &str {
        &self.model.title
    }

    /// Snapshot of every task in render order. 1:1 with upstream
    /// `get tasks(): readonly PlanTask[]`. The returned [`PlanTask`]
    /// objects mirror the upstream readonly view (id + status + title
    /// only, no details/output).
    pub fn tasks(&self) -> Vec<PlanTask> {
        self.model
            .tasks
            .iter()
            .map(|t| PlanTask {
                id: t.id.clone(),
                status: t.status,
                title: t.title.clone(),
            })
            .collect()
    }

    /// Currently active task. 1:1 with upstream `get currentTask():
    /// PlanTask | null`. Walks tasks from end to start looking for
    /// the last `in_progress`, falling back to the final task.
    pub fn current_task(&self) -> Option<PlanTask> {
        let active = self
            .model
            .tasks
            .iter()
            .rev()
            .find(|t| t.status == PlanTaskStatus::InProgress)
            .or_else(|| self.model.tasks.last())?;
        Some(PlanTask {
            id: active.id.clone(),
            status: active.status,
            title: active.title.clone(),
        })
    }

    /// Plain-text fallback for adapters that don't support PlanModel.
    /// 1:1 port of upstream `getFallbackText(): string`. Output shape:
    /// `📋 Title\n<status-icon> task1\n<status-icon> task2\n…`.
    pub fn fallback_text(&self) -> String {
        let mut lines: Vec<String> = Vec::with_capacity(1 + self.model.tasks.len());
        let title = if self.model.title.is_empty() {
            "Plan"
        } else {
            self.model.title.as_str()
        };
        lines.push(format!("\u{1F4CB} {title}"));
        for task in &self.model.tasks {
            let icon = match task.status {
                PlanTaskStatus::Complete => "\u{2705}",
                PlanTaskStatus::InProgress => "\u{1F504}",
                PlanTaskStatus::Error => "\u{274C}",
                PlanTaskStatus::Pending => "\u{2B1C}",
            };
            lines.push(format!("{icon} {}", task.title));
        }
        lines.join("\n")
    }

    /// Snapshot of the underlying [`PlanModel`] for adapter dispatch.
    /// 1:1 port of upstream `getPostData(): PlanModel`.
    pub fn post_data(&self) -> PlanModel {
        self.model.clone()
    }

    /// Append a new task to the plan's in-memory model. 1:1 port of
    /// the model-update portion of upstream `addTask(options)`. The
    /// adapter-side card refresh is deferred until the Adapter trait
    /// extension; the in-memory model is updated immediately so
    /// future readers see the new task.
    pub fn add_task(&mut self, options: AddTaskOptions) -> &PlanModelTask {
        let title = content_to_plain_text(Some(&options.title));
        let task = PlanModelTask {
            details: options.children,
            id: generate_task_id(),
            output: None,
            status: PlanTaskStatus::Pending,
            title,
        };
        self.model.tasks.push(task);
        self.model.tasks.last().expect("just pushed a task")
    }

    /// Update the in-memory state of an existing task. 1:1 port of
    /// the model-update portion of upstream `updateTask(input)`.
    /// Returns `Some(&mut PlanModelTask)` when a matching task was
    /// found (by `id` if provided; otherwise the last in-progress
    /// task), `None` when nothing matched.
    pub fn update_task_in_model(&mut self, input: UpdateTaskInput) -> Option<&mut PlanModelTask> {
        let (target_id, output, status) = match input {
            UpdateTaskInput::Content(content) => (None, Some(content), None),
            UpdateTaskInput::Fields(f) => (f.id, f.output, f.status),
        };

        let position = if let Some(id) = target_id {
            self.model.tasks.iter().position(|t| t.id == id)
        } else {
            self.model
                .tasks
                .iter()
                .rposition(|t| t.status == PlanTaskStatus::InProgress)
        };

        let pos = position?;
        let task = &mut self.model.tasks[pos];
        if let Some(o) = output {
            task.output = Some(o);
        }
        if let Some(s) = status {
            task.status = s;
        }
        Some(task)
    }

    /// Mark the plan as complete, replacing the title with the supplied
    /// final message. 1:1 port of the model-update portion of upstream
    /// `complete({ completeMessage })`. All tasks not yet
    /// [`PlanTaskStatus::Error`] transition to
    /// [`PlanTaskStatus::Complete`].
    pub fn complete_in_model(&mut self, options: CompletePlanOptions) {
        let title = content_to_plain_text(Some(&options.complete_message));
        if !title.is_empty() {
            self.model.title = title;
        }
        for task in &mut self.model.tasks {
            if task.status != PlanTaskStatus::Error {
                task.status = PlanTaskStatus::Complete;
            }
        }
    }
}

/// Group-tasks display mode from [`StreamingPlanOptions`]. 1:1 port
/// of upstream `groupTasks?: "plan" | "timeline"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupTasksMode {
    /// All tasks grouped into a single plan block.
    Plan,
    /// Individual task cards shown inline with text (default).
    Timeline,
}

/// Options for [`StreamingPlan`]. 1:1 port of upstream
/// `interface StreamingPlanOptions`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StreamingPlanOptions {
    /// Block-Kit elements to attach when the stream stops (Slack only).
    #[serde(rename = "endWith", default, skip_serializing_if = "Option::is_none")]
    pub end_with: Option<Vec<serde_json::Value>>,
    /// Display grouping mode (Slack only).
    #[serde(
        rename = "groupTasks",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub group_tasks: Option<GroupTasksMode>,
    /// Minimum interval between updates in ms (default 500).
    #[serde(
        rename = "updateIntervalMs",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub update_interval_ms: Option<u64>,
}

/// A `StreamingPlan` wraps an async iterable with platform-specific
/// streaming options. 1:1 port (data-only) of upstream
/// `class StreamingPlan implements PostableObject<StreamingPlanData>`.
///
/// **Stream representation.** Upstream stores the stream as an
/// `AsyncIterable<string | StreamChunk | StreamEvent>`. The Rust port
/// holds the lowered `Vec<serde_json::Value>` (or an iterator that
/// produces them); adapters consuming a StreamingPlan should run the
/// values through [`crate::from_full_stream::from_full_stream`] to
/// extract text + StreamChunk yields. A future slice will swap the
/// `Vec` for `futures::Stream` once an async-runtime decision has
/// been made.
#[derive(Debug, Clone)]
pub struct StreamingPlan {
    options: StreamingPlanOptions,
    stream: Vec<serde_json::Value>,
}

impl StreamingPlan {
    /// 1:1 port of upstream
    /// `new StreamingPlan(stream, options = {}): StreamingPlan`.
    pub fn new(stream: Vec<serde_json::Value>, options: StreamingPlanOptions) -> Self {
        Self { options, stream }
    }

    /// Options pass-through. 1:1 with upstream `get options(): StreamingPlanOptions`.
    pub fn options(&self) -> &StreamingPlanOptions {
        &self.options
    }

    /// Stream pass-through. 1:1 with upstream
    /// `get stream(): AsyncIterable<...>`.
    pub fn stream(&self) -> &[serde_json::Value] {
        &self.stream
    }

    /// Upstream `kind` discriminator: always `"stream"`.
    pub fn kind(&self) -> &'static str {
        "stream"
    }

    /// Upstream `getFallbackText(): string` — always empty for a
    /// streaming plan.
    pub fn fallback_text(&self) -> String {
        String::new()
    }
}

fn generate_task_id() -> String {
    // Upstream uses crypto.randomUUID(); the Rust port uses the
    // existing `rand` workspace dep via a short hex token. Adopters
    // can swap in a UUID crate if uniqueness across processes matters
    // (the in-memory port only needs uniqueness within a single Plan).
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("task-{n:016x}")
}
