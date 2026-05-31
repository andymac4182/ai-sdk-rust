use std::{collections::BTreeMap, error::Error, fmt};

use serde::{Deserialize, Serialize};
use workflow_errors::WorkflowError;

use crate::{schemas::Serializable, util::pluralize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepInvocationQueueItem {
    pub correlation_id: String,
    pub step_name: String,
    pub args: Vec<Serializable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closure_vars: Option<BTreeMap<String, Serializable>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub this_val: Option<Serializable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_created_event: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookInvocationQueueItem {
    pub correlation_id: String,
    pub token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Serializable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_created_event: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disposed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_webhook: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_system: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abort_requested: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abort_reason: Option<Serializable>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitInvocationQueueItem {
    pub correlation_id: String,
    pub resume_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_created_event: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum QueueItem {
    #[serde(rename = "step")]
    Step(StepInvocationQueueItem),
    #[serde(rename = "hook")]
    Hook(HookInvocationQueueItem),
    #[serde(rename = "wait")]
    Wait(WaitInvocationQueueItem),
}

/// Error thrown to suspend a workflow until queued operations complete.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkflowSuspension {
    message: String,
    pub steps: Vec<QueueItem>,
    pub step_count: usize,
    pub hook_count: usize,
    pub wait_count: usize,
    pub hook_disposed_count: usize,
    pub abort_count: usize,
}

impl WorkflowSuspension {
    #[must_use]
    pub fn new(steps: impl IntoIterator<Item = QueueItem>) -> Self {
        let steps: Vec<_> = steps.into_iter().collect();
        let mut step_count = 0;
        let mut hook_count = 0;
        let mut wait_count = 0;
        let mut hook_disposed_count = 0;
        let mut abort_count = 0;

        for item in &steps {
            match item {
                QueueItem::Step(_) => step_count += 1,
                QueueItem::Wait(_) => wait_count += 1,
                QueueItem::Hook(hook) if hook.disposed == Some(true) => {
                    hook_disposed_count += 1;
                }
                QueueItem::Hook(hook) if hook.abort_requested == Some(true) => {
                    abort_count += 1;
                }
                QueueItem::Hook(_) => hook_count += 1,
            }
        }

        let message = build_message(step_count, hook_count, wait_count, hook_disposed_count);

        Self {
            message,
            steps,
            step_count,
            hook_count,
            wait_count,
            hook_disposed_count,
            abort_count,
        }
    }

    #[must_use]
    pub fn name(&self) -> &'static str {
        "WorkflowSuspension"
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for WorkflowSuspension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for WorkflowSuspension {}

fn build_message(
    step_count: usize,
    hook_count: usize,
    wait_count: usize,
    hook_disposed_count: usize,
) -> String {
    let mut parts = Vec::new();
    if step_count > 0 {
        parts.push(format!(
            "{step_count} {}",
            pluralize("step", "steps", step_count)
        ));
    }
    if hook_count > 0 {
        parts.push(format!(
            "{hook_count} {}",
            pluralize("hook", "hooks", hook_count)
        ));
    }
    if wait_count > 0 {
        parts.push(format!(
            "{wait_count} {}",
            pluralize("wait", "waits", wait_count)
        ));
    }
    if hook_disposed_count > 0 {
        parts.push(format!(
            "{hook_disposed_count} hook {}",
            pluralize("disposal", "disposals", hook_disposed_count)
        ));
    }

    let total_count = step_count + hook_count + wait_count + hook_disposed_count;
    let has_or_have = pluralize("has", "have", total_count);
    let type_count = usize::from(step_count > 0)
        + usize::from(hook_count > 0)
        + usize::from(wait_count > 0)
        + usize::from(hook_disposed_count > 0);

    let action = if type_count > 1 {
        "processed"
    } else if step_count > 0 {
        "run"
    } else if hook_count > 0 || wait_count > 0 {
        "created"
    } else if hook_disposed_count > 0 {
        "processed"
    } else {
        "received"
    };

    if parts.is_empty() {
        "0 steps have not been run yet".to_string()
    } else {
        format!(
            "{} {has_or_have} not been {action} yet",
            parts.join(" and ")
        )
    }
}

#[must_use]
pub fn enotsup() -> WorkflowError {
    WorkflowError::workflow_runtime(
        "This API is not available inside a workflow function. Workflow functions run in a deterministic VM; move the call to a step function for full Node.js access.",
    )
}
