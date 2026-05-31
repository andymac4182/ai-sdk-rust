use std::{error::Error, fmt, sync::Arc};

use serde_json::Value;

use crate::context_errors::ContextViolationError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HookEntity {
    pub hook_id: String,
    pub token: String,
    pub run_id: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HookOptions;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Hook<TOutput> {
    pub output: Option<TOutput>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaIssue {
    pub message: String,
    pub path: Vec<String>,
}

impl SchemaIssue {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            path: Vec::new(),
        }
    }

    #[must_use]
    pub fn at_path(message: impl Into<String>, path: impl IntoIterator<Item = String>) -> Self {
        Self {
            message: message.into(),
            path: path.into_iter().collect(),
        }
    }
}

pub type SchemaValidator = dyn Fn(Value) -> Result<Value, Vec<SchemaIssue>> + Send + Sync;
pub type HookResumer = dyn Fn(&str, Value) -> Result<HookEntity, HookResumeError> + Send + Sync;

#[derive(Clone)]
pub struct TypedHook {
    schema: Option<Arc<SchemaValidator>>,
    resumer: Arc<HookResumer>,
}

impl TypedHook {
    #[must_use]
    pub fn new(schema: Option<Arc<SchemaValidator>>, resumer: Arc<HookResumer>) -> Self {
        Self { schema, resumer }
    }

    pub fn create(
        &self,
        _options: Option<HookOptions>,
    ) -> Result<Hook<Value>, ContextViolationError> {
        Err(ContextViolationError::not_in_workflow_context(
            "defineHook().create()",
            "https://workflow-sdk.dev/docs/api-reference/workflow/define-hook",
        ))
    }

    pub fn resume(&self, token: &str, payload: Value) -> Result<HookEntity, HookResumeError> {
        let payload = match &self.schema {
            Some(schema) => schema(payload).map_err(format_schema_issues)?,
            None => payload,
        };
        (self.resumer)(token, payload)
    }
}

#[must_use]
pub fn define_hook(schema: Option<Arc<SchemaValidator>>, resumer: Arc<HookResumer>) -> TypedHook {
    TypedHook::new(schema, resumer)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HookResumeError {
    message: String,
}

impl HookResumeError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for HookResumeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for HookResumeError {}

fn format_schema_issues(issues: Vec<SchemaIssue>) -> HookResumeError {
    let lines = issues
        .into_iter()
        .map(|issue| {
            if issue.path.is_empty() {
                format!("  {}", issue.message)
            } else {
                format!("  at \"{}\": {}", issue.path.join("."), issue.message)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    HookResumeError::new(format!(
        "Hook payload did not match the defined schema:\n{lines}"
    ))
}
