//! Rust port of the upstream `vercel/chat` `adapter-shared` package — the
//! shared utilities every chat adapter relies on (error types, buffer
//! helpers, card helpers, crypto).
//!
//! Upstream inventory commit:
//! `aba6aa94fe5a2ed909ec4daa7db0e21887507fa4` (2026-05-22). See
//! [`docs/chat/upstream-parity.md`](../../docs/chat/upstream-parity.md) for
//! the per-module parity status. The shared token in this crate's name
//! mirrors the upstream package name and is documented as a
//! naming-conventions exception in
//! [`scripts/check-naming-conventions.sh`](../../scripts/check-naming-conventions.sh).

pub mod errors;
