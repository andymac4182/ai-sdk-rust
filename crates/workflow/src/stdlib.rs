//! Standard-library steps exported by upstream `workflow`.

/// Step-function descriptor for package-owned standard library functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepFunctionDescriptor {
    name: &'static str,
    directive: &'static str,
}

impl StepFunctionDescriptor {
    /// Creates a descriptor for a standard-library step function.
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            directive: "use step",
        }
    }

    /// Exported function name.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Upstream directive that hoists the function through workflow step
    /// execution.
    pub const fn directive(&self) -> &'static str {
        self.directive
    }
}

/// Descriptor for upstream `workflow`'s hoisted `fetch` step.
pub fn fetch() -> StepFunctionDescriptor {
    StepFunctionDescriptor::new("fetch")
}
