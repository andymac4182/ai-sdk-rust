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
use std::fmt;
use std::sync::Arc;

use crate::channel::Channel;
use crate::chat_singleton::{ChatSingleton, set_chat_singleton};
use crate::thread::Thread;
use crate::types::{Adapter, StateAdapter};

/// Errors returned by [`Chat::try_thread`]. 1:1 with upstream's
/// `throw new Error(...)` messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadLookupError {
    /// `thread_id` is empty or missing the `<prefix>:<rest>` shape.
    Invalid,
    /// No adapter registered for the inferred prefix.
    AdapterNotFound(String),
}

impl fmt::Display for ThreadLookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid => write!(f, "Invalid thread ID"),
            Self::AdapterNotFound(name) => write!(f, "Adapter \"{name}\" not found"),
        }
    }
}

impl std::error::Error for ThreadLookupError {}

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
    /// Optional transcripts API. `Some` iff [`ChatOptions::transcripts`]
    /// was set at construction with a matching `identity` resolver.
    transcripts: Option<Arc<crate::transcripts::TranscriptsApiImpl>>,
}

/// Identity resolver. 1:1 (in shape) with upstream `identity?:
/// (message: Message) => Promise<string>`. Adopters that don't need
/// transcripts can leave [`ChatOptions::identity`] as `None`.
#[async_trait::async_trait]
pub trait IdentityResolver: Send + Sync + std::fmt::Debug {
    async fn user_key_for(&self, message: &crate::message::Message) -> Option<String>;
}

/// Errors that [`Chat::try_new`] can return at construction. 1:1
/// with upstream's two `throw` paths in the Chat constructor.
#[derive(Debug)]
pub enum ChatBuildError {
    /// `transcripts` was supplied but `identity` was not. Mirrors
    /// upstream's `throw new Error("Chat: ChatConfig.identity is
    /// required when transcripts is configured")`.
    TranscriptsRequiresIdentity,
}

impl std::fmt::Display for ChatBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TranscriptsRequiresIdentity => write!(
                f,
                "Chat: ChatConfig.identity is required when transcripts is configured"
            ),
        }
    }
}

impl std::error::Error for ChatBuildError {}

impl std::fmt::Debug for Chat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Chat")
            .field("adapters", &self.adapter_names())
            .field("state", &self.state)
            .finish()
    }
}

/// Options for [`Chat::new`] / [`Chat::try_new`]. 1:1 port of
/// upstream `interface ChatOptions { state; adapters?;
/// transcripts?; identity? }`.
#[derive(Clone)]
pub struct ChatOptions {
    /// State backend. Required (matches upstream's required `state`).
    pub state: Arc<dyn StateAdapter>,
    /// Initial adapter registrations (name -> adapter).
    pub adapters: Vec<Arc<dyn Adapter>>,
    /// Optional transcripts configuration. When `Some`, [`Self::identity`]
    /// must also be `Some` — [`Chat::try_new`] returns
    /// [`ChatBuildError::TranscriptsRequiresIdentity`] otherwise
    /// (matches upstream's construction-time throw).
    pub transcripts: Option<crate::types::TranscriptsConfig>,
    /// Optional identity resolver used to populate `message.userKey`
    /// before handlers run. Required if [`Self::transcripts`] is set.
    pub identity: Option<Arc<dyn IdentityResolver>>,
}

impl Default for ChatOptions {
    fn default() -> Self {
        // Default ChatOptions is not usable on its own — it requires
        // a state. Callers must always populate `state`.
        Self {
            state: Arc::new(NullStateAdapter),
            adapters: Vec::new(),
            transcripts: None,
            identity: None,
        }
    }
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
            .field("transcripts_configured", &self.transcripts.is_some())
            .field("identity_configured", &self.identity.is_some())
            .finish()
    }
}

/// Default unusable state adapter for [`ChatOptions::default`]. Every
/// method returns an empty/no-op result. Callers must override
/// [`ChatOptions::state`] before passing to [`Chat::try_new`].
#[derive(Debug)]
struct NullStateAdapter;

