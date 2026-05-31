//! Internal builtins exposed for runtime wiring and parity tests.

use std::{error::Error, fmt};

/// Upstream retry ceiling for the internal attribute writer step.
pub const INTERNAL_ATTRIBUTES_MAX_ATTEMPTS: u32 = 3;

/// Process-wide warning state for worlds that do not implement attributes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BuiltinSetAttributesState {
    unsupported_world_warned: bool,
}

impl BuiltinSetAttributesState {
    /// Returns whether the unsupported-world warning has been emitted.
    pub fn unsupported_world_warned(&self) -> bool {
        self.unsupported_world_warned
    }
}

/// Attribute change accepted by the internal set-attributes builtin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeChange {
    /// Attribute key.
    pub key: String,
    /// New value, or `None` to unset.
    pub value: Option<String>,
}

impl AttributeChange {
    /// Creates an attribute upsert.
    pub fn set(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: Some(value.into()),
        }
    }

    /// Creates an attribute unset.
    pub fn unset(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: None,
        }
    }
}

/// Options accepted by the internal set-attributes builtin.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SetAttributesOptions {
    /// Whether reserved attribute keys may be written.
    pub allow_reserved_attributes: bool,
}

/// Step context read by the internal set-attributes builtin.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuiltinStepContext {
    /// Current step attempt. Missing attempts follow upstream and are treated
    /// as the final internal attempt.
    pub attempt: Option<u32>,
    /// Current workflow run id.
    pub workflow_run_id: Option<String>,
}

impl BuiltinStepContext {
    /// Creates a context with attempt and run id.
    pub fn new(attempt: u32, workflow_run_id: impl Into<String>) -> Self {
        Self {
            attempt: Some(attempt),
            workflow_run_id: Some(workflow_run_id.into()),
        }
    }
}

/// Logging sink for the internal builtin.
pub trait SetAttributesLogger {
    /// Emits a warning.
    fn warn(&mut self, message: &str);

    /// Emits an error.
    fn error(&mut self, message: &str);
}

/// World adapter support needed by the internal builtin.
pub trait AttributeWorld {
    /// World adapter name, if known.
    fn name(&self) -> Option<&str> {
        None
    }

    /// Writes attribute changes for a run.
    fn experimental_set_attributes(
        &mut self,
        run_id: &str,
        changes: &[AttributeChange],
        options: SetAttributesOptions,
    ) -> Result<(), SetAttributesError>;
}

/// Supported or unsupported world dispatch.
pub enum AttributeWorldDispatch<'a> {
    /// A world adapter that implements attribute writes.
    Supported(&'a mut dyn AttributeWorld),
    /// A world adapter that does not expose the experimental attribute write
    /// hook.
    Unsupported { name: Option<&'a str> },
}

/// Error returned by the internal set-attributes builtin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetAttributesError {
    /// The current step context did not include a workflow run id.
    MissingWorkflowRunId,
    /// The world adapter rejected the attribute write.
    World(String),
}

impl fmt::Display for SetAttributesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingWorkflowRunId => formatter.write_str(
                "__builtin_set_attributes: no workflow run id available in step context",
            ),
            Self::World(message) => formatter.write_str(message),
        }
    }
}

impl Error for SetAttributesError {}

/// Internal response-body builtin descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinStepDescriptor {
    name: &'static str,
    directive: &'static str,
}

impl BuiltinStepDescriptor {
    /// Creates a builtin step descriptor.
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            directive: "use step",
        }
    }

    /// Builtin step function name.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Upstream directive that hoists the builtin through step execution.
    pub const fn directive(&self) -> &'static str {
        self.directive
    }
}

/// Descriptor for upstream `__builtin_response_array_buffer`.
pub fn __builtin_response_array_buffer() -> BuiltinStepDescriptor {
    BuiltinStepDescriptor::new("__builtin_response_array_buffer")
}

/// Descriptor for upstream `__builtin_response_json`.
pub fn __builtin_response_json() -> BuiltinStepDescriptor {
    BuiltinStepDescriptor::new("__builtin_response_json")
}

/// Descriptor for upstream `__builtin_response_text`.
pub fn __builtin_response_text() -> BuiltinStepDescriptor {
    BuiltinStepDescriptor::new("__builtin_response_text")
}

/// Step bridge for workflow-body `setAttributes` calls.
pub fn __builtin_set_attributes<L>(
    changes: &[AttributeChange],
    options: SetAttributesOptions,
    context: &BuiltinStepContext,
    world: AttributeWorldDispatch<'_>,
    state: &mut BuiltinSetAttributesState,
    logger: &mut L,
) -> Result<(), SetAttributesError>
where
    L: SetAttributesLogger,
{
    if changes.is_empty() {
        return Ok(());
    }

    let attempt = context.attempt.unwrap_or(INTERNAL_ATTRIBUTES_MAX_ATTEMPTS);

    let result = match world {
        AttributeWorldDispatch::Supported(world) => {
            let run_id = context
                .workflow_run_id
                .as_deref()
                .ok_or(SetAttributesError::MissingWorkflowRunId)?;
            world.experimental_set_attributes(run_id, changes, options)
        }
        AttributeWorldDispatch::Unsupported { name } => {
            warn_unsupported_world(name, state, logger);
            return Ok(());
        }
    };

    match result {
        Ok(()) => Ok(()),
        Err(error) if attempt < INTERNAL_ATTRIBUTES_MAX_ATTEMPTS => Err(error),
        Err(error) => {
            logger.error(&format!(
                "[workflow] setAttributes: failed to post tags after {INTERNAL_ATTRIBUTES_MAX_ATTEMPTS} attempts; dropping the internal attribute write. {error}"
            ));
            Ok(())
        }
    }
}

/// Upstream `maxRetries` value attached to the builtin function.
pub const fn __builtin_set_attributes_max_retries() -> u32 {
    INTERNAL_ATTRIBUTES_MAX_ATTEMPTS - 1
}

fn warn_unsupported_world<L>(
    name: Option<&str>,
    state: &mut BuiltinSetAttributesState,
    logger: &mut L,
) where
    L: SetAttributesLogger,
{
    if state.unsupported_world_warned {
        return;
    }

    state.unsupported_world_warned = true;
    let world_name = name
        .filter(|name| !name.is_empty())
        .map(|name| format!(" ({name})"))
        .unwrap_or_default();
    logger.warn(&format!(
        "[workflow] setAttributes: the current world implementation{world_name} does not implement experimentalSetAttributes; this call (and any subsequent setAttributes calls in this process) is a no-op. Attributes will become available once the world adapter adds support."
    ));
}
