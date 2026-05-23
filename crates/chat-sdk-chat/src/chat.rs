//! `Chat` — the top-level holder of registered adapters + state backend.
//!
//! 1:1 port (in progress) of `packages/chat/src/chat.ts`.
//!
//! Upstream `class Chat` is the singleton consumers construct at app
//! startup: it holds a map of named adapters, the state backend, and
//! exposes factories (`chat.threadFor(...)`, `chat.channelFor(...)`).
//! The full upstream class is ~2700 LOC; this Rust port lands in
//! stages.
//!
//! **What this slice ships (slice 129):**
//!
//! - [`Chat`] struct holding `HashMap<String, Arc<dyn Adapter>>` +
//!   `Arc<dyn StateAdapter>`. 1:1 with upstream
//!   `class Chat { constructor({ state, adapters }) }`.
//! - [`Chat::register_adapter`] / [`Chat::get_adapter`] —
//!   per-platform adapter registration + lookup.
//! - [`Chat::adapter_names`] — list of registered adapter names.
//! - [`Chat::thread_for`] / [`Chat::channel_for`] — factories for
//!   the corresponding handle types (return `None` when the named
//!   adapter isn't registered).
//! - [`Chat::register_singleton`] / [`Chat::state`] — wires the
//!   `Chat` instance into the global `chat_singleton` slot so
//!   far-flung consumers (e.g. workflow rehydrators) can reach it.
//! - `impl ChatSingleton for Chat` — satisfies the upstream
//!   `ChatSingleton` interface that lives in `chat_singleton.rs`.
//!
//! **What is deferred:**
//!
//! - `chat.on(...)` event registration (needs the per-event handler
//!   model — lands alongside the first Phase-2 adapter).
//! - `chat.transcripts` / `chat.threadHistory` / `chat.callbackUrl`
//!   convenience accessors (each wraps the corresponding class
//!   ported in slices 118-120; the wrapper is a 5-line method).
//! - `chat.openModal`, `chat.parseMessage`, `chat.fetchMessage`, …
//!   — each maps to a not-yet-extended `Adapter` trait method.

use std::collections::HashMap;
use std::sync::Arc;

use crate::channel::Channel;
use crate::chat_singleton::{ChatSingleton, set_chat_singleton};
use crate::thread::Thread;
use crate::types::{Adapter, StateAdapter};

/// Top-level chat handle. 1:1 port (in progress) of upstream
/// `class Chat`.
#[derive(Clone)]
pub struct Chat {
    adapters: Arc<HashMap<String, Arc<dyn Adapter>>>,
    state: Arc<dyn StateAdapter>,
}

impl std::fmt::Debug for Chat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Chat")
            .field("adapters", &self.adapter_names())
            .field("state", &self.state)
            .finish()
    }
}

/// Options for [`Chat::new`]. 1:1 port of upstream
/// `interface ChatOptions { state; adapters? }`.
#[derive(Clone)]
pub struct ChatOptions {
    /// State backend. Required (matches upstream's required `state`).
    pub state: Arc<dyn StateAdapter>,
    /// Initial adapter registrations (name -> adapter). Optional;
    /// adapters can also be added later via
    /// [`Chat::register_adapter`].
    pub adapters: Vec<Arc<dyn Adapter>>,
}

