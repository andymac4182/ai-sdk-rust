use workflow_errors::WorkflowError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnknownField {
    String(String),
    Number(i64),
    Boolean(bool),
    Null,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UnknownErrorShape {
    pub name: Option<UnknownField>,
    pub message: Option<UnknownField>,
    pub stack: Option<UnknownField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ErrorLike {
    pub name: String,
    pub message: String,
    pub stack: Option<String>,
    pub fatal: bool,
}

impl ErrorLike {
    #[must_use]
    pub fn new(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            message: message.into(),
            stack: None,
            fatal: false,
        }
    }

    #[must_use]
    pub fn with_stack(mut self, stack: impl Into<String>) -> Self {
        self.stack = Some(stack.into());
        self
    }

    #[must_use]
    pub fn with_fatal(mut self, fatal: bool) -> Self {
        self.fatal = fatal;
        self
    }
}

impl From<WorkflowError> for ErrorLike {
    fn from(value: WorkflowError) -> Self {
        Self {
            name: value.name().to_string(),
            message: value.message().to_string(),
            stack: value.stack().map(ToOwned::to_owned),
            fatal: value.is_fatal(),
        }
    }
}

#[must_use]
pub fn get_error_name(value: &ErrorLike) -> &str {
    &value.name
}

#[must_use]
pub fn get_error_stack(value: &ErrorLike) -> &str {
    value.stack.as_deref().unwrap_or("")
}

#[must_use]
pub fn is_abort_error(value: &ErrorLike) -> bool {
    value.name == "AbortError" && !value.message.is_empty()
}

#[must_use]
pub fn is_abort_error_shape(value: &UnknownErrorShape) -> bool {
    matches!(value.name, Some(UnknownField::String(ref name)) if name == "AbortError")
        && matches!(value.message, Some(UnknownField::String(_)))
        && matches!(value.stack, None | Some(UnknownField::String(_)))
}

#[must_use]
pub fn promote_abort_error_to_fatal(value: ErrorLike) -> ErrorLike {
    if !is_abort_error(&value) || value.fatal {
        return value;
    }

    ErrorLike {
        name: "FatalError".to_string(),
        message: format!("Aborted: {}", value.message),
        stack: value.stack,
        fatal: true,
    }
}