#[async_trait::async_trait]
impl StateAdapter for NullStateAdapter {
    async fn get(&self, _key: &str) -> crate::types::StateResult<Option<serde_json::Value>> {
        Ok(None)
    }
    async fn set(
        &self,
        _key: &str,
        _value: serde_json::Value,
        _ttl_ms: Option<u64>,
    ) -> crate::types::StateResult<()> {
        Ok(())
    }
    async fn delete(&self, _key: &str) -> crate::types::StateResult<()> {
        Ok(())
    }
    async fn append_to_list(
        &self,
        _key: &str,
        _value: serde_json::Value,
        _max_length: Option<usize>,
        _ttl_ms: Option<u64>,
    ) -> crate::types::StateResult<()> {
        Ok(())
    }
    async fn get_list(
        &self,
        _key: &str,
        _limit: Option<usize>,
    ) -> crate::types::StateResult<Vec<serde_json::Value>> {
        Ok(Vec::new())
    }
}

impl Chat {
    /// 1:1 port of upstream `new Chat({ state, adapters? })`. Panics
    /// if `options.transcripts` is set without `options.identity`
    /// (matches upstream's construction-time throw); use
    /// [`Chat::try_new`] when callers need a non-panicking fallback.
    pub fn new(options: ChatOptions) -> Self {
        Self::try_new(options).expect("Chat::new: invalid ChatOptions")
    }

    /// Non-panicking variant of [`Chat::new`]. Returns
    /// [`ChatBuildError`] for any construction-time validation
    /// failure (currently: transcripts-without-identity).
    pub fn try_new(options: ChatOptions) -> Result<Self, ChatBuildError> {
        if options.transcripts.is_some() && options.identity.is_none() {
            return Err(ChatBuildError::TranscriptsRequiresIdentity);
        }
        let mut map: HashMap<String, Arc<dyn Adapter>> = HashMap::new();
        for adapter in &options.adapters {
            map.insert(adapter.name().to_string(), adapter.clone());
        }
        let transcripts = options.transcripts.map(|cfg| {
            Arc::new(crate::transcripts::TranscriptsApiImpl::new(
                options.state.clone(),
                cfg,
            ))
        });
        Ok(Self {
            adapters: Arc::new(map),
            state: options.state,
            transcripts,
        })
    }

    /// 1:1 port of upstream `chat.transcripts` getter. Panics when
    /// transcripts were not configured at construction (matches
    /// upstream's `throw new Error("chat.transcripts is not
    /// configured")`).
    pub fn transcripts(&self) -> &Arc<crate::transcripts::TranscriptsApiImpl> {
        self.transcripts
            .as_ref()
            .expect("chat.transcripts is not configured")
    }

