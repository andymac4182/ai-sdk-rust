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

/// Default TTL the chat singleton uses for the per-thread lock the
/// concurrency layer acquires before invoking each handler. 1:1
/// with upstream's private `DEFAULT_LOCK_TTL_MS = 30_000` (30 s).
pub const DEFAULT_LOCK_TTL_MS: u64 = 30_000;

/// Default TTL for message-dedupe entries. Used to suppress
/// duplicate inbound webhooks that the platform retries within
/// this window. 1:1 with upstream's private
/// `DEDUPE_TTL_MS = 5 * 60 * 1000` (5 min).
pub const DEDUPE_TTL_MS: u64 = 5 * 60 * 1000;

/// Default TTL for modal-context state stored alongside open
/// modals. 1:1 with upstream's private
/// `MODAL_CONTEXT_TTL_MS = 24 * 60 * 60 * 1000` (24 h).
pub const MODAL_CONTEXT_TTL_MS: u64 = 24 * 60 * 60 * 1000;

/// Whether `user_id` matches the Slack member-id pattern. 1:1 with
/// upstream's private `SLACK_USER_ID_REGEX = /^[UW][A-Z0-9]+$/`.
/// Slack member ids start with `U` (user) or `W` (workspace owner)
/// followed by uppercase alphanumerics. Used by the chat singleton's
/// `adapterFor(userId)` router to dispatch to the Slack adapter.
pub fn is_slack_user_id(user_id: &str) -> bool {
    let mut chars = user_id.chars();
    let Some(first) = chars.next() else { return false };
    if first != 'U' && first != 'W' {
        return false;
    }
    let rest = chars.as_str();
    !rest.is_empty()
        && rest
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// Whether `user_id` matches the Discord snowflake pattern. 1:1
/// with upstream's private
/// `DISCORD_SNOWFLAKE_REGEX = /^\d{17,19}$/`.
pub fn is_discord_snowflake(user_id: &str) -> bool {
    let len = user_id.len();
    (17..=19).contains(&len) && user_id.chars().all(|c| c.is_ascii_digit())
}

/// Whether `user_id` matches the Linear user-uuid v4 pattern. 1:1
/// with upstream's private
/// `LINEAR_UUID_REGEX = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i`.
/// Case-insensitive hex with the canonical 8-4-4-4-12 dash pattern.
pub fn is_linear_uuid(user_id: &str) -> bool {
    let bytes = user_id.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (i, &b) in bytes.iter().enumerate() {
        let is_dash_pos = matches!(i, 8 | 13 | 18 | 23);
        if is_dash_pos {
            if b != b'-' {
                return false;
            }
        } else if !b.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

/// Whether `user_id` is a non-empty digit-only string. 1:1 with
/// upstream's private `NUMERIC_REGEX = /^\d+$/`. Used as the first
/// pass before dispatching numeric ids to Discord / Telegram /
/// GitHub.
pub fn is_numeric_user_id(user_id: &str) -> bool {
    !user_id.is_empty() && user_id.chars().all(|c| c.is_ascii_digit())
}

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

    /// Get the registered singleton `Chat` instance. 1:1 port of
    /// upstream `Chat.getSingleton()` static method. Returns the
    /// `Arc<dyn ChatSingleton>` from the global slot. Panics
    /// (via the underlying `get_chat_singleton()`) if no singleton
    /// has been registered yet.
    pub fn get_singleton() -> std::sync::Arc<dyn ChatSingleton> {
        crate::chat_singleton::get_chat_singleton()
    }

    /// Whether a singleton has been registered. 1:1 port of
    /// upstream `Chat.hasSingleton()` static method.
    pub fn has_singleton() -> bool {
        crate::chat_singleton::has_chat_singleton()
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
    // ---------- user-id pattern predicates ----------
    // 1:1 with upstream's private regexes used by `adapterFor(userId)`
    // routing. No standalone upstream tests; the predicates are
    // exercised through the router which needs ChatImpl + multiple
    // adapter registrations to wire up. Test the predicates directly
    // so future router slices can rely on them.

    #[test]
    fn is_slack_user_id_accepts_u_and_w_prefixed_uppercase_alphanum() {
        assert!(is_slack_user_id("U0123ABC"));
        assert!(is_slack_user_id("WABCDEF1"));
        // No `U` or `W` prefix.
        assert!(!is_slack_user_id("X0123ABC"));
        // Empty after prefix.
        assert!(!is_slack_user_id("U"));
        // Lowercase rejected (matches upstream `[A-Z0-9]+`).
        assert!(!is_slack_user_id("Uabcdef1"));
        // Dashes / underscores rejected.
        assert!(!is_slack_user_id("U_ABC"));
        // Empty string.
        assert!(!is_slack_user_id(""));
    }

    #[test]
    fn is_discord_snowflake_accepts_17_to_19_digit_strings() {
        assert!(is_discord_snowflake(&"1".repeat(17)));
        assert!(is_discord_snowflake(&"1".repeat(18)));
        assert!(is_discord_snowflake(&"1".repeat(19)));
        // Out of range.
        assert!(!is_discord_snowflake(&"1".repeat(16)));
        assert!(!is_discord_snowflake(&"1".repeat(20)));
        // Non-digit char.
        assert!(!is_discord_snowflake("12345678901234567a"));
        // Empty.
        assert!(!is_discord_snowflake(""));
    }

    #[test]
    fn is_linear_uuid_accepts_canonical_v4_layout_case_insensitively() {
        // Canonical v4 layout, lowercase.
        assert!(is_linear_uuid("8f1f3c7e-d4e1-4f9a-bf2b-1c3d4e5f6a7b"));
        // Uppercase hex.
        assert!(is_linear_uuid("8F1F3C7E-D4E1-4F9A-BF2B-1C3D4E5F6A7B"));
        // Wrong length.
        assert!(!is_linear_uuid("8f1f3c7e-d4e1-4f9a-bf2b-1c3d4e5f6a7"));
        assert!(!is_linear_uuid("8f1f3c7e-d4e1-4f9a-bf2b-1c3d4e5f6a7b0"));
        // Dash in wrong position.
        assert!(!is_linear_uuid("8f1f3c7ed-4e1-4f9a-bf2b-1c3d4e5f6a7b"));
        // Non-hex char.
        assert!(!is_linear_uuid("zzzzzzzz-d4e1-4f9a-bf2b-1c3d4e5f6a7b"));
        // Empty.
        assert!(!is_linear_uuid(""));
    }

    #[test]
    fn is_numeric_user_id_accepts_non_empty_digit_strings() {
        assert!(is_numeric_user_id("1"));
        assert!(is_numeric_user_id("123456789"));
        assert!(!is_numeric_user_id(""));
        assert!(!is_numeric_user_id("12a3"));
        assert!(!is_numeric_user_id("-1"));
    }

    #[test]
    fn chat_ttl_constants_match_upstream() {
        // 1:1 with upstream's private `DEFAULT_LOCK_TTL_MS = 30_000`,
        // `DEDUPE_TTL_MS = 5 * 60 * 1000`, `MODAL_CONTEXT_TTL_MS =
        // 24 * 60 * 60 * 1000`. The wall-clock seconds these encode
        // matter for adopter HTTP-handler tuning.
        assert_eq!(DEFAULT_LOCK_TTL_MS, 30_000);
        assert_eq!(DEDUPE_TTL_MS, 5 * 60 * 1000);
        assert_eq!(MODAL_CONTEXT_TTL_MS, 24 * 60 * 60 * 1000);
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

    // ---------- Chat::has_singleton + Chat::get_singleton ----------
    // 1:1 port of upstream `Chat.hasSingleton()` + `Chat.getSingleton()`
    // static class methods.

    #[test]
    fn chat_has_singleton_reflects_the_global_slot() {
        let _guard = SINGLETON_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_chat_singleton();
        assert!(!Chat::has_singleton());
        let chat = Arc::new(make_chat(&["slack"]));
        chat.register_singleton();
        assert!(Chat::has_singleton());
        clear_chat_singleton();
        assert!(!Chat::has_singleton());
    }

    #[test]
    fn chat_get_singleton_returns_the_registered_instance() {
        let _guard = SINGLETON_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_chat_singleton();
        let chat = Arc::new(make_chat(&["slack"]));
        chat.register_singleton();
        let fetched = Chat::get_singleton();
        assert!(fetched.get_adapter("slack").is_some());
        clear_chat_singleton();
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