impl std::fmt::Debug for ChatOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatOptions")
            .field("state", &self.state)
            .field(
                "adapters",
                &self
                    .adapters
                    .iter()
                    .map(|a| a.name().to_string())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl Chat {
    /// 1:1 port of upstream `new Chat({ state, adapters? })`.
    /// Adapters are keyed by their `Adapter::name()` return value;
    /// duplicates from the supplied list silently overwrite earlier
    /// entries (last-write-wins), matching upstream's
    /// `adapters.forEach(a => map.set(a.name, a))`.
    pub fn new(options: ChatOptions) -> Self {
        let mut map: HashMap<String, Arc<dyn Adapter>> = HashMap::new();
        for adapter in options.adapters {
            map.insert(adapter.name().to_string(), adapter);
        }
        Self {
            adapters: Arc::new(map),
            state: options.state,
        }
    }

    /// Register a new adapter by name. 1:1 port of upstream
    /// `chat.registerAdapter(adapter)`. Replaces any existing
    /// adapter with the same name.
    ///
    /// **Cost note:** the adapter map is wrapped in an `Arc` so
    /// `Clone` is cheap. `register_adapter` re-allocates the map
    /// (clone-then-mutate); adopters that register many adapters
    /// up front should pass them all to [`Chat::new`] instead of
    /// calling this method repeatedly.
    pub fn register_adapter(&mut self, adapter: Arc<dyn Adapter>) {
        let mut map = (*self.adapters).clone();
        map.insert(adapter.name().to_string(), adapter);
        self.adapters = Arc::new(map);
    }

    /// Look up an adapter by name. 1:1 port of upstream
    /// `chat.getAdapter(name)`. Returns `None` when no matching
    /// adapter is registered.
    pub fn get_adapter(&self, name: &str) -> Option<Arc<dyn Adapter>> {
        self.adapters.get(name).cloned()
    }

    /// List the names of every registered adapter (in arbitrary
    /// order). Convenience over upstream's `Object.keys(adapters)`.
    pub fn adapter_names(&self) -> Vec<String> {
        self.adapters.keys().cloned().collect()
    }

    /// Number of registered adapters.
    pub fn adapter_count(&self) -> usize {
        self.adapters.len()
    }

    /// Borrow the state backend. 1:1 port of upstream
    /// `chat.state` getter.
    pub fn state(&self) -> &Arc<dyn StateAdapter> {
        &self.state
    }

    /// Factory: build a [`Thread`] handle backed by the named
    /// adapter. Returns `None` when no adapter is registered under
    /// `adapter_name`. 1:1 with the inline `chat.getAdapter(name) &&
    /// new Thread({ adapter, threadId })` pattern at upstream
    /// handler callsites.
    pub fn thread_for(&self, adapter_name: &str, thread_id: impl Into<String>) -> Option<Thread> {
        Some(Thread::new(self.get_adapter(adapter_name)?, thread_id))
    }

    /// Factory: build a [`Channel`] handle backed by the named
    /// adapter. Returns `None` when no adapter is registered under
    /// `adapter_name`.
    pub fn channel_for(
        &self,
        adapter_name: &str,
        channel_id: impl Into<String>,
    ) -> Option<Channel> {
        Some(Channel::new(self.get_adapter(adapter_name)?, channel_id))
    }

    /// Register this `Chat` instance as the global singleton. 1:1
    /// port of upstream `chat.registerSingleton()`. After this call
    /// the global `get_chat_singleton()` accessor in
    /// [`crate::chat_singleton`] returns `self`-wrapped in `Arc`.
    pub fn register_singleton(self: &Arc<Self>) {
        set_chat_singleton(self.clone());
    }
}

impl ChatSingleton for Chat {
    fn get_adapter(&self, name: &str) -> Option<Arc<dyn Adapter>> {
        Chat::get_adapter(self, name)
    }

    fn get_state(&self) -> Arc<dyn StateAdapter> {
        self.state.clone()
    }
}

#[cfg(test)]
mod tests {
    //! Additive coverage for the [`Chat`] surface. Upstream's
    //! `chat.test.ts` exercises every cross-adapter handler — those
    //! land as the Adapter trait grows and individual adapter
    //! packages ship.
    use super::*;
    use crate::chat_singleton::{clear_chat_singleton, get_chat_singleton};
    use crate::types::{Adapter, AdapterResult, StateAdapter, StateResult};
    use std::sync::Mutex;

    static SINGLETON_LOCK: Mutex<()> = Mutex::new(());

    /// Bare-minimum adapter, only `name` overridden.
    #[derive(Debug)]
    struct NamedAdapter {
        name: String,
    }

    #[async_trait::async_trait]
    impl Adapter for NamedAdapter {
        fn name(&self) -> &str {
            &self.name
        }
    }

    /// Bare-minimum state backend (every method returns the trait
    /// default; the empty MinimalState pattern from slice 125's
    /// tests).
    #[derive(Debug, Default)]
    struct NullState;

    #[async_trait::async_trait]
    impl StateAdapter for NullState {
        async fn get(&self, _key: &str) -> StateResult<Option<serde_json::Value>> {
            Ok(None)
        }
        async fn set(
            &self,
            _key: &str,
            _value: serde_json::Value,
            _ttl_ms: Option<u64>,
        ) -> StateResult<()> {
            Ok(())
        }
        async fn delete(&self, _key: &str) -> StateResult<()> {
            Ok(())
        }
        async fn append_to_list(
            &self,
            _key: &str,
            _value: serde_json::Value,
            _max_length: Option<usize>,
            _ttl_ms: Option<u64>,
        ) -> StateResult<()> {
            Ok(())
        }
        async fn get_list(
            &self,
            _key: &str,
            _limit: Option<usize>,
        ) -> StateResult<Vec<serde_json::Value>> {
            Ok(Vec::new())
        }
    }

    fn make_chat(adapter_names: &[&str]) -> Chat {
        let state: Arc<dyn StateAdapter> = Arc::new(NullState);
        let adapters: Vec<Arc<dyn Adapter>> = adapter_names
            .iter()
            .map(|n| {
                Arc::new(NamedAdapter {
                    name: (*n).to_string(),
                }) as Arc<dyn Adapter>
            })
            .collect();
        Chat::new(ChatOptions { state, adapters })
    }

    #[test]
    fn chat_new_registers_supplied_adapters_by_name() {
        let chat = make_chat(&["slack", "teams"]);
        assert_eq!(chat.adapter_count(), 2);
        assert!(chat.get_adapter("slack").is_some());
        assert!(chat.get_adapter("teams").is_some());
        assert!(chat.get_adapter("unknown").is_none());
    }

    #[test]
    fn chat_register_adapter_adds_a_new_adapter() {
        let mut chat = make_chat(&["slack"]);
        assert_eq!(chat.adapter_count(), 1);
        chat.register_adapter(Arc::new(NamedAdapter {
            name: "discord".to_string(),
        }));
        assert_eq!(chat.adapter_count(), 2);
        assert!(chat.get_adapter("discord").is_some());
    }

    #[test]
    fn chat_register_adapter_overwrites_an_existing_name() {
        let mut chat = make_chat(&["slack"]);
        let original = chat.get_adapter("slack").unwrap();
        chat.register_adapter(Arc::new(NamedAdapter {
            name: "slack".to_string(),
        }));
        let replacement = chat.get_adapter("slack").unwrap();
        // Different Arcs (i.e. it really was replaced, not the same
        // instance reused).
        assert!(!Arc::ptr_eq(&original, &replacement));
    }

    #[test]
    fn chat_thread_for_returns_a_thread_backed_by_the_named_adapter() {
        let chat = make_chat(&["slack"]);
        let thread = chat.thread_for("slack", "T1").unwrap();
        assert_eq!(thread.thread_id(), "T1");
        assert_eq!(thread.adapter_name(), "slack");
    }

    #[test]
    fn chat_thread_for_returns_none_for_unknown_adapter() {
        let chat = make_chat(&["slack"]);
        assert!(chat.thread_for("teams", "T1").is_none());
    }

    #[test]
    fn chat_channel_for_returns_a_channel_backed_by_the_named_adapter() {
        let chat = make_chat(&["slack"]);
        let channel = chat.channel_for("slack", "C1").unwrap();
        assert_eq!(channel.channel_id(), "C1");
        assert_eq!(channel.adapter_name(), "slack");
    }

    #[test]
    fn chat_channel_for_returns_none_for_unknown_adapter() {
        let chat = make_chat(&["slack"]);
        assert!(chat.channel_for("teams", "C1").is_none());
    }

    #[test]
    fn chat_adapter_names_lists_every_registered_adapter() {
        let chat = make_chat(&["slack", "teams", "discord"]);
        let mut names = chat.adapter_names();
        names.sort();
        assert_eq!(names, vec!["discord", "slack", "teams"]);
    }

    #[test]
    fn chat_implements_chat_singleton_trait_via_get_adapter_and_get_state() {
        // The trait is implemented on Chat directly (not Arc<Chat>).
        // We exercise it through dyn dispatch.
        let chat = make_chat(&["slack"]);
        let singleton: &dyn ChatSingleton = &chat;
        assert!(singleton.get_adapter("slack").is_some());
        // get_state returns the same Arc instance the chat holds.
        let from_trait = singleton.get_state();
        let from_struct = chat.state().clone();
        assert!(Arc::ptr_eq(&from_trait, &from_struct));
    }

    #[test]
    fn chat_register_singleton_publishes_to_the_global_slot() {
        let _guard = SINGLETON_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_chat_singleton();
        let chat = Arc::new(make_chat(&["slack"]));
        chat.register_singleton();
        let fetched = get_chat_singleton();
        // The fetched singleton is the chat we registered.
        assert!(fetched.get_adapter("slack").is_some());
        clear_chat_singleton();
    }

    #[test]
    fn chat_clone_shares_the_adapter_map_arc() {
        let chat = make_chat(&["slack"]);
        let cloned = chat.clone();
        // Both clones see the same Arc<HashMap>.
        assert!(Arc::ptr_eq(&chat.adapters, &cloned.adapters));
    }
}
