//! Rust port of the upstream `vercel/chat` `chat` package — the unified chat
//! abstraction that adapters (`slack`, `teams`, `discord`, …) plug into.
//!
//! Upstream inventory commit:
//! `aba6aa94fe5a2ed909ec4daa7db0e21887507fa4` (2026-05-22). See
//! [`docs/chat/upstream-parity.md`](../../docs/chat/upstream-parity.md) for the
//! per-module parity status. JSX/React authoring surfaces from upstream
//! (`jsx-runtime`, `jsx-react.test.tsx`, etc.) are intentionally
//! `js-only-documented` and have no Rust counterpart; the card/modal *data
//! shapes* they produce are portable and will land in the `cards` and
//! `modals` modules in later slices.

pub mod callback_url;
pub mod cards;
pub mod chat_singleton;
pub mod emoji;
pub mod errors;
pub mod from_full_stream;
pub mod logger;
pub mod markdown;
pub mod message;
pub mod modals;
pub mod plan;
pub mod postable_object;
pub mod reviver;
pub mod thread_history;
pub mod transcripts;
pub mod types;
