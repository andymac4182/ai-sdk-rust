//! Singleton holder for the `Chat` instance.
//!
//! 1:1 port of `packages/chat/src/chat-singleton.ts`. Exists as a separate
//! module to avoid the circular dependency between the future `chat.rs` and
//! `thread.rs` modules, mirroring upstream.
//!
//! The upstream interface holds a `ChatSingleton` object with `getAdapter`
//! and `getState` methods. The Rust port preserves that contract via the
//! [`ChatSingleton`] trait and stores the active instance behind a global
//! `Mutex<Option<Arc<dyn ChatSingleton>>>`.

use std::sync::{Arc, Mutex, OnceLock};

use crate::types::{Adapter, StateAdapter};

/// Holder trait the Chat instance implements. 1:1 port of upstream
/// `interface ChatSingleton`. Mirrors the upstream object surface — adapters
/// are looked up by name; the state backend is exposed without a key.
pub trait ChatSingleton: Send + Sync {
    /// Returns the adapter registered under `name`, or `None` when no
    /// adapter matches. Mirrors upstream
    /// `getAdapter(name: string): Adapter | undefined`.
    fn get_adapter(&self, name: &str) -> Option<Arc<dyn Adapter>>;

    /// Returns the active state backend. Mirrors upstream
    /// `getState(): StateAdapter`. Implementations MUST register a state
    /// backend before any consumer calls `get_state` — there is no
    /// `Option` here on the upstream side either.
    fn get_state(&self) -> Arc<dyn StateAdapter>;
}

fn slot() -> &'static Mutex<Option<Arc<dyn ChatSingleton>>> {
    static SLOT: OnceLock<Mutex<Option<Arc<dyn ChatSingleton>>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Set the active singleton. Mirrors upstream `setChatSingleton(chat)`.
///
/// Marked `internal` in upstream; used by `Chat::register_singleton` in the
/// future `chat.rs` module.
pub fn set_chat_singleton(chat: Arc<dyn ChatSingleton>) {
    *slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(chat);
}

/// Get the active singleton.
///
/// # Panics
///
/// Panics with the upstream error message
/// `"No Chat singleton registered. Call chat.registerSingleton() first."`
/// when no singleton has been set. Mirrors upstream
/// `getChatSingleton(): throws Error`.
pub fn get_chat_singleton() -> Arc<dyn ChatSingleton> {
    // Snapshot under the lock, then drop the lock BEFORE optionally
    // panicking. Otherwise a missing-singleton panic poisons the SLOT
    // mutex and every later call (including `clear_chat_singleton`)
    // panics in turn — fatal for parallel tests.
    let snapshot = {
        let guard = slot()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.clone()
    };
    snapshot.expect("No Chat singleton registered. Call chat.registerSingleton() first.")
}

/// Get the active singleton if one has been registered, returning
/// `None` instead of panicking. Useful at callers that want to
/// degrade gracefully when no singleton is present (e.g. the
/// standalone JSON reviver — slice 443).
pub fn try_get_chat_singleton() -> Option<Arc<dyn ChatSingleton>> {
    slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Whether a singleton has been registered.
/// Mirrors upstream `hasChatSingleton(): boolean`.
pub fn has_chat_singleton() -> bool {
    slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .is_some()
}

/// Clear the active singleton.
///
/// Marked `internal` in upstream; used by tests via the upstream
/// `beforeEach(() => clearChatSingleton())` pattern, mirrored in the
/// `#[cfg(test)] mod tests` block below.
pub fn clear_chat_singleton() {
    *slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

#[cfg(test)]
mod tests {
    //! 1:1 port of `packages/chat/src/chat-singleton.test.ts` from upstream
    //! `vercel/chat` @ `aba6aa94fe5a2ed909ec4daa7db0e21887507fa4`.
    //!
    //! Adaptation: upstream relies on Vitest's sequential test runner for
    //! the shared `_singleton` module variable. Rust runs `#[test]` cases
    //! in parallel by default, so each test acquires a process-wide
    //! [`Mutex`] (`TEST_LOCK`) before touching the singleton, then calls
    //! [`clear_chat_singleton`] to reproduce upstream's
    //! `beforeEach(() => clearChatSingleton())`.

    use super::*;
    use crate::types::{Adapter, StateAdapter};

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Mock singleton mirroring upstream `vi.fn()` placeholders. The four
    /// upstream tests never call the methods, so the Rust mock can panic if
    /// they ever are.
    #[derive(Debug)]
    struct MockSingleton;
    impl ChatSingleton for MockSingleton {
        fn get_adapter(&self, _: &str) -> Option<Arc<dyn Adapter>> {
            None
        }
        fn get_state(&self) -> Arc<dyn StateAdapter> {
            unreachable!("MockSingleton::get_state was not expected to be called in these tests")
        }
    }

    fn setup() -> std::sync::MutexGuard<'static, ()> {
        let guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        clear_chat_singleton();
        guard
    }

    // describe("Chat Singleton")

    #[test]
    fn should_have_no_singleton_by_default() {
        let _guard = setup();
        assert!(!has_chat_singleton());
    }

    #[test]
    fn should_throw_when_getting_unregistered_singleton() {
        let _guard = setup();
        // Rust analogue of upstream `expect(() => …).toThrow(message)`:
        // catch the panic via `catch_unwind` and inspect its message.
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = get_chat_singleton();
        }));
        let payload = panicked.expect_err("expected get_chat_singleton to panic");
        let msg = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        assert!(
            msg.contains("No Chat singleton registered. Call chat.registerSingleton() first."),
            "unexpected panic payload: {msg:?}"
        );
    }

    #[test]
    fn should_set_and_get_a_singleton() {
        let _guard = setup();
        let mock: Arc<dyn ChatSingleton> = Arc::new(MockSingleton);
        let mock_clone = Arc::clone(&mock);
        set_chat_singleton(mock);
        assert!(has_chat_singleton());
        let fetched = get_chat_singleton();
        // Upstream `expect(getChatSingleton()).toBe(mock)` is identity
        // comparison. In Rust, identity of an Arc'd trait object is
        // `Arc::ptr_eq` on the underlying allocation. We hold a clone of
        // the original mock so we can compare after `set_chat_singleton`
        // consumes the first reference.
        assert!(Arc::ptr_eq(&fetched, &mock_clone));
    }

    #[test]
    fn should_clear_the_singleton() {
        let _guard = setup();
        set_chat_singleton(Arc::new(MockSingleton));
        assert!(has_chat_singleton());
        clear_chat_singleton();
        assert!(!has_chat_singleton());
    }

    #[test]
    fn should_allow_overwriting_the_singleton() {
        let _guard = setup();
        let first: Arc<dyn ChatSingleton> = Arc::new(MockSingleton);
        let second: Arc<dyn ChatSingleton> = Arc::new(MockSingleton);
        let second_clone = Arc::clone(&second);
        set_chat_singleton(first);
        set_chat_singleton(second);
        let fetched = get_chat_singleton();
        assert!(Arc::ptr_eq(&fetched, &second_clone));
    }
}
