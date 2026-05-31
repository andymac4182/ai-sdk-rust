use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use workflow_errors::WorkflowError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttributeChange {
    pub key: String,
    pub value: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExperimentalSetAttributesOptions {
    pub allow_reserved_attributes: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepContext {
    pub workflow_run_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExperimentalSetAttributesOutcome {
    Posted,
    Noop,
    UnsupportedWorld {
        warning_emitted: bool,
        warning: Option<String>,
    },
}

pub trait AttributeWorld: Send + Sync {
    fn name(&self) -> Option<&str> {
        None
    }

    fn supports_experimental_set_attributes(&self) -> bool {
        true
    }

    fn experimental_set_attributes(
        &self,
        run_id: &str,
        changes: &[AttributeChange],
        options: ExperimentalSetAttributesOptions,
    ) -> Result<(), WorkflowError>;
}

thread_local! {
    static STEP_CONTEXT: RefCell<Option<StepContext>> = const { RefCell::new(None) };
}

static ATTRIBUTE_WORLD: OnceLock<Mutex<Option<Arc<dyn AttributeWorld>>>> = OnceLock::new();
static UNSUPPORTED_WORLD_WARNED: AtomicBool = AtomicBool::new(false);

pub fn with_step_context<T>(context: StepContext, callback: impl FnOnce() -> T) -> T {
    STEP_CONTEXT.with(|slot| {
        let previous = slot.replace(Some(context));
        let result = callback();
        slot.replace(previous);
        result
    })
}

pub fn set_attribute_world(world: Arc<dyn AttributeWorld>) {
    let lock = ATTRIBUTE_WORLD.get_or_init(|| Mutex::new(None));
    *lock.lock().expect("attribute world mutex poisoned") = Some(world);
}

pub fn clear_attribute_world() {
    if let Some(lock) = ATTRIBUTE_WORLD.get() {
        *lock.lock().expect("attribute world mutex poisoned") = None;
    }
}

pub fn reset_unsupported_world_warning() {
    UNSUPPORTED_WORLD_WARNED.store(false, Ordering::SeqCst);
}

pub fn normalize_attribute_changes(
    attrs: BTreeMap<String, Option<String>>,
    options: ExperimentalSetAttributesOptions,
) -> Result<Vec<AttributeChange>, WorkflowError> {
    let changes = attrs
        .into_iter()
        .map(|(key, value)| AttributeChange { key, value })
        .collect::<Vec<_>>();

    if changes.is_empty() {
        return Ok(changes);
    }

    validate_attribute_changes(&changes, options)?;
    Ok(changes)
}

pub fn experimental_set_attributes(
    attrs: BTreeMap<String, Option<String>>,
    options: ExperimentalSetAttributesOptions,
) -> Result<ExperimentalSetAttributesOutcome, WorkflowError> {
    let run_id = STEP_CONTEXT.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|context| context.workflow_run_id.clone())
    });
    let Some(run_id) = run_id else {
        return Err(WorkflowError::fatal(
            "experimental_setAttributes() must be called from a 'use workflow' or 'use step' function. Calling it from plain host code is not supported.",
        ));
    };

    let changes = normalize_attribute_changes(attrs, options)?;
    if changes.is_empty() {
        return Ok(ExperimentalSetAttributesOutcome::Noop);
    }

    let world = ATTRIBUTE_WORLD
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("attribute world mutex poisoned")
        .clone()
        .ok_or_else(|| WorkflowError::workflow_runtime("workflow world is not configured"))?;

    if !world.supports_experimental_set_attributes() {
        let emitted = !UNSUPPORTED_WORLD_WARNED.swap(true, Ordering::SeqCst);
        let warning = emitted.then(|| {
            let world_name = world
                .name()
                .filter(|name| !name.is_empty())
                .map(|name| format!(" ({name})"))
                .unwrap_or_default();
            format!(
                "[workflow] setAttributes: the current world implementation{world_name} does not implement experimentalSetAttributes; this call (and any subsequent setAttributes calls in this process) is a no-op. Attributes will become available once the world adapter adds support."
            )
        });
        return Ok(ExperimentalSetAttributesOutcome::UnsupportedWorld {
            warning_emitted: emitted,
            warning,
        });
    }

    world.experimental_set_attributes(&run_id, &changes, options)?;
    Ok(ExperimentalSetAttributesOutcome::Posted)
}

fn validate_attribute_changes(
    changes: &[AttributeChange],
    options: ExperimentalSetAttributesOptions,
) -> Result<(), WorkflowError> {
    for change in changes {
        if change.key.is_empty() {
            return Err(WorkflowError::fatal(
                "Workflow attribute keys must not be empty",
            ));
        }
        if change.key.starts_with('$') && !options.allow_reserved_attributes {
            return Err(WorkflowError::fatal(format!(
                "Workflow attribute key \"{}\" uses the reserved \"$\" namespace",
                change.key
            )));
        }
    }
    Ok(())
}
