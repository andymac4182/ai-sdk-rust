use std::error::Error;
use std::fmt;

/// Error type used by world trait contracts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldError {
    message: String,
}

impl WorldError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn unsupported(operation: impl AsRef<str>) -> Self {
        Self::new(format!(
            "world operation {:?} is not supported by this implementation",
            operation.as_ref()
        ))
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for WorldError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for WorldError {}
