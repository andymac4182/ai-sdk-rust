//! OpenAI-compatible provider helpers for the Rust port of upstream
//! `@ai-sdk/openai-compatible`.

#![forbid(unsafe_code)]

/// The OpenAI-compatible crate version compiled into the library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod baseten;
pub mod openai_compatible;

pub use baseten::{BasetenModelConfig, BasetenProvider, BasetenProviderSettings, create_baseten};
pub use openai_compatible::{
    OpenAICompatibleChatLanguageModel, OpenAICompatibleCompletionLanguageModel,
    OpenAICompatibleEmbeddingModel, OpenAICompatibleErrorToMessage,
    OpenAICompatibleExtractMetadataArgs, OpenAICompatibleExtractMetadataFuture,
    OpenAICompatibleImageModel, OpenAICompatibleMetadataExtractor, OpenAICompatibleModelEntry,
    OpenAICompatibleModelListResponse, OpenAICompatibleProvider, OpenAICompatibleProviderSettings,
    OpenAICompatibleRequestBodyTransformer, OpenAICompatibleStreamMetadataExtractor,
    OpenAICompatibleTransport, OpenAICompatibleTransportFuture, create_openai_compatible,
};
