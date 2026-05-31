use std::{cell::RefCell, error::Error, fmt};

const BRANCH: &str = "\u{251c}\u{25b6}";
const LAST_BRANCH: &str = "\u{2570}\u{25b6}";

thread_local! {
    static WORKFLOW_CONTEXT: RefCell<Option<WorkflowMetadata>> = const { RefCell::new(None) };
}

/// Minimal workflow context metadata needed by context-violation diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkflowMetadata {
    pub workflow_name: String,
}

/// Runs a closure with workflow metadata visible to context-error constructors.
pub fn with_workflow_context<T>(metadata: WorkflowMetadata, callback: impl FnOnce() -> T) -> T {
    WORKFLOW_CONTEXT.with(|slot| {
        let previous = slot.replace(Some(metadata));
        let result = callback();
        slot.replace(previous);
        result
    })
}

fn current_workflow_name() -> Option<String> {
    WORKFLOW_CONTEXT.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|metadata| metadata.workflow_name.clone())
    })
}

/// Context violation error variants from upstream `context-violation-error.ts`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ContextViolationKind {
    NotInWorkflowContext,
    NotInStepContext,
    NotInWorkflowOrStepContext,
    UnavailableInWorkflowContext,
}

/// Structured context-violation error with a plain message and pretty render.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextViolationError {
    kind: ContextViolationKind,
    name: &'static str,
    message: String,
    pretty_message: String,
}

impl ContextViolationError {
    #[must_use]
    pub fn not_in_workflow_context(function_name: &str, docs_url: &str) -> Self {
        Self::from_parts(
            ContextViolationKind::NotInWorkflowContext,
            "NotInWorkflowContextError",
            vec![format!(
                "`{function_name}` can only be called inside a workflow function"
            )],
            vec![format!("docs: {docs_url}")],
        )
    }

    #[must_use]
    pub fn not_in_step_context(function_name: &str, docs_url: &str) -> Self {
        Self::from_parts(
            ContextViolationKind::NotInStepContext,
            "NotInStepContextError",
            vec![format!(
                "`{function_name}` can only be called inside a step function"
            )],
            vec![format!("docs: {docs_url}")],
        )
    }

    #[must_use]
    pub fn not_in_workflow_or_step_context(function_name: &str, docs_url: &str) -> Self {
        Self::from_parts(
            ContextViolationKind::NotInWorkflowOrStepContext,
            "NotInWorkflowOrStepContextError",
            vec![format!(
                "`{function_name}` can only be called inside a workflow or step function"
            )],
            vec![format!("docs: {docs_url}")],
        )
    }

    #[must_use]
    pub fn unavailable_in_workflow_context(function_name: &str, docs_url: &str) -> Self {
        let context_line = current_workflow_name().map_or_else(
            || "this call was made from a workflow context.".to_string(),
            |workflow_name| {
                format!("this call was made from the {workflow_name} workflow context.")
            },
        );

        Self::from_parts(
            ContextViolationKind::UnavailableInWorkflowContext,
            "UnavailableInWorkflowContextError",
            vec![format!(
                "`{function_name}` cannot be called from a workflow context."
            )],
            vec![
                "calling this in a workflow context can cause determinism issues.".to_string(),
                context_line,
                format!("docs: {docs_url}"),
            ],
        )
    }

    #[must_use]
    pub const fn kind(&self) -> ContextViolationKind {
        self.kind
    }

    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub fn pretty_message(&self) -> &str {
        &self.pretty_message
    }

    #[must_use]
    pub const fn is_fatal(&self) -> bool {
        true
    }

    fn from_parts(
        kind: ContextViolationKind,
        name: &'static str,
        title_lines: Vec<String>,
        detail_lines: Vec<String>,
    ) -> Self {
        let title = title_lines.join("\n");
        let message = render_framed(&title, &detail_lines);
        let pretty_message = format!("{name}: {message}");
        Self {
            kind,
            name,
            message,
            pretty_message,
        }
    }
}

impl fmt::Display for ContextViolationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for ContextViolationError {}

fn render_framed(title: &str, detail_lines: &[String]) -> String {
    let mut lines = vec![title.to_string()];
    for (index, detail) in detail_lines.iter().enumerate() {
        let prefix = if index + 1 == detail_lines.len() {
            LAST_BRANCH
        } else {
            BRANCH
        };
        lines.push(format!("{prefix} {detail}"));
    }
    lines.join("\n")
}

pub fn throw_not_in_workflow_context(
    function_name: &str,
    docs_url: &str,
) -> Result<(), ContextViolationError> {
    Err(ContextViolationError::not_in_workflow_context(
        function_name,
        docs_url,
    ))
}

pub fn throw_not_in_step_context(
    function_name: &str,
    docs_url: &str,
) -> Result<(), ContextViolationError> {
    Err(ContextViolationError::not_in_step_context(
        function_name,
        docs_url,
    ))
}

pub fn throw_not_in_workflow_or_step_context(
    function_name: &str,
    docs_url: &str,
) -> Result<(), ContextViolationError> {
    Err(ContextViolationError::not_in_workflow_or_step_context(
        function_name,
        docs_url,
    ))
}

pub fn throw_unavailable_in_workflow_context(
    function_name: &str,
    docs_url: &str,
) -> Result<(), ContextViolationError> {
    Err(ContextViolationError::unavailable_in_workflow_context(
        function_name,
        docs_url,
    ))
}
