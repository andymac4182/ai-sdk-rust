//! Persistence contracts for the Rust Open Agents remote-agent service.
//!
//! This crate models the durable records rechecked from Open Agents:
//! sessions, chats, JSON chat messages, active workflow ownership, workflow
//! runs and steps, usage rows, Slack thread mappings, sandbox lifecycle data,
//! and retry idempotency keys. It intentionally ships an in-memory
//! implementation first; the existing workspace Postgres state crate is still
//! a connection skeleton, so a production SQL backend should land with the
//! repo's eventual DB client and migration pattern.

pub mod memory;
pub mod types;

pub use memory::MemoryPersistenceStore;
pub use types::*;

#[cfg(test)]
mod contract_tests;
