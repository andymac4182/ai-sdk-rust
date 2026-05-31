use thiserror::Error;

/// Diagnostic context carried by runtime encryption/decryption failures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDecryptionContext {
    pub operation: &'static str,
    pub byte_length: usize,
    pub format_prefix: Option<String>,
}

impl RuntimeDecryptionContext {
    pub fn new(operation: &'static str, byte_length: usize) -> Self {
        Self {
            operation,
            byte_length,
            format_prefix: None,
        }
    }

    pub fn with_format_prefix(mut self, format_prefix: impl Into<String>) -> Self {
        self.format_prefix = Some(format_prefix.into());
        self
    }
}

/// Rust analogue of upstream `RuntimeDecryptionError`.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("{message}")]
pub struct RuntimeDecryptionError {
    pub message: String,
    pub context: RuntimeDecryptionContext,
}

impl RuntimeDecryptionError {
    pub fn new(message: impl Into<String>, context: RuntimeDecryptionContext) -> Self {
        Self {
            message: message.into(),
            context,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WorkflowCoreError {
    #[error(
        "Data too short to contain format prefix: expected at least {expected} bytes, got {actual}"
    )]
    FormatDataTooShort { expected: usize, actual: usize },

    #[error("Invalid format prefix: \"{prefix}\". Must be 4 characters of [a-z0-9].")]
    InvalidFormatPrefix { prefix: String },

    #[error("Unsupported serialization format: {0}")]
    UnsupportedSerializationFormat(String),

    #[error("Encryption key must be exactly 32 bytes, got {0}")]
    InvalidEncryptionKeyLength(usize),

    #[error(transparent)]
    RuntimeDecryption(#[from] RuntimeDecryptionError),

    #[error("Failed to serialize value: {0}")]
    Serialization(String),

    #[error("Failed to deserialize value: {0}")]
    Deserialization(String),

    #[error("Invalid base64 option: {0}")]
    InvalidBase64Option(String),

    #[error("Invalid base64: {0}")]
    InvalidBase64(String),

    #[error("Invalid hex: {0}")]
    InvalidHex(String),

    #[error("Step functions cannot be deserialized in client context.")]
    StepFunctionInClientContext,

    #[error("Step functions cannot be serialized outside workflow context.")]
    StepFunctionOutsideWorkflowContext,
}

pub type Result<T> = std::result::Result<T, WorkflowCoreError>;