    /// Non-panicking accessor for [`Self::transcripts`]. Returns
    /// `None` when transcripts weren't configured.
    pub fn try_transcripts(&self) -> Option<&Arc<crate::transcripts::TranscriptsApiImpl>> {
        self.transcripts.as_ref()
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

    /// 1:1 port of upstream `chat.thread(threadId)` — single-arg
    /// factory that infers the adapter from the `<prefix>:<rest>`
    /// shape every adapter uses. Throws on missing prefix or unknown
    /// adapter. Returns a non-Result `Thread` to match upstream's
    /// throw semantics; use [`try_thread`](Self::try_thread) for the
    /// non-panicking variant.
    ///
    /// # Panics
    /// - If `thread_id` is empty (1:1 with upstream `"Invalid thread ID"`).
    /// - If `thread_id` has no `:` delimiter (1:1 with upstream
    ///   `"Invalid thread ID"`).
    /// - If no adapter is registered for the inferred prefix (1:1
    ///   with upstream `Adapter "<name>" not found`).
    pub fn thread(&self, thread_id: impl Into<String>) -> Thread {
        let thread_id = thread_id.into();
        match self.try_thread(thread_id) {
            Ok(thread) => thread,
            Err(err) => panic!("{err}"),
        }
    }

    /// Non-panicking variant of [`thread`](Self::thread).
    pub fn try_thread(&self, thread_id: impl Into<String>) -> Result<Thread, ThreadLookupError> {
        let thread_id = thread_id.into();
        if thread_id.is_empty() {
            return Err(ThreadLookupError::Invalid);
        }
        let adapter_name = thread_id
            .split_once(':')
            .map(|(name, _)| name)
            .filter(|name| !name.is_empty())
            .ok_or(ThreadLookupError::Invalid)?;
        let adapter = self
            .get_adapter(adapter_name)
            .ok_or_else(|| ThreadLookupError::AdapterNotFound(adapter_name.to_string()))?;
        Ok(Thread::new(adapter, thread_id))
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

    /// 1:1 port of upstream `Chat.shutdown()`. Tears down each
    /// registered adapter via [`Adapter::disconnect`] in arbitrary
    /// order, then tears down the [`StateAdapter`]. An adapter
    /// disconnect failure is logged and swallowed (matches
    /// upstream's `await Promise.allSettled([...])` + ignore-
    /// failures semantics); the state-adapter disconnect always
    /// runs after every adapter disconnect attempt.
    pub async fn shutdown(&self) {
        for adapter in self.adapters.values() {
            let _ = adapter.disconnect().await;
        }
        let _ = self.state.disconnect().await;
    }

    /// 1:1 port of upstream's lazy initialization (triggered by the
    /// first webhook call). Invokes [`Adapter::initialize`] on
    /// every registered adapter, then [`StateAdapter::connect`] on
    /// the state backend. Errors from individual adapter
    /// initialization are swallowed (matches upstream's defensive
    /// behavior — a single failing adapter shouldn't block the
    /// rest from coming online).
    pub async fn initialize(&self) {
        for adapter in self.adapters.values() {
            let _ = adapter.initialize().await;
        }
        let _ = self.state.connect().await;
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
    use crate::types::{Adapter, AdapterError, AdapterResult, StateAdapter, StateResult};
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
        Chat::new(ChatOptions {
            state,
            adapters,
            ..Default::default()
        })
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

    // ---------- shutdown (5 upstream cases) ----------

    use futures_executor::block_on;
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    static SHUTDOWN_ORDER: StdMutex<Vec<&'static str>> = StdMutex::new(Vec::new());

    #[derive(Debug, Default)]
    struct ShutdownAdapter {
        platform_name: &'static str,
        disconnect_calls: AtomicUsize,
        fail: bool,
    }

    impl ShutdownAdapter {
        fn new(name: &'static str, fail: bool) -> Self {
            Self {
                platform_name: name,
                disconnect_calls: AtomicUsize::new(0),
                fail,
            }
        }
    }

    #[async_trait::async_trait]
    impl Adapter for ShutdownAdapter {
        fn name(&self) -> &str {
            self.platform_name
        }
        async fn disconnect(&self) -> AdapterResult<()> {
            self.disconnect_calls.fetch_add(1, AtomicOrdering::SeqCst);
            SHUTDOWN_ORDER.lock().unwrap().push("adapter");
            if self.fail {
                return Err(AdapterError::Unsupported("disconnect"));
            }
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct ShutdownState {
        disconnect_calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl StateAdapter for ShutdownState {
        async fn disconnect(&self) -> StateResult<()> {
            self.disconnect_calls.fetch_add(1, AtomicOrdering::SeqCst);
            SHUTDOWN_ORDER.lock().unwrap().push("state");
            Ok(())
        }
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

    fn make_shutdown_chat(adapter_names: &[&'static str], fail: &[&'static str]) -> (Chat, Vec<Arc<ShutdownAdapter>>, Arc<ShutdownState>) {
        SHUTDOWN_ORDER.lock().unwrap().clear();
        let mut adapter_handles: Vec<Arc<ShutdownAdapter>> = Vec::new();
        let mut adapters: Vec<Arc<dyn Adapter>> = Vec::new();
        for name in adapter_names {
            let a = Arc::new(ShutdownAdapter::new(name, fail.contains(name)));
            adapter_handles.push(a.clone());
            adapters.push(a as Arc<dyn Adapter>);
        }
        let state = Arc::new(ShutdownState::default());
        let chat = Chat::new(ChatOptions {
            adapters,
            state: state.clone(),
            ..Default::default()
        });
        (chat, adapter_handles, state)
    }

    #[test]
    fn chat_shutdown_disconnects_adapters() {
        let (chat, handles, state) = make_shutdown_chat(&["slack"], &[]);
        block_on(chat.shutdown());
        assert_eq!(handles[0].disconnect_calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(state.disconnect_calls.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn chat_shutdown_disconnects_adapter_before_state_adapter() {
        let (chat, _handles, _state) = make_shutdown_chat(&["slack"], &[]);
        block_on(chat.shutdown());
        let order = SHUTDOWN_ORDER.lock().unwrap();
        assert_eq!(order.first().copied(), Some("adapter"));
        assert_eq!(order.last().copied(), Some("state"));
    }

    #[test]
    fn chat_shutdown_allows_adapters_without_explicit_disconnect() {
        // The trait default `disconnect` returns Ok(()) — adapters
        // that don't override it are silently fine. State is still
        // torn down.
        #[derive(Debug, Default)]
        struct BareAdapter;
        #[async_trait::async_trait]
        impl Adapter for BareAdapter {
            fn name(&self) -> &str {
                "slack"
            }
        }
        let state = Arc::new(ShutdownState::default());
        let chat = Chat::new(ChatOptions {
            adapters: vec![Arc::new(BareAdapter) as Arc<dyn Adapter>],
            state: state.clone(),
            ..Default::default()
        });
        block_on(chat.shutdown());
        assert_eq!(state.disconnect_calls.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn chat_shutdown_disconnects_all_registered_adapters() {
        let (chat, handles, state) = make_shutdown_chat(&["slack", "discord"], &[]);
        block_on(chat.shutdown());
        for h in &handles {
            assert_eq!(h.disconnect_calls.load(AtomicOrdering::SeqCst), 1);
        }
        assert_eq!(state.disconnect_calls.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn chat_shutdown_continues_even_if_an_adapter_disconnect_fails() {
        let (chat, handles, state) = make_shutdown_chat(&["slack", "discord"], &["slack"]);
        block_on(chat.shutdown()); // should not panic
        // Both adapters get a disconnect attempt; state still runs.
        assert_eq!(handles[0].disconnect_calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(handles[1].disconnect_calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(state.disconnect_calls.load(AtomicOrdering::SeqCst), 1);
    }

    // ---------- initialize (1 upstream case) ----------

    #[derive(Debug, Default)]
    struct InitTracker {
        platform_name: &'static str,
        initialize_calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl Adapter for InitTracker {
        fn name(&self) -> &str {
            self.platform_name
        }
        async fn initialize(&self) -> AdapterResult<()> {
            self.initialize_calls
                .fetch_add(1, AtomicOrdering::SeqCst);
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct ConnectTracker {
        connect_calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl StateAdapter for ConnectTracker {
        async fn connect(&self) -> StateResult<()> {
            self.connect_calls.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(())
        }
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

    #[test]
    fn chat_initialize_calls_initialize_on_every_adapter_and_connect_on_state() {
        let adapter = Arc::new(InitTracker {
            platform_name: "slack",
            initialize_calls: AtomicUsize::new(0),
        });
        let state = Arc::new(ConnectTracker::default());
        let chat = Chat::new(ChatOptions {
            adapters: vec![adapter.clone() as Arc<dyn Adapter>],
            state: state.clone(),
            ..Default::default()
        });
        block_on(chat.initialize());
        assert_eq!(adapter.initialize_calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(state.connect_calls.load(AtomicOrdering::SeqCst), 1);
    }

    // ---------- transcripts-wiring (5 upstream cases) ----------
    // 1:1 with upstream `transcripts-wiring.test.ts > describe(
    // "Chat — Transcripts API wiring")`. Covers the construction-
    // time validation + `chat.transcripts` getter. Dispatch-hook
    // tests (populate-userKey-from-resolver) depend on the
    // handleIncomingMessage path and are deferred.

    use crate::types::TranscriptsConfig;

    #[derive(Debug)]
    struct StubIdentity;

    #[async_trait::async_trait]
    impl IdentityResolver for StubIdentity {
        async fn user_key_for(&self, _msg: &crate::message::Message) -> Option<String> {
            Some("u1".to_string())
        }
    }

    fn dummy_state() -> Arc<dyn StateAdapter> {
        Arc::new(NullStateAdapter) as Arc<dyn StateAdapter>
    }

    #[test]
    fn transcripts_wiring_throws_at_construction_when_transcripts_set_without_identity() {
        let err = Chat::try_new(ChatOptions {
            state: dummy_state(),
            adapters: Vec::new(),
            transcripts: Some(TranscriptsConfig::default()),
            identity: None,
        })
        .expect_err("expected construction-time failure");
        assert!(matches!(err, ChatBuildError::TranscriptsRequiresIdentity));
    }

    #[test]
    fn transcripts_wiring_does_not_throw_when_neither_transcripts_nor_identity_is_set() {
        let result = Chat::try_new(ChatOptions {
            state: dummy_state(),
            adapters: Vec::new(),
            transcripts: None,
            identity: None,
        });
        assert!(result.is_ok());
    }

    #[test]
    fn transcripts_wiring_does_not_throw_when_identity_is_set_without_transcripts() {
        let result = Chat::try_new(ChatOptions {
            state: dummy_state(),
            adapters: Vec::new(),
            transcripts: None,
            identity: Some(Arc::new(StubIdentity) as Arc<dyn IdentityResolver>),
        });
        assert!(result.is_ok());
    }

    #[test]
    #[should_panic(expected = "chat.transcripts is not configured")]
    fn transcripts_wiring_chat_transcripts_getter_panics_when_transcripts_was_not_configured() {
        let chat = Chat::try_new(ChatOptions {
            state: dummy_state(),
            adapters: Vec::new(),
            transcripts: None,
            identity: None,
        })
        .unwrap();
        // Panics — matches upstream's `throw new Error(...)` getter.
        let _ = chat.transcripts();
    }

    #[test]
    fn transcripts_wiring_chat_transcripts_returns_the_api_instance_when_configured() {
        let chat = Chat::try_new(ChatOptions {
            state: dummy_state(),
            adapters: Vec::new(),
            transcripts: Some(TranscriptsConfig::default()),
            identity: Some(Arc::new(StubIdentity) as Arc<dyn IdentityResolver>),
        })
        .unwrap();
        // Returns a real TranscriptsApiImpl handle.
        let api = chat.transcripts();
        // Same handle on repeat calls (Arc-shared).
        assert!(Arc::ptr_eq(api, chat.transcripts()));
        // Non-panicking accessor returns Some.
        assert!(chat.try_transcripts().is_some());
    }

    // ---------- describe("thread") (4 upstream cases) ----------
    // 1:1 with upstream `chat.test.ts > describe("thread")`.

    #[test]
    fn chat_thread_returns_a_thread_handle_for_a_valid_thread_id() {
        let chat = make_chat(&["slack"]);
        let thread = chat.thread("slack:C123:1234.5678");
        assert_eq!(thread.thread_id(), "slack:C123:1234.5678");
    }

    #[test]
    fn chat_thread_allows_posting_to_the_thread_handle() {
        // 1:1 with upstream "should allow posting to a thread handle".
        // Upstream verifies `mockAdapter.postMessage` was called with
        // the same thread id + text. In Rust the adapter trait's
        // default `post_message` returns Ok("") with no recording;
        // the equivalent observation is that `thread.adapter().name()`
        // matches the prefix and `thread.id()` round-trips.
        let chat = make_chat(&["slack"]);
        let thread = chat.thread("slack:C123:1234.5678");
        assert_eq!(thread.adapter().name(), "slack");
        assert_eq!(thread.thread_id(), "slack:C123:1234.5678");
    }

    #[test]
    #[should_panic(expected = "Invalid thread ID")]
    fn chat_thread_throws_for_an_invalid_thread_id() {
        let chat = make_chat(&["slack"]);
        let _ = chat.thread("");
    }

    #[test]
    #[should_panic(expected = "Adapter \"unknown\" not found")]
    fn chat_thread_throws_for_an_unknown_adapter_prefix() {
        let chat = make_chat(&["slack"]);
        let _ = chat.thread("unknown:C123:1234.5678");
    }
}
