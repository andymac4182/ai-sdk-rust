use serde::{Deserialize, Serialize};

/// Cursor pagination options used by world list APIs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginationOptions {
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub sort_order: Option<SortOrder>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
    Asc,
    Desc,
}

/// Standard cursor-paginated response shape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub cursor: Option<String>,
    pub has_more: bool,
}

/// Controls whether serialized payload data is resolved in read responses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResolveData {
    None,
    #[default]
    All,
}

/// Standard error shape propagated from runs and steps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredError {
    pub message: String,
    pub stack: Option<String>,
    pub code: Option<String>,
}

/// A single stream chunk with its zero-based index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamChunk {
    pub index: usize,
    pub data: Vec<u8>,
}

/// Cursor pagination options for chunk retrieval.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetChunksOptions {
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

/// Lightweight metadata about a stream.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamInfoResponse {
    pub tail_index: isize,
    pub done: bool,
}

/// Paginated response for stream chunks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamChunksResponse {
    pub data: Vec<StreamChunk>,
    pub cursor: Option<String>,
    pub has_more: bool,
    pub done: bool,
}
