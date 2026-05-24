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

/// Errors returned by [`Chat::open_dm`]. 1:1 with upstream's
/// `throw new ChatError(...)` in `chat.openDM`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenDmError {
    /// No adapter pattern matched the user id (Slack `U.../W...`,
    /// GChat `users/...`, Teams `29:...`, Linear UUID, or numeric).
    UnknownUserIdFormat(String),
    /// The inferred adapter rejected `open_dm` (most often
    /// `AdapterError::Unsupported("open_dm")`).
    AdapterError(String),
}

impl fmt::Display for OpenDmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownUserIdFormat(id) => {
                write!(f, "Cannot infer adapter from userId \"{id}\"")
            }
            Self::AdapterError(msg) => write!(f, "open_dm failed: {msg}"),
        }
    }
}

impl std::error::Error for OpenDmError {}

/// Errors returned by [`Chat::get_user`]. 1:1 with upstream's
/// `throw new ChatError(...)` cases in `chat.getUser`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GetUserError {
    /// No adapter pattern matched the user id.
    UnknownUserIdFormat(String),
    /// Numeric user id matched multiple registered adapters
    /// (Discord/Telegram/GitHub). 1:1 with upstream's
    /// `throw new ChatError(..., "AMBIGUOUS_USER_ID")` when
    /// `candidates.length > 1`. Carries the conflicting adapter
    /// names so callers can disambiguate.
    AmbiguousUserId {
        user_id: String,
        candidates: Vec<String>,
    },
    /// The inferred adapter rejected `get_user` (most often
    /// `AdapterError::Unsupported("get_user")`).
    AdapterError(String),
}

impl fmt::Display for GetUserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownUserIdFormat(id) => {
                write!(f, "Cannot infer adapter from userId \"{id}\"")
            }
            Self::AmbiguousUserId { user_id, candidates } => write!(
                f,
                "Numeric userId \"{user_id}\" is ambiguous between adapters: {}. Call the platform's adapter directly (e.g. `adapter.getUser(userId)`).",
                candidates.join(", ")
            ),
            Self::AdapterError(msg) => write!(f, "get_user failed: {msg}"),
        }
    }
}

impl std::error::Error for GetUserError {}

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

/// Boxed future returned by an event-handler closure. The handler is
/// `async`, so each invocation produces a fresh `Pin<Box<dyn Future>>`.
pub type HandlerFuture = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

/// 1:1 with upstream `MentionHandler<TState>` — invoked when a new
/// `@bot` mention arrives in a non-subscribed thread. The closure
/// receives a [`Thread`] handle bound to the matched adapter + a clone
/// of the message; the upstream third `context` parameter is deferred
/// behind a `MessageContext` port.
pub type MentionHandler = Arc<
    dyn Fn(Thread, crate::message::Message) -> HandlerFuture + Send + Sync + 'static,
>;

/// 1:1 with upstream `SubscribedMessageHandler<TState>` — invoked for
/// every message in a thread previously subscribed via
/// `thread.subscribe()`. Subscribed handlers take priority over
/// mention handlers; in a subscribed thread, even an `@bot` mention
/// fires the subscribed handler (not the mention handler).
pub type SubscribedMessageHandler = Arc<
    dyn Fn(Thread, crate::message::Message) -> HandlerFuture + Send + Sync + 'static,
>;

/// 1:1 with upstream `DirectMessageHandler<TState>` — invoked for
/// every message in a DM thread (`adapter.is_dm(thread_id) == true`)
/// when at least one DM handler is registered. DM handlers take
/// priority over both subscribed and mention handlers. The closure
/// receives a [`crate::channel::Channel`] as a third argument
/// (matching upstream's `(thread, message, channel)` signature) so
/// adopters can post channel-scoped replies (or look up channel
/// metadata) inside the handler.
pub type DirectMessageHandler = Arc<
    dyn Fn(Thread, crate::message::Message, crate::channel::Channel) -> HandlerFuture
        + Send
        + Sync
        + 'static,
>;

/// 1:1 with upstream `MessageHandler<TState>` — invoked for messages
/// in unsubscribed threads whose text matches a registered regex
/// pattern. Pattern handlers fire only when no higher-priority
/// branch (DM / subscribed / mention) handled the message.
pub type MessageHandler = Arc<
    dyn Fn(Thread, crate::message::Message) -> HandlerFuture + Send + Sync + 'static,
>;

/// Stored regex+handler pair for [`Chat::on_new_message`].
struct MessagePattern {
    pattern: regex::Regex,
    handler: MessageHandler,
}

/// 1:1 with upstream `ReactionEvent` minus the `thread` field. The
/// dispatcher constructs the [`Thread`] from `thread_id` and the
/// dispatching adapter before invoking the handler.
#[derive(Clone, Debug)]
pub struct ReactionEventInput {
    /// Normalized emoji value (`{{emoji:name}}` placeholder).
    pub emoji: crate::types::EmojiValue,
    /// Platform-native emoji string as received from the source
    /// (e.g. Slack `+1`, Teams `like`).
    pub raw_emoji: String,
    /// `true` for reaction-added events, `false` for reaction-removed
    /// events.
    pub added: bool,
    /// Author of the reaction.
    pub user: crate::types::Author,
    /// Platform message id the reaction is attached to.
    pub message_id: String,
    /// Platform thread id containing the reacting message.
    pub thread_id: String,
    /// Platform-specific raw event payload.
    pub raw: serde_json::Value,
}

/// 1:1 with upstream `ReactionEvent` — the input shape plus a
/// dispatcher-constructed [`Thread`] handle for posting back.
#[derive(Clone, Debug)]
pub struct ReactionEvent {
    pub emoji: crate::types::EmojiValue,
    pub raw_emoji: String,
    pub added: bool,
    pub user: crate::types::Author,
    pub message_id: String,
    pub thread_id: String,
    pub raw: serde_json::Value,
    pub thread: Thread,
}

/// 1:1 with upstream `EmojiFilter` — either a normalized
/// [`crate::types::EmojiValue`] or a platform-native raw-emoji
/// string. A filter matches when the reaction event's `emoji.name`
/// equals the filter's emoji name OR when the raw-emoji string
/// matches the filter exactly (1:1 with upstream's `match` helper).
#[derive(Clone, Debug)]
pub enum EmojiFilter {
    /// Match the reaction by normalized emoji name. Mirrors the
    /// upstream `EmojiValue` filter form.
    Emoji(crate::types::EmojiValue),
    /// Match the reaction by raw-emoji string. Mirrors the upstream
    /// `string` filter form.
    Raw(String),
}

impl From<crate::types::EmojiValue> for EmojiFilter {
    fn from(v: crate::types::EmojiValue) -> Self {
        Self::Emoji(v)
    }
}

impl From<String> for EmojiFilter {
    fn from(s: String) -> Self {
        Self::Raw(s)
    }
}

impl From<&str> for EmojiFilter {
    fn from(s: &str) -> Self {
        Self::Raw(s.to_string())
    }
}

/// 1:1 with upstream `ReactionHandler` — invoked once per filtered
/// reaction event.
pub type ReactionHandler =
    Arc<dyn Fn(ReactionEvent) -> HandlerFuture + Send + Sync + 'static>;

/// Stored filter+handler pair for [`Chat::on_reaction_filtered`].
struct ReactionRegistration {
    /// `None` means "match all reactions" (1:1 with upstream's
    /// no-filter `onReaction(handler)` form).
    filters: Option<Vec<EmojiFilter>>,
    handler: ReactionHandler,
}

/// 1:1 with upstream `ActionEvent` minus the `thread` and
/// `openModal` fields. The dispatcher constructs the [`Thread`]
/// from `thread_id`; `openModal` lands in a follow-up slice.
#[derive(Clone, Debug)]
pub struct ActionEventInput {
    /// Block-kit / cards action id (e.g. `"approve"`).
    pub action_id: String,
    /// Optional action value payload (e.g. `"order-123"`).
    pub value: Option<String>,
    /// Author of the action (button click / menu selection).
    pub user: crate::types::Author,
    /// Platform message id the action originated from.
    pub message_id: String,
    /// Platform thread id containing the action surface.
    pub thread_id: String,
    /// Platform-specific raw event payload.
    pub raw: serde_json::Value,
}

/// 1:1 with upstream `ActionEvent` — the input shape plus a
/// dispatcher-constructed [`Thread`] handle for posting back. The
/// upstream `openModal` callback is deferred until the
/// `Adapter::open_modal` trait method lands behind a follow-up
/// slice; for now adopters can call `chat.open_modal` directly via
/// the existing modal pipeline.
#[derive(Clone, Debug)]
pub struct ActionEvent {
    pub action_id: String,
    pub value: Option<String>,
    pub user: crate::types::Author,
    pub message_id: String,
    pub thread_id: String,
    pub raw: serde_json::Value,
    pub thread: Thread,
}

/// 1:1 with upstream `ActionHandler` — invoked per matching action
/// event. Receives the dispatcher-constructed [`ActionEvent`].
pub type ActionHandler =
    Arc<dyn Fn(ActionEvent) -> HandlerFuture + Send + Sync + 'static>;

/// Stored filter+handler pair for [`Chat::on_action_filtered`].
struct ActionRegistration {
    /// `None` means "match all actions" (1:1 with upstream's
    /// no-filter `onAction(handler)` form). Otherwise matches when
    /// the event's `action_id` equals any string in the vec.
    filters: Option<Vec<String>>,
    handler: ActionHandler,
}

/// 1:1 with upstream `SlashCommandEvent` minus the `channel` /
/// `openModal` fields. The dispatcher constructs the
/// [`crate::channel::Channel`] from `channel_id`; the openModal
/// callback is deferred behind a follow-up slice.
#[derive(Clone, Debug)]
pub struct SlashCommandEventInput {
    /// Slash-command name as received (e.g. `"/help"`). Always
    /// includes the leading slash.
    pub command: String,
    /// Trailing text passed alongside the command (e.g. `"topic"`).
    pub text: String,
    /// Author of the slash-command invocation.
    pub user: crate::types::Author,
    /// Platform channel id where the command was issued.
    pub channel_id: String,
    /// Optional trigger id (Slack-only; used to open modals).
    pub trigger_id: Option<String>,
    /// Platform-specific raw event payload.
    pub raw: serde_json::Value,
}

/// 1:1 with upstream `SlashCommandEvent` — the input shape plus a
/// dispatcher-constructed [`crate::channel::Channel`] handle for
/// channel-scoped replies.
#[derive(Clone, Debug)]
pub struct SlashCommandEvent {
    pub command: String,
    pub text: String,
    pub user: crate::types::Author,
    pub channel_id: String,
    pub trigger_id: Option<String>,
    pub raw: serde_json::Value,
    pub channel: crate::channel::Channel,
}

/// 1:1 with upstream `SlashCommandHandler` — invoked per matching
/// slash-command event.
pub type SlashCommandHandler =
    Arc<dyn Fn(SlashCommandEvent) -> HandlerFuture + Send + Sync + 'static>;

/// Stored filter+handler pair for [`Chat::on_slash_command_filtered`].
struct SlashCommandRegistration {
    /// `None` means "match all slash commands" (1:1 with upstream's
    /// no-filter `onSlashCommand(handler)` form). Otherwise matches
    /// when the event's `command` equals any string in the vec
    /// after both sides are normalized to include a leading slash.
    filters: Option<Vec<String>>,
    handler: SlashCommandHandler,
}

/// Normalize a slash-command filter string to always include the
/// leading `/`. Matches upstream's "should normalize command names
/// without leading slash" behavior (`onSlashCommand("help", ...)`
/// matches `command: "/help"`).
fn normalize_slash_command(cmd: &str) -> String {
    if cmd.starts_with('/') {
        cmd.to_string()
    } else {
        format!("/{cmd}")
    }
}

/// 1:1 with upstream `OptionsLoadEvent` — invoked by the platform
/// when a dynamic select-menu needs its options populated. The
/// handler returns a list of option entries (or option groups)
/// that the platform renders inline.
#[derive(Clone, Debug)]
pub struct OptionsLoadEvent {
    /// Block-kit / cards action id of the select-menu (e.g.
    /// `"person_select"`).
    pub action_id: String,
    /// Typed-ahead query string entered by the user.
    pub query: String,
    /// Author of the typeahead query.
    pub user: crate::types::Author,
    /// Platform-specific raw event payload.
    pub raw: serde_json::Value,
}

/// 1:1 with upstream `OptionsLoadHandler` — returns the option
/// entries the platform should render in the select-menu. The
/// return type is `serde_json::Value` to accommodate both flat
/// option lists and grouped option lists (upstream's
/// `OptionGroup<TValue> | OptionItem<TValue>` union) without
/// requiring callers to commit to a single shape.
///
/// Returning `Ok(serde_json::Value::Null)` is interpreted as "no
/// options" — the dispatcher proceeds to the next handler. Returning
/// `Err` logs the error and the dispatcher also proceeds.
pub type OptionsLoadHandler = Arc<
    dyn Fn(OptionsLoadEvent) -> OptionsLoadFuture + Send + Sync + 'static,
>;

/// Boxed future returned by an [`OptionsLoadHandler`]. Carries the
/// `Result<serde_json::Value, Box<dyn Error>>` instead of `()` so
/// handler errors are recoverable (upstream's "should continue after
/// handler errors" behavior).
pub type OptionsLoadFuture = std::pin::Pin<
    Box<
        dyn std::future::Future<
                Output = Result<
                    serde_json::Value,
                    Box<dyn std::error::Error + Send + Sync>,
                >,
            > + Send,
    >,
>;

/// Stored filter+handler pair for [`Chat::on_options_load`] /
/// [`Chat::on_options_load_filtered`].
struct OptionsLoadRegistration {
    /// `None` means "catch-all" (runs only when no specific
    /// handler succeeded). Otherwise matches when the event's
    /// `action_id` equals any string in the vec.
    filters: Option<Vec<String>>,
    handler: OptionsLoadHandler,
}

/// Per-Chat handler storage. Each handler vec is wrapped in
/// `Arc<Mutex<...>>` so registration goes through `&self` (matching
/// upstream's mutating-but-not-`&mut` shape) while keeping
/// [`Chat::clone`] cheap.
#[derive(Clone, Default)]
struct ChatHandlers {
    mention: Arc<std::sync::Mutex<Vec<MentionHandler>>>,
    subscribed: Arc<std::sync::Mutex<Vec<SubscribedMessageHandler>>>,
    direct_message: Arc<std::sync::Mutex<Vec<DirectMessageHandler>>>,
    message_patterns: Arc<std::sync::Mutex<Vec<MessagePattern>>>,
    reaction: Arc<std::sync::Mutex<Vec<ReactionRegistration>>>,
    action: Arc<std::sync::Mutex<Vec<ActionRegistration>>>,
    slash_command: Arc<std::sync::Mutex<Vec<SlashCommandRegistration>>>,
    options_load: Arc<std::sync::Mutex<Vec<OptionsLoadRegistration>>>,
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
    /// Optional identity resolver. 1:1 with upstream `identity?` —
    /// invoked by `handle_incoming_message` to populate
    /// `message.user_key` before handlers run (slice 387).
    identity: Option<Arc<dyn IdentityResolver>>,
    /// Registered event handlers (slice 415). Wrapped in
    /// `Arc<Mutex<...>>` so registration works through `&self` and
    /// handler dispatch can snapshot the vec under a short lock.
    handlers: ChatHandlers,
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
            identity: options.identity,
            handlers: ChatHandlers::default(),
        })
    }

    /// 1:1 port of upstream `chat.onNewMention(handler)`. Registers a
    /// handler that fires for messages containing a bot mention in a
    /// non-subscribed thread. Multiple handlers are invoked in
    /// registration order; each runs to completion before the next
    /// starts (matching upstream's sequential `await` loop).
    ///
    /// The closure must be `Send + Sync + 'static` because the
    /// dispatcher runs handlers across async tasks. Use
    /// [`HandlerFuture`] as the return type:
    ///
    /// ```ignore
    /// chat.on_new_mention(|thread, message| Box::pin(async move {
    ///     thread.post(&format!("got: {}", message.text)).await.unwrap();
    /// }));
    /// ```
    pub fn on_new_mention<F>(&self, handler: F)
    where
        F: Fn(Thread, crate::message::Message) -> HandlerFuture + Send + Sync + 'static,
    {
        self.handlers.mention.lock().unwrap().push(Arc::new(handler));
    }

    /// 1:1 port of upstream `chat.onSubscribedMessage(handler)`.
    /// Registers a handler that fires for every message in a thread
    /// previously subscribed via `thread.subscribe()`. Subscribed
    /// handlers take priority over mention handlers — a thread that
    /// is subscribed routes to `onSubscribedMessage`, NOT to
    /// `onNewMention`, even when the message contains an `@bot`
    /// mention.
    pub fn on_subscribed_message<F>(&self, handler: F)
    where
        F: Fn(Thread, crate::message::Message) -> HandlerFuture + Send + Sync + 'static,
    {
        self.handlers
            .subscribed
            .lock()
            .unwrap()
            .push(Arc::new(handler));
    }

    /// 1:1 port of upstream `chat.onDirectMessage(handler)`. Registers
    /// a handler that fires for every message in a DM thread (per
    /// `adapter.is_dm(thread_id)`) when at least one DM handler is
    /// registered. DM handlers take priority over subscribed and
    /// mention handlers — a subscribed DM thread still routes to
    /// `onDirectMessage`, not `onSubscribedMessage`.
    ///
    /// If no DM handlers are registered, DM threads fall through to
    /// the normal mention/subscribed cascade (matches upstream's
    /// "fall through to onNewMention" behavior).
    pub fn on_direct_message<F>(&self, handler: F)
    where
        F: Fn(Thread, crate::message::Message, crate::channel::Channel) -> HandlerFuture
            + Send
            + Sync
            + 'static,
    {
        self.handlers
            .direct_message
            .lock()
            .unwrap()
            .push(Arc::new(handler));
    }

    /// 1:1 port of upstream `chat.onNewMessage(pattern, handler)`.
    /// Registers a handler that fires for messages in unsubscribed
    /// threads whose text matches the regex pattern. Pattern handlers
    /// run only when no higher-priority branch (DM / subscribed /
    /// mention) handles the message — they're the fallback for
    /// unmatched messages.
    ///
    /// All registered patterns are tested against the message text;
    /// every matching pattern's handler fires in registration order.
    pub fn on_new_message<F>(&self, pattern: regex::Regex, handler: F)
    where
        F: Fn(Thread, crate::message::Message) -> HandlerFuture + Send + Sync + 'static,
    {
        self.handlers.message_patterns.lock().unwrap().push(MessagePattern {
            pattern,
            handler: Arc::new(handler),
        });
    }

    /// 1:1 port of upstream `chat.onReaction(handler)` no-filter
    /// overload. Registers a handler that fires for every reaction
    /// event (except reactions from the bot itself).
    pub fn on_reaction<F>(&self, handler: F)
    where
        F: Fn(ReactionEvent) -> HandlerFuture + Send + Sync + 'static,
    {
        self.handlers.reaction.lock().unwrap().push(ReactionRegistration {
            filters: None,
            handler: Arc::new(handler),
        });
    }

    /// 1:1 port of upstream `chat.onReaction(emojiFilters, handler)`
    /// filter overload. Registers a handler that fires only for
    /// reactions matching any of the supplied [`EmojiFilter`]s — a
    /// reaction matches when its normalized `emoji.name` equals the
    /// filter's emoji name OR its `raw_emoji` string equals the
    /// filter's raw string.
    pub fn on_reaction_filtered<F, I, E>(&self, filters: I, handler: F)
    where
        F: Fn(ReactionEvent) -> HandlerFuture + Send + Sync + 'static,
        I: IntoIterator<Item = E>,
        E: Into<EmojiFilter>,
    {
        let filters: Vec<EmojiFilter> = filters.into_iter().map(Into::into).collect();
        self.handlers.reaction.lock().unwrap().push(ReactionRegistration {
            filters: Some(filters),
            handler: Arc::new(handler),
        });
    }

    /// 1:1 port of upstream `chat.processReaction(event)`. Dispatches
    /// the reaction to every registered [`ReactionHandler`] whose
    /// filter matches (or any handler with no filter). Skips
    /// dispatch when `event.user.is_me` (upstream's "skip reactions
    /// from self" gate). The dispatcher constructs the [`Thread`]
    /// from `event.thread_id` and the dispatching adapter.
    /// 1:1 port of upstream `chat.onAction(handler)` no-filter
    /// overload. Registers a handler that fires for every action
    /// event (except actions from the bot itself).
    pub fn on_action<F>(&self, handler: F)
    where
        F: Fn(ActionEvent) -> HandlerFuture + Send + Sync + 'static,
    {
        self.handlers.action.lock().unwrap().push(ActionRegistration {
            filters: None,
            handler: Arc::new(handler),
        });
    }

    /// 1:1 port of upstream `chat.onAction(actionIds, handler)`
    /// filter overload. Registers a handler that fires only for
    /// actions whose `action_id` equals any of the supplied
    /// filter strings. Accepts both single-string (`"approve"`) and
    /// array (`["approve", "reject"]`) forms via the `IntoIterator`
    /// signature.
    pub fn on_action_filtered<F, I, S>(&self, action_ids: I, handler: F)
    where
        F: Fn(ActionEvent) -> HandlerFuture + Send + Sync + 'static,
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let filters: Vec<String> = action_ids.into_iter().map(Into::into).collect();
        self.handlers.action.lock().unwrap().push(ActionRegistration {
            filters: Some(filters),
            handler: Arc::new(handler),
        });
    }

    /// 1:1 port of upstream `chat.processAction(event)`. Dispatches
    /// the action to every registered [`ActionHandler`] whose filter
    /// matches (or any handler with no filter). Skips dispatch when
    /// `event.user.is_me` (upstream's "skip actions from self"
    /// gate). The dispatcher constructs the [`Thread`] from
    /// `event.thread_id` and the dispatching adapter.
    pub async fn process_action(&self, adapter: &dyn Adapter, event: ActionEventInput) {
        // 1:1 with upstream "Skip actions from self".
        if event.user.is_me {
            return;
        }

        let adapter_arc = match self.adapters.get(adapter.name()).cloned() {
            Some(a) => a,
            None => return,
        };

        let handlers_snapshot: Vec<(Option<Vec<String>>, ActionHandler)> = self
            .handlers
            .action
            .lock()
            .unwrap()
            .iter()
            .map(|r| (r.filters.clone(), r.handler.clone()))
            .collect();

        for (filters, handler) in handlers_snapshot {
            let matches = match filters {
                None => true,
                Some(filters) => filters.iter().any(|f| f.as_str() == event.action_id.as_str()),
            };
            if !matches {
                continue;
            }
            let thread = Thread::new(adapter_arc.clone(), &event.thread_id);
            let action = ActionEvent {
                action_id: event.action_id.clone(),
                value: event.value.clone(),
                user: event.user.clone(),
                message_id: event.message_id.clone(),
                thread_id: event.thread_id.clone(),
                raw: event.raw.clone(),
                thread,
            };
            handler(action).await;
        }
    }

    /// 1:1 port of upstream `chat.onSlashCommand(handler)`
    /// no-filter overload. Registers a handler that fires for every
    /// slash-command event (except commands from the bot itself).
    pub fn on_slash_command<F>(&self, handler: F)
    where
        F: Fn(SlashCommandEvent) -> HandlerFuture + Send + Sync + 'static,
    {
        self.handlers
            .slash_command
            .lock()
            .unwrap()
            .push(SlashCommandRegistration {
                filters: None,
                handler: Arc::new(handler),
            });
    }

    /// 1:1 port of upstream `chat.onSlashCommand(commands, handler)`
    /// filter overload. Registers a handler that fires only for
    /// commands whose normalized name (with leading `/`) matches
    /// any of the supplied filter strings. Accepts both single-
    /// string and array forms via the `IntoIterator` signature.
    /// Filters without a leading `/` are normalized to include one
    /// (matches upstream's "should normalize command names without
    /// leading slash" behavior).
    pub fn on_slash_command_filtered<F, I, S>(&self, commands: I, handler: F)
    where
        F: Fn(SlashCommandEvent) -> HandlerFuture + Send + Sync + 'static,
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let filters: Vec<String> = commands
            .into_iter()
            .map(|s| normalize_slash_command(&s.into()))
            .collect();
        self.handlers
            .slash_command
            .lock()
            .unwrap()
            .push(SlashCommandRegistration {
                filters: Some(filters),
                handler: Arc::new(handler),
            });
    }

    /// 1:1 port of upstream `chat.processSlashCommand(event)`.
    /// Dispatches the slash-command to every registered
    /// [`SlashCommandHandler`] whose filter matches (or any handler
    /// with no filter). Skips dispatch when `event.user.is_me`
    /// (upstream's "skip slash commands from self" gate). The
    /// dispatcher constructs the [`crate::channel::Channel`] from
    /// `event.channel_id` and the dispatching adapter.
    pub async fn process_slash_command(
        &self,
        adapter: &dyn Adapter,
        event: SlashCommandEventInput,
    ) {
        // 1:1 with upstream "Skip slash commands from self".
        if event.user.is_me {
            return;
        }

        let adapter_arc = match self.adapters.get(adapter.name()).cloned() {
            Some(a) => a,
            None => return,
        };

        let handlers_snapshot: Vec<(Option<Vec<String>>, SlashCommandHandler)> = self
            .handlers
            .slash_command
            .lock()
            .unwrap()
            .iter()
            .map(|r| (r.filters.clone(), r.handler.clone()))
            .collect();

        for (filters, handler) in handlers_snapshot {
            let matches = match filters {
                None => true,
                Some(filters) => filters.iter().any(|f| f.as_str() == event.command.as_str()),
            };
            if !matches {
                continue;
            }
            let channel = crate::channel::Channel::new(adapter_arc.clone(), &event.channel_id);
            let slash = SlashCommandEvent {
                command: event.command.clone(),
                text: event.text.clone(),
                user: event.user.clone(),
                channel_id: event.channel_id.clone(),
                trigger_id: event.trigger_id.clone(),
                raw: event.raw.clone(),
                channel,
            };
            handler(slash).await;
        }
    }

    /// 1:1 port of upstream `chat.onOptionsLoad(handler)`
    /// catch-all overload. Catch-all handlers fire only when no
    /// specific (action-id-filtered) handler succeeded for the
    /// matching event.
    pub fn on_options_load<F>(&self, handler: F)
    where
        F: Fn(OptionsLoadEvent) -> OptionsLoadFuture + Send + Sync + 'static,
    {
        self.handlers
            .options_load
            .lock()
            .unwrap()
            .push(OptionsLoadRegistration {
                filters: None,
                handler: Arc::new(handler),
            });
    }

    /// 1:1 port of upstream `chat.onOptionsLoad(actionId, handler)`
    /// (also covers the `onOptionsLoad(actionIds[], handler)`
    /// overload via the IntoIterator signature). Specific handlers
    /// run before catch-all handlers; the first specific handler
    /// to return successfully provides the options list.
    pub fn on_options_load_filtered<F, I, S>(&self, action_ids: I, handler: F)
    where
        F: Fn(OptionsLoadEvent) -> OptionsLoadFuture + Send + Sync + 'static,
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let filters: Vec<String> = action_ids.into_iter().map(Into::into).collect();
        self.handlers
            .options_load
            .lock()
            .unwrap()
            .push(OptionsLoadRegistration {
                filters: Some(filters),
                handler: Arc::new(handler),
            });
    }

    /// 1:1 port of upstream `chat.processOptionsLoad(event)`.
    /// Dispatches the options-load event to specific handlers first
    /// (in registration order); falls back to catch-all handlers
    /// when no specific handler returns a non-empty result. Continues
    /// past handler errors per upstream's "should continue after
    /// handler errors" behavior. Returns the first non-empty,
    /// non-erroring options payload, or `None` if no handler
    /// succeeded.
    pub async fn process_options_load(
        &self,
        _adapter: &dyn Adapter,
        event: OptionsLoadEvent,
    ) -> Option<serde_json::Value> {
        let registrations: Vec<(Option<Vec<String>>, OptionsLoadHandler)> = self
            .handlers
            .options_load
            .lock()
            .unwrap()
            .iter()
            .map(|r| (r.filters.clone(), r.handler.clone()))
            .collect();

        // First pass: specific handlers (filters.is_some() and
        // matches event.action_id).
        for (filters, handler) in registrations.iter() {
            let Some(filters) = filters else { continue };
            if !filters.iter().any(|f| f.as_str() == event.action_id.as_str()) {
                continue;
            }
            match handler(event.clone()).await {
                Ok(serde_json::Value::Null) => continue,
                Ok(value) => return Some(value),
                Err(_) => continue, // logger error path deferred
            }
        }

        // Second pass: catch-all handlers (filters.is_none()).
        for (filters, handler) in registrations.iter() {
            if filters.is_some() {
                continue;
            }
            match handler(event.clone()).await {
                Ok(serde_json::Value::Null) => continue,
                Ok(value) => return Some(value),
                Err(_) => continue,
            }
        }

        None
    }

    pub async fn process_reaction(&self, adapter: &dyn Adapter, event: ReactionEventInput) {
        // 1:1 with upstream "Skip reactions from self".
        if event.user.is_me {
            return;
        }

        let adapter_arc = match self.adapters.get(adapter.name()).cloned() {
            Some(a) => a,
            None => return,
        };

        let handlers_snapshot: Vec<(Option<Vec<EmojiFilter>>, ReactionHandler)> = self
            .handlers
            .reaction
            .lock()
            .unwrap()
            .iter()
            .map(|r| (r.filters.clone(), r.handler.clone()))
            .collect();

        for (filters, handler) in handlers_snapshot {
            let matches = match filters {
                None => true,
                Some(filters) => filters.iter().any(|f| {
                    // 1:1 with upstream `chat.ts:1660-1680`: for each
                    // filter, extract the "filter name" string and
                    // match against EITHER the normalized
                    // `event.emoji.name` OR the raw `event.raw_emoji`.
                    // This lets the same `["thumbs_up"]` filter match
                    // both `(emoji=thumbs_up, raw=+1)` (Slack) and
                    // `(emoji=thumbs_up, raw=like)` (Teams).
                    let filter_name = match f {
                        EmojiFilter::Emoji(value) => value.name.as_str(),
                        EmojiFilter::Raw(s) => s.as_str(),
                    };
                    filter_name == event.emoji.name.as_str()
                        || filter_name == event.raw_emoji.as_str()
                }),
            };
            if !matches {
                continue;
            }
            let thread = Thread::new(adapter_arc.clone(), &event.thread_id);
            let reaction = ReactionEvent {
                emoji: event.emoji.clone(),
                raw_emoji: event.raw_emoji.clone(),
                added: event.added,
                user: event.user.clone(),
                message_id: event.message_id.clone(),
                thread_id: event.thread_id.clone(),
                raw: event.raw.clone(),
                thread,
            };
            handler(reaction).await;
        }
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

    /// 1:1 port (early-exit subset) of upstream's
    /// `Chat.handleIncomingMessage(adapter, threadId, message)`.
    /// This slice ships only the two upstream early-exit paths:
    ///
    /// 1. **Self-skip**: if `message.author.is_me`, return early
    ///    without recording state or dispatching handlers.
    /// 2. **Dedup**: if the same `(adapter, message.id)` pair has
    ///    already been processed within [`DEDUPE_TTL_MS`], return
    ///    early.
    ///
    /// The full upstream method also threads through lock
    /// acquisition, per-thread-history persistence, and the
    /// concurrency strategy dispatcher — those land in follow-up
    /// slices alongside the event-handler trait surface
    /// (`on_new_mention`, `on_subscribed_message`, etc.).
    ///
    /// Returns `Ok(true)` when the message was newly dispatched
    /// (passed both early-exit gates), `Ok(false)` when an
    /// early-exit applied. Returns `Err` only when the state
    /// adapter's `set_if_not_exists` itself errors — *not* for
    /// downstream dispatcher errors (those land in follow-ups).
    pub async fn handle_incoming_message(
        &self,
        adapter: &dyn Adapter,
        _thread_id: &str,
        message: &mut crate::message::Message,
    ) -> crate::types::StateResult<bool> {
        // 1:1 with upstream "Skip messages from self (bot's own
        // messages)" — `if (message.author.isMe) return`.
        if message.author.is_me {
            return Ok(false);
        }

        // 1:1 with upstream "Deduplicate messages atomically" —
        // `dedupe:<adapter>:<message.id>` key with `DEDUPE_TTL_MS`
        // TTL via setIfNotExists.
        let dedupe_key = format!("dedupe:{}:{}", adapter.name(), message.id);
        let is_first = self
            .state
            .set_if_not_exists(
                &dedupe_key,
                serde_json::Value::Bool(true),
                Some(DEDUPE_TTL_MS),
            )
            .await?;
        if !is_first {
            return Ok(false);
        }

        // 1:1 with upstream `transcripts-wiring.test.ts >
        // describe("dispatch hook")` — when an IdentityResolver is
        // configured, invoke it to populate `message.user_key`
        // before handlers run. Upstream behavior on resolver
        // outcomes:
        // - resolver returns a non-empty string → set userKey
        // - resolver returns null / undefined / empty string →
        //   leave userKey untouched (None)
        // - resolver throws → log and proceed with userKey unset
        //   (the Rust port treats `Err` from the resolver the same
        //   way once the trait surface adopts `Result`; the
        //   current `Option<String>` return shape encodes the
        //   no-userKey decision via `None`)
        if let Some(resolver) = self.identity.as_ref() {
            if let Some(resolved) = resolver.user_key_for(message).await {
                if !resolved.is_empty() {
                    message.user_key = Some(resolved);
                }
            }
        }

        // Handler dispatch (slices 415, 416, 417, 418). Snapshot
        // each handler list under a short lock then drop the guard
        // before awaiting so handler invocations don't hold the
        // registration mutex. Multi-handler dispatch is sequential to
        // match upstream's `await` loop semantics.
        //
        // Routing priority (Phase A+B+C+D scope), 1:1 with upstream
        // `Chat.handleIncomingMessage`:
        // 1. If `adapter.is_dm(thread_id)` AND at least one DM
        //    handler is registered → dispatch to `onDirectMessage`,
        //    return.
        // 2. If `adapter.is_dm(thread_id)` AND no DM handlers
        //    registered → set `message.is_mention = Some(true)`
        //    (backward-compat: treat DMs as mentions) and fall
        //    through.
        // 3. If the thread is subscribed → dispatch to
        //    `onSubscribedMessage`, return.
        // 4. Else if `message.is_mention == Some(true)` → dispatch
        //    to `onNewMention`, return.
        // 5. Walk message_patterns; fire each matching pattern
        //    handler (every matching pattern handler runs, in
        //    registration order, matching upstream's for-of loop).
        //
        // Full routing (detectMention walker computing is_mention;
        // lock/concurrency dispatch) lands in Phases E+.
        //
        // Skip dispatch entirely if the dispatching adapter isn't in
        // the registered map (upstream tests sometimes pass a
        // standalone mock); the message still counts as dispatched
        // per the `Ok(true)` return contract.
        let adapter_arc = self.adapters.get(adapter.name()).cloned();
        if let Some(adapter_arc) = adapter_arc {
            let is_dm = adapter.is_dm(_thread_id).unwrap_or(false);
            let dm_handlers: Vec<DirectMessageHandler> =
                self.handlers.direct_message.lock().unwrap().clone();
            if is_dm && !dm_handlers.is_empty() {
                let channel_id = crate::channel::derive_channel_id(&*adapter_arc, _thread_id);
                for handler in dm_handlers {
                    let thread = Thread::new(adapter_arc.clone(), _thread_id);
                    let channel = crate::channel::Channel::new(adapter_arc.clone(), &channel_id);
                    handler(thread, message.clone(), channel).await;
                }
                return Ok(true);
            }
            // 1:1 with upstream "Backward compat: treat DMs as
            // mentions when no DM handlers registered".
            if is_dm {
                message.is_mention = Some(true);
            }

            let is_subscribed = self
                .state
                .is_subscribed(_thread_id)
                .await
                .unwrap_or(false);

            if is_subscribed {
                let handlers_snapshot: Vec<SubscribedMessageHandler> =
                    self.handlers.subscribed.lock().unwrap().clone();
                for handler in handlers_snapshot {
                    let thread = Thread::new(adapter_arc.clone(), _thread_id);
                    handler(thread, message.clone()).await;
                }
                return Ok(true);
            }

            if message.is_mention == Some(true) {
                let handlers_snapshot: Vec<MentionHandler> =
                    self.handlers.mention.lock().unwrap().clone();
                for handler in handlers_snapshot {
                    let thread = Thread::new(adapter_arc.clone(), _thread_id);
                    handler(thread, message.clone()).await;
                }
                return Ok(true);
            }

            // Pattern handlers fire as the fallback for unmatched
            // messages. Snapshot the regex+handler pairs (clone the
            // Arc handlers but the Regex needs reconstruction; we
            // store a Vec of (Regex, MessageHandler) clones via the
            // intermediate `MessagePattern` struct).
            let patterns_snapshot: Vec<(regex::Regex, MessageHandler)> = self
                .handlers
                .message_patterns
                .lock()
                .unwrap()
                .iter()
                .map(|mp| (mp.pattern.clone(), mp.handler.clone()))
                .collect();
            for (pattern, handler) in patterns_snapshot {
                if pattern.is_match(&message.text) {
                    let thread = Thread::new(adapter_arc.clone(), _thread_id);
                    handler(thread, message.clone()).await;
                }
            }
        }

        Ok(true)
    }

    /// 1:1 port of upstream `Chat.openDM(user)`. Infers the adapter
    /// from `user_id` (Slack `U.../W...`, GChat `users/...`, Teams
    /// `29:...`, Linear UUID v4, or numeric for Discord/Telegram/
    /// GitHub depending on which adapters are registered), then
    /// calls `adapter.open_dm(user_id)` and returns the resulting
    /// `Thread` handle.
    /// 1:1 port of upstream `chat.openDM(author)` Author-object
    /// overload — extracts `author.userId` and delegates to
    /// [`Self::open_dm`]. Mirrors upstream's `typeof user === "string"
    /// ? user : user.userId` argument-shape dispatch.
    pub async fn open_dm_for_author(
        &self,
        author: &crate::types::Author,
    ) -> Result<Thread, OpenDmError> {
        self.open_dm(&author.user_id).await
    }

    pub async fn open_dm(&self, user_id: &str) -> Result<Thread, OpenDmError> {
        let adapter = self
            .infer_adapter_for_user_id(user_id)
            .ok_or_else(|| OpenDmError::UnknownUserIdFormat(user_id.to_string()))?;
        let thread_id = adapter
            .open_dm(user_id)
            .await
            .map_err(|err| OpenDmError::AdapterError(format!("{err:?}")))?;
        // 1:1 with upstream's `createThread` — derive is_dm from
        // `adapter.is_dm(thread_id) ?? false`. open_dm targets a
        // DM by definition, but the adapter is the source of truth.
        let is_dm = adapter.is_dm(&thread_id).unwrap_or(false);
        Ok(Thread::new(adapter, thread_id).with_is_dm(is_dm))
    }

    /// 1:1 port of upstream `Chat.getUser(user)`. Infers the adapter
    /// from `user_id` via [`Self::infer_adapter_for_user_id`] and
    /// delegates to `adapter.get_user(user_id)`. Returns `Ok(None)`
    /// when the user isn't found at the platform; returns
    /// `Err(GetUserError::UnknownUserIdFormat)` when no adapter
    /// pattern matches; returns `Err(GetUserError::AdapterError)`
    /// when the inferred adapter rejects `get_user`
    /// (most often `AdapterError::Unsupported("get_user")`).
    /// 1:1 port of upstream `chat.getUser(author)` Author-object
    /// overload — extracts `author.userId` and delegates to
    /// [`Self::get_user`].
    pub async fn get_user_for_author(
        &self,
        author: &crate::types::Author,
    ) -> Result<Option<crate::types::UserInfo>, GetUserError> {
        self.get_user(&author.user_id).await
    }

    pub async fn get_user(
        &self,
        user_id: &str,
    ) -> Result<Option<crate::types::UserInfo>, GetUserError> {
        // 1:1 with upstream: numeric IDs check collision across
        // discord/telegram/github registered adapters. Returns
        // AmbiguousUserId when >1 candidate, otherwise the single
        // matching adapter. Non-numeric IDs fall through to the
        // priority-order infer helper.
        let adapter = if is_numeric_user_id(user_id) {
            let mut candidates: Vec<String> = Vec::new();
            if is_discord_snowflake(user_id) && self.get_adapter("discord").is_some() {
                candidates.push("discord".to_string());
            }
            if self.get_adapter("telegram").is_some() {
                candidates.push("telegram".to_string());
            }
            if self.get_adapter("github").is_some() {
                candidates.push("github".to_string());
            }
            if candidates.len() > 1 {
                return Err(GetUserError::AmbiguousUserId {
                    user_id: user_id.to_string(),
                    candidates,
                });
            }
            if let Some(name) = candidates.first() {
                self.get_adapter(name)
            } else {
                self.infer_adapter_for_user_id(user_id)
            }
        } else {
            self.infer_adapter_for_user_id(user_id)
        }
        .ok_or_else(|| GetUserError::UnknownUserIdFormat(user_id.to_string()))?;
        adapter
            .get_user(user_id)
            .await
            .map_err(|err| GetUserError::AdapterError(format!("{err:?}")))
    }

    /// Infer the adapter most likely to own this user id. 1:1 with
    /// upstream's private `inferAdapterFromUserId` — Slack-prefix
    /// (`U.../W...`) routes to slack, GChat-prefix (`users/...`) to
    /// gchat, Teams-prefix (`29:`) to teams, UUID to linear,
    /// numeric to discord/telegram/github (in registration-order
    /// preference). Returns `None` when no adapter pattern matches.
    pub fn infer_adapter_for_user_id(&self, user_id: &str) -> Option<Arc<dyn Adapter>> {
        if user_id.starts_with("users/") {
            if let Some(adapter) = self.get_adapter("gchat") {
                return Some(adapter);
            }
        }
        if user_id.starts_with("29:") {
            if let Some(adapter) = self.get_adapter("teams") {
                return Some(adapter);
            }
        }
        if is_linear_uuid(user_id) {
            if let Some(adapter) = self.get_adapter("linear") {
                return Some(adapter);
            }
        }
        if is_slack_user_id(user_id) {
            if let Some(adapter) = self.get_adapter("slack") {
                return Some(adapter);
            }
        }
        if is_numeric_user_id(user_id) {
            // Discord snowflakes are 17-19 digits; ambiguity with
            // Telegram/GitHub is left to the caller — first match
            // wins in registration-order (upstream raises an
            // AMBIGUOUS_USER_ID error in the multi-candidate case;
            // not modelled here yet).
            if is_discord_snowflake(user_id) {
                if let Some(adapter) = self.get_adapter("discord") {
                    return Some(adapter);
                }
            }
            if let Some(adapter) = self.get_adapter("telegram") {
                return Some(adapter);
            }
            if let Some(adapter) = self.get_adapter("github") {
                return Some(adapter);
            }
        }
        None
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

    /// Adapter whose `open_dm` returns `"<name>:D<user_id>:"` and
    /// `post_message` records calls — mirrors upstream
    /// `createMockAdapter("slack")` for the openDM describe block.
    #[derive(Debug, Default)]
    struct OpenDmAdapter {
        name: String,
        post_calls: Mutex<Vec<(String, String)>>,
    }

    #[async_trait::async_trait]
    impl Adapter for OpenDmAdapter {
        fn name(&self) -> &str {
            &self.name
        }
        async fn open_dm(&self, user_id: &str) -> AdapterResult<String> {
            Ok(format!("{}:D{user_id}:", self.name))
        }
        async fn post_message(&self, thread_id: &str, text: &str) -> AdapterResult<String> {
            self.post_calls
                .lock()
                .unwrap()
                .push((thread_id.to_string(), text.to_string()));
            Ok("msg-id".to_string())
        }
        fn is_dm(&self, thread_id: &str) -> Option<bool> {
            // Slack-style DM thread ids start with `<adapter>:D` —
            // matches the open_dm output above.
            Some(thread_id.starts_with(&format!("{}:D", self.name)))
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

    // ---------- describe("openDM") (3 of 4 upstream cases; Author-object case deferred) ----------
    // 1:1 with upstream `chat.test.ts > describe("openDM")`. The
    // "should accept Author object and extract userId" case needs an
    // `Into<UserId>` impl on `Author` — deferred until the Chat event
    // loop ports.

    fn chat_with_open_dm_adapter(adapter_name: &str) -> (Chat, Arc<OpenDmAdapter>) {
        let adapter = Arc::new(OpenDmAdapter {
            name: adapter_name.to_string(),
            ..Default::default()
        });
        let state: Arc<dyn StateAdapter> = Arc::new(NullState);
        let adapters: Vec<Arc<dyn Adapter>> = vec![adapter.clone() as Arc<dyn Adapter>];
        let chat = Chat::new(ChatOptions {
            state,
            adapters,
            ..Default::default()
        });
        (chat, adapter)
    }

    #[test]
    fn chat_open_dm_should_infer_slack_adapter_from_u_prefixed_user_id() {
        let (chat, adapter) = chat_with_open_dm_adapter("slack");
        let thread = futures_executor::block_on(chat.open_dm("U123456")).unwrap();
        assert_eq!(thread.thread_id(), "slack:DU123456:");
        assert_eq!(thread.adapter().name(), "slack");
        // post_message wasn't called — open_dm only opens, doesn't post.
        assert!(adapter.post_calls.lock().unwrap().is_empty());
    }

    #[test]
    fn chat_open_dm_should_throw_error_for_unknown_user_id_format() {
        let (chat, _adapter) = chat_with_open_dm_adapter("slack");
        let err = futures_executor::block_on(chat.open_dm("invalid-user-id")).unwrap_err();
        assert!(matches!(err, OpenDmError::UnknownUserIdFormat(ref id) if id == "invalid-user-id"));
        assert!(err.to_string().contains("Cannot infer adapter from userId"));
    }

    // ---------- describe("getUser") (4 of 10 upstream cases) ----------
    // 1:1 with upstream `chat.test.ts > describe("getUser")`. The 6
    // deferred cases need Author-object Into trait impl, or
    // multi-adapter inference ambiguity testing, or numeric-id
    // adapter-priority testing — each ports as its own slice.

    use crate::types::UserInfo;

    #[derive(Debug)]
    struct GetUserAdapter {
        name: String,
        result: Mutex<Option<UserInfo>>,
        unsupported: bool,
        calls: Mutex<Vec<String>>,
    }

    impl GetUserAdapter {
        fn new(name: &str, result: Option<UserInfo>) -> Self {
            Self {
                name: name.to_string(),
                result: Mutex::new(result),
                unsupported: false,
                calls: Mutex::new(Vec::new()),
            }
        }
        fn unsupported(name: &str) -> Self {
            Self {
                name: name.to_string(),
                result: Mutex::new(None),
                unsupported: true,
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl Adapter for GetUserAdapter {
        fn name(&self) -> &str {
            &self.name
        }
        async fn get_user(&self, user_id: &str) -> AdapterResult<Option<UserInfo>> {
            self.calls.lock().unwrap().push(user_id.to_string());
            if self.unsupported {
                return Err(AdapterError::Unsupported("get_user"));
            }
            Ok(self.result.lock().unwrap().clone())
        }
    }

    fn chat_with_get_user_adapter(adapter: GetUserAdapter) -> (Chat, Arc<GetUserAdapter>) {
        let adapter = Arc::new(adapter);
        let state: Arc<dyn StateAdapter> = Arc::new(NullState);
        let adapters: Vec<Arc<dyn Adapter>> = vec![adapter.clone() as Arc<dyn Adapter>];
        let chat = Chat::new(ChatOptions {
            state,
            adapters,
            ..Default::default()
        });
        (chat, adapter)
    }

    fn alice() -> UserInfo {
        UserInfo {
            user_id: "U123456".to_string(),
            user_name: "alice".to_string(),
            full_name: "Alice Smith".to_string(),
            email: Some("alice@example.com".to_string()),
            avatar_url: Some("https://example.com/alice.png".to_string()),
            is_bot: false,
        }
    }

    #[test]
    fn chat_get_user_should_return_user_info_from_adapter() {
        let (chat, adapter) =
            chat_with_get_user_adapter(GetUserAdapter::new("slack", Some(alice())));
        let user = futures_executor::block_on(chat.get_user("U123456"))
            .unwrap()
            .unwrap();
        assert_eq!(user.email.as_deref(), Some("alice@example.com"));
        assert_eq!(user.full_name, "Alice Smith");
        let calls = adapter.calls.lock().unwrap();
        assert_eq!(calls.as_slice(), &["U123456".to_string()]);
    }

    #[test]
    fn chat_get_user_should_throw_when_adapter_does_not_support_get_user() {
        let (chat, _adapter) =
            chat_with_get_user_adapter(GetUserAdapter::unsupported("slack"));
        let err = futures_executor::block_on(chat.get_user("U123456")).unwrap_err();
        assert!(matches!(err, GetUserError::AdapterError(ref msg) if msg.contains("get_user")));
    }

    #[test]
    fn chat_get_user_should_return_null_when_user_is_not_found() {
        let (chat, _adapter) = chat_with_get_user_adapter(GetUserAdapter::new("slack", None));
        let user = futures_executor::block_on(chat.get_user("U999999")).unwrap();
        assert!(user.is_none());
    }

    // ---------- describe("isDM") (1 of 3 upstream cases) ----------
    // 1:1 with upstream `chat.test.ts > describe("isDM")`. The 2
    // deferred cases need `handleIncomingMessage` + the chat event
    // dispatcher to wire `adapter.is_dm(thread_id)` into the
    // delivered thread handle.

    #[test]
    fn chat_is_dm_should_return_true_for_dm_threads() {
        let (chat, _adapter) = chat_with_open_dm_adapter("slack");
        let thread = futures_executor::block_on(chat.open_dm("U123456")).unwrap();
        // OpenDmAdapter.is_dm returns true for thread ids prefixed
        // with "<adapter>:D", which matches the open_dm output.
        assert!(thread.is_dm());
    }

    #[test]
    fn chat_get_user_should_throw_error_for_unknown_user_id_format() {
        let (chat, _adapter) =
            chat_with_get_user_adapter(GetUserAdapter::new("slack", Some(alice())));
        let err = futures_executor::block_on(chat.get_user("invalid-user-id")).unwrap_err();
        assert!(
            matches!(err, GetUserError::UnknownUserIdFormat(ref id) if id == "invalid-user-id")
        );
        assert!(err.to_string().contains("Cannot infer adapter from userId"));
    }

    #[test]
    fn chat_open_dm_should_allow_posting_to_dm_thread() {
        let (chat, adapter) = chat_with_open_dm_adapter("slack");
        let thread = futures_executor::block_on(chat.open_dm("U123456")).unwrap();
        futures_executor::block_on(thread.post("Hello via DM!")).unwrap();
        let calls = adapter.post_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "slack:DU123456:");
        assert_eq!(calls[0].1, "Hello via DM!");
    }

    // ---------- describe("openDM") + describe("getUser") Author overloads (2 cases) ----------
    // 1:1 with upstream `chat.test.ts > describe("openDM") >
    // it("should accept Author object and extract userId")` and
    // the parallel getUser case. The Rust port exposes Author
    // dispatch via `open_dm_for_author` / `get_user_for_author`
    // sibling methods (matching upstream's runtime
    // `typeof user === "string" ? user : user.userId` argument-
    // shape branch).

    // ---------- describe("getUser") inference cases (4 of 6 deferred) ----------
    // 1:1 with upstream `chat.test.ts > describe("getUser")` cases
    // that exercise inference via different adapter shapes. The
    // 5th case (AMBIGUOUS_USER_ID) needs the inference helper to
    // detect multi-adapter collisions and return a typed error
    // (deferred — current `infer_adapter_for_user_id` picks the
    // priority-order first match). The 6th (Slack-case-sensitivity)
    // is exercised indirectly by the unambiguous-numeric tests.

    fn chat_with_named_get_user_adapter(adapter_name: &str) -> (Chat, Arc<GetUserAdapter>) {
        chat_with_get_user_adapter(GetUserAdapter::new(adapter_name, Some(alice())))
    }

    #[test]
    fn chat_get_user_should_infer_linear_adapter_from_a_uuid() {
        let (chat, _adapter) = chat_with_named_get_user_adapter("linear");
        let user = futures_executor::block_on(
            chat.get_user("8f1f3c7e-d4e1-4f9a-bf2b-1c3d4e5f6a7b"),
        )
        .unwrap()
        .unwrap();
        // Routes to the only registered adapter (linear) since the
        // UUID shape matches `is_linear_uuid`.
        assert_eq!(user.full_name, "Alice Smith");
    }

    #[test]
    fn chat_get_user_should_infer_telegram_from_numeric_id_when_only_telegram_is_registered() {
        let (chat, adapter) = chat_with_named_get_user_adapter("telegram");
        let user = futures_executor::block_on(chat.get_user("12345"))
            .unwrap()
            .unwrap();
        assert_eq!(user.full_name, "Alice Smith");
        assert_eq!(adapter.calls.lock().unwrap().as_slice(), &["12345".to_string()]);
    }

    #[test]
    fn chat_get_user_should_infer_github_from_numeric_id_when_only_github_is_registered() {
        let (chat, adapter) = chat_with_named_get_user_adapter("github");
        let user = futures_executor::block_on(chat.get_user("12345"))
            .unwrap()
            .unwrap();
        assert_eq!(user.full_name, "Alice Smith");
        assert_eq!(adapter.calls.lock().unwrap().as_slice(), &["12345".to_string()]);
    }

    #[test]
    fn chat_get_user_should_not_match_github_style_logins_as_slack_ids_case_sensitivity() {
        // 1:1 with upstream "should not match GitHub-style logins
        // as Slack ids (case sensitivity)". `user123` is lowercase
        // and must NOT match the case-sensitive Slack regex
        // `^[UW][A-Z0-9]+$`. With slack + github both registered,
        // a lowercase id falls through every pattern and yields
        // UnknownUserIdFormat (not AmbiguousUserId, not a Slack hit).
        let slack = Arc::new(GetUserAdapter::new("slack", Some(alice())));
        let github = Arc::new(GetUserAdapter::new("github", Some(alice())));
        let state: Arc<dyn StateAdapter> = Arc::new(NullState);
        let adapters: Vec<Arc<dyn Adapter>> = vec![
            slack.clone() as Arc<dyn Adapter>,
            github.clone() as Arc<dyn Adapter>,
        ];
        let chat = Chat::new(ChatOptions {
            state,
            adapters,
            ..Default::default()
        });
        let err = futures_executor::block_on(chat.get_user("user123")).unwrap_err();
        assert!(
            matches!(err, GetUserError::UnknownUserIdFormat(ref id) if id == "user123"),
            "expected UnknownUserIdFormat for lowercase id, got {err:?}"
        );
        // Neither adapter was called.
        assert!(slack.calls.lock().unwrap().is_empty());
        assert!(github.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn chat_get_user_should_throw_ambiguous_user_id_when_numeric_id_matches_multiple_registered_adapters() {
        // 1:1 with upstream `it("should throw AMBIGUOUS_USER_ID
        // when numeric id matches multiple registered adapters")`.
        // Registers both telegram + github so a numeric id matches
        // both candidates.
        let telegram = Arc::new(GetUserAdapter::new("telegram", Some(alice())));
        let github = Arc::new(GetUserAdapter::new("github", Some(alice())));
        let state: Arc<dyn StateAdapter> = Arc::new(NullState);
        let adapters: Vec<Arc<dyn Adapter>> = vec![
            telegram.clone() as Arc<dyn Adapter>,
            github.clone() as Arc<dyn Adapter>,
        ];
        let chat = Chat::new(ChatOptions {
            state,
            adapters,
            ..Default::default()
        });
        let err = futures_executor::block_on(chat.get_user("12345")).unwrap_err();
        match err {
            GetUserError::AmbiguousUserId {
                ref user_id,
                ref candidates,
            } => {
                assert_eq!(user_id, "12345");
                assert!(candidates.contains(&"telegram".to_string()));
                assert!(candidates.contains(&"github".to_string()));
            }
            other => panic!("expected AmbiguousUserId, got {other:?}"),
        }
        assert!(err.to_string().contains("ambiguous between adapters"));
        // Neither adapter was called.
        assert!(telegram.calls.lock().unwrap().is_empty());
        assert!(github.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn chat_get_user_should_infer_discord_for_17_to_19_digit_snowflake_when_only_discord_is_registered() {
        let (chat, adapter) = chat_with_named_get_user_adapter("discord");
        let user = futures_executor::block_on(chat.get_user("123456789012345678"))
            .unwrap()
            .unwrap();
        assert_eq!(user.full_name, "Alice Smith");
        assert_eq!(
            adapter.calls.lock().unwrap().as_slice(),
            &["123456789012345678".to_string()]
        );
    }

    #[test]
    fn chat_open_dm_should_accept_author_object_and_extract_user_id() {
        let (chat, adapter) = chat_with_open_dm_adapter("slack");
        let author = crate::types::Author {
            user_id: "U789ABC".to_string(),
            user_name: "testuser".to_string(),
            full_name: "Test User".to_string(),
            is_bot: crate::types::BotStatus::Known(false),
            is_me: false,
        };
        let thread = futures_executor::block_on(chat.open_dm_for_author(&author)).unwrap();
        assert_eq!(thread.thread_id(), "slack:DU789ABC:");
        assert_eq!(thread.adapter().name(), "slack");
        // open_dm doesn't post — verify post_message recorder is empty.
        assert!(adapter.post_calls.lock().unwrap().is_empty());
    }

    #[test]
    fn chat_get_user_should_accept_author_object_and_extract_user_id() {
        let (chat, adapter) =
            chat_with_get_user_adapter(GetUserAdapter::new("slack", Some(alice())));
        let author = crate::types::Author {
            user_id: "U123456".to_string(),
            user_name: "alice".to_string(),
            full_name: "Alice Smith".to_string(),
            is_bot: crate::types::BotStatus::Known(false),
            is_me: false,
        };
        let user = futures_executor::block_on(chat.get_user_for_author(&author))
            .unwrap()
            .unwrap();
        assert_eq!(user.full_name, "Alice Smith");
        let calls = adapter.calls.lock().unwrap();
        assert_eq!(calls.as_slice(), &["U123456".to_string()]);
    }

    // ---------- describe("Chat") > handleIncomingMessage early-exit ----------
    // Slice 347 ports the 2 portable early-exit upstream cases:
    // "should skip messages from self" + "should skip duplicate
    // messages with the same id" (the latter is in
    // `describe("message deduplication")`). The full
    // handler-dispatch cases are deferred until the chat-sdk-chat
    // event-handler trait surface lands (on_new_mention etc.).

    use crate::message::Message;
    use crate::types::{Author, BotStatus, MessageMetadata};

    fn dispatched_message(id: &str, is_me: bool) -> Message {
        use crate::markdown::root;
        Message::new(
            id,
            "slack:C123:1234.5678",
            "hello",
            root(vec![]),
            serde_json::json!({}),
            Author {
                user_id: "U_AUTHOR".to_string(),
                user_name: "author".to_string(),
                full_name: "Author".to_string(),
                is_bot: BotStatus::Known(false),
                is_me,
            },
            MessageMetadata {
                date_sent: "2024-01-15T10:30:00.000Z".to_string(),
                edited: false,
                edited_at: None,
            },
            Vec::new(),
        )
    }

    /// State adapter backed by an in-process HashMap so the
    /// dedupe `set_if_not_exists` round-trips. Uses the
    /// default trait `set_if_not_exists` impl (which is
    /// `get` + `set` — sufficient for dedup semantics in tests).
    #[derive(Debug, Default)]
    struct InMemoryState {
        cache: Mutex<std::collections::HashMap<String, serde_json::Value>>,
    }

    #[async_trait::async_trait]
    impl StateAdapter for InMemoryState {
        async fn get(&self, key: &str) -> StateResult<Option<serde_json::Value>> {
            Ok(self.cache.lock().unwrap().get(key).cloned())
        }
        async fn set(
            &self,
            key: &str,
            value: serde_json::Value,
            _ttl_ms: Option<u64>,
        ) -> StateResult<()> {
            self.cache.lock().unwrap().insert(key.to_string(), value);
            Ok(())
        }
        async fn delete(&self, key: &str) -> StateResult<()> {
            self.cache.lock().unwrap().remove(key);
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

    fn chat_with_in_memory_state() -> (Chat, Arc<dyn Adapter>) {
        let state: Arc<dyn StateAdapter> = Arc::new(InMemoryState::default());
        let adapter: Arc<dyn Adapter> = Arc::new(NamedAdapter {
            name: "slack".to_string(),
        });
        let chat = Chat::new(ChatOptions {
            state,
            adapters: vec![adapter.clone()],
            ..Default::default()
        });
        (chat, adapter)
    }

    #[test]
    fn chat_handle_incoming_message_should_skip_messages_from_self() {
        let (chat, adapter) = chat_with_in_memory_state();
        let mut msg = dispatched_message("msg-1", true);
        let dispatched =
            futures_executor::block_on(chat.handle_incoming_message(adapter.as_ref(), "T1", &mut msg))
                .unwrap();
        // is_me=true → early-exit, returns false (not dispatched).
        assert!(!dispatched);
    }

    #[test]
    fn chat_handle_incoming_message_should_skip_duplicate_messages_with_the_same_id() {
        let (chat, adapter) = chat_with_in_memory_state();
        let mut msg = dispatched_message("msg-1", false);
        // First call: passes both gates, returns true.
        let first =
            futures_executor::block_on(chat.handle_incoming_message(adapter.as_ref(), "T1", &mut msg))
                .unwrap();
        assert!(first);
        // Second call (same id): dedupe gate trips, returns false.
        let second =
            futures_executor::block_on(chat.handle_incoming_message(adapter.as_ref(), "T1", &mut msg))
                .unwrap();
        assert!(!second);
    }

    #[test]
    fn chat_handle_incoming_message_dispatches_new_messages() {
        // Additive: verifies the happy path returns true so the
        // early-exit semantics don't accidentally short-circuit
        // every message.
        let (chat, adapter) = chat_with_in_memory_state();
        let mut msg = dispatched_message("new-msg", false);
        let dispatched =
            futures_executor::block_on(chat.handle_incoming_message(adapter.as_ref(), "T1", &mut msg))
                .unwrap();
        assert!(dispatched);
    }

    // ---------- describe("dispatch hook") (4 mapped + 2 js-only) ----------
    // 1:1 with upstream `transcripts-wiring.test.ts >
    // describe("dispatch hook")`. 4 of 6 portable upstream cases
    // mapped 1:1; 2 unreachable upstream cases per the slice-380
    // type-system-impossible pattern:
    //
    // - `populates message.userKey from a sync resolver that returns
    //   a plain string`: the Rust `IdentityResolver` trait surface
    //   is async-only (`async fn user_key_for(...) -> Option<String>`).
    //   A sync resolver isn't constructible at the type level.
    // - `logs and proceeds without userKey when the resolver throws`:
    //   upstream's resolver can throw; the Rust trait returns
    //   `Option<String>`, not `Result<Option<String>, _>`. To match
    //   upstream's throw + warn-log behavior, the trait would need
    //   the Result variant + a logger trait method that the chat
    //   instance could invoke. Both extensions are tracked as
    //   deferred refinement items — until they land, the throws-case
    //   is unreachable at the Rust type level.
    //
    // Brings transcripts-wiring upstream parity to 9 Rust-mapped
    // (5 construction + 4 dispatch-hook) + 2 js-only-documented =
    // 11/11 upstream cases accounted for.

    #[derive(Debug)]
    struct FixedIdentityResolver {
        result: Option<String>,
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl IdentityResolver for FixedIdentityResolver {
        async fn user_key_for(
            &self,
            _message: &crate::message::Message,
        ) -> Option<String> {
            self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            self.result.clone()
        }
    }

    fn chat_with_identity(
        identity: Arc<FixedIdentityResolver>,
    ) -> (Chat, Arc<dyn Adapter>) {
        let state: Arc<dyn StateAdapter> = Arc::new(InMemoryState::default());
        let adapter: Arc<dyn Adapter> = Arc::new(NamedAdapter {
            name: "slack".to_string(),
        });
        let chat = Chat::new(ChatOptions {
            state,
            adapters: vec![adapter.clone()],
            identity: Some(identity as Arc<dyn IdentityResolver>),
            ..Default::default()
        });
        (chat, adapter)
    }

    #[test]
    fn chat_handle_incoming_should_populate_message_user_key_from_resolver_before_handlers_run() {
        let resolver = Arc::new(FixedIdentityResolver {
            result: Some("user@example.com".to_string()),
            calls: AtomicUsize::new(0),
        });
        let (chat, adapter) = chat_with_identity(resolver.clone());
        let mut msg = dispatched_message("msg-1", false);
        let dispatched = futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "T1", &mut msg),
        )
        .unwrap();
        assert!(dispatched);
        assert_eq!(resolver.calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(msg.user_key.as_deref(), Some("user@example.com"));
    }

    #[test]
    fn chat_handle_incoming_should_leave_user_key_unset_when_resolver_returns_none() {
        let resolver = Arc::new(FixedIdentityResolver {
            result: None,
            calls: AtomicUsize::new(0),
        });
        let (chat, adapter) = chat_with_identity(resolver.clone());
        let mut msg = dispatched_message("msg-1", false);
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "T1", &mut msg),
        )
        .unwrap();
        assert_eq!(resolver.calls.load(AtomicOrdering::SeqCst), 1);
        assert!(msg.user_key.is_none());
    }

    #[test]
    fn chat_handle_incoming_should_treat_resolver_returning_empty_string_as_no_user_key() {
        let resolver = Arc::new(FixedIdentityResolver {
            result: Some("".to_string()),
            calls: AtomicUsize::new(0),
        });
        let (chat, adapter) = chat_with_identity(resolver.clone());
        let mut msg = dispatched_message("msg-1", false);
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "T1", &mut msg),
        )
        .unwrap();
        assert_eq!(resolver.calls.load(AtomicOrdering::SeqCst), 1);
        assert!(msg.user_key.is_none());
    }

    #[test]
    fn chat_handle_incoming_should_not_call_the_resolver_when_no_identity_configured() {
        let (chat, adapter) = chat_with_in_memory_state();
        let mut msg = dispatched_message("msg-1", false);
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "T1", &mut msg),
        )
        .unwrap();
        // No identity configured → user_key stays None and no
        // resolver invocation can happen (no resolver to invoke).
        assert!(msg.user_key.is_none());
    }

    // ---------- describe("onNewMention behavior") — Phase A (slice 415) ----------
    //
    // 1:1 with the simplest upstream `chat.test.ts` mention-dispatch
    // cases. Phase A wires `Chat::on_new_mention` registration + the
    // `handle_incoming_message` dispatcher branch that fires when
    // `message.is_mention == Some(true)`. The upstream `detectMention`
    // computation (walking the formatted AST for `<@botUserId>`) is
    // deferred to Phase B — Phase A trusts whatever the caller set on
    // the message before invoking the dispatcher.

    #[test]
    fn on_new_mention_dispatches_when_message_is_mention_is_true() {
        // 1:1 with upstream "should trigger onNewMention for message
        // events containing a bot mention" (Phase A scope: caller
        // pre-sets `is_mention`; detectMention walker deferred).
        let (chat, adapter) = chat_with_in_memory_state();
        let invocations = Arc::new(AtomicUsize::new(0));
        let counter = invocations.clone();
        chat.on_new_mention(move |_thread, _msg| {
            let c = counter.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let mut msg = dispatched_message("msg-1", false);
        msg.is_mention = Some(true);
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "slack:C123:1234.5678", &mut msg),
        )
        .unwrap();
        assert_eq!(invocations.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn on_new_mention_does_not_dispatch_when_is_mention_is_false() {
        // 1:1 with upstream "should not trigger onNewMention when
        // message event has no bot mention" (Phase A scope).
        let (chat, adapter) = chat_with_in_memory_state();
        let invocations = Arc::new(AtomicUsize::new(0));
        let counter = invocations.clone();
        chat.on_new_mention(move |_thread, _msg| {
            let c = counter.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let mut msg = dispatched_message("msg-1", false);
        msg.is_mention = Some(false);
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "slack:C123:1234.5678", &mut msg),
        )
        .unwrap();
        assert_eq!(invocations.load(AtomicOrdering::SeqCst), 0);
    }

    #[test]
    fn on_new_mention_does_not_dispatch_when_is_mention_is_none() {
        // Phase A behavior: `None` is treated as "no mention", same
        // as `Some(false)`. The upstream detectMention walker (which
        // runs unconditionally) lands in Phase B and will replace
        // the caller-set value before dispatch.
        let (chat, adapter) = chat_with_in_memory_state();
        let invocations = Arc::new(AtomicUsize::new(0));
        let counter = invocations.clone();
        chat.on_new_mention(move |_thread, _msg| {
            let c = counter.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let mut msg = dispatched_message("msg-1", false);
        // is_mention deliberately left as None (default)
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "slack:C123:1234.5678", &mut msg),
        )
        .unwrap();
        assert_eq!(invocations.load(AtomicOrdering::SeqCst), 0);
    }

    #[test]
    fn on_new_mention_invokes_all_registered_handlers_in_order() {
        // Additive: upstream's onNewMention spec doesn't enumerate
        // multi-handler ordering explicitly but does iterate via
        // sequential awaits. This test locks in that handlers fire
        // in registration order.
        let (chat, adapter) = chat_with_in_memory_state();
        let order: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let o1 = order.clone();
        chat.on_new_mention(move |_thread, _msg| {
            let o = o1.clone();
            Box::pin(async move {
                o.lock().unwrap().push(1);
            })
        });
        let o2 = order.clone();
        chat.on_new_mention(move |_thread, _msg| {
            let o = o2.clone();
            Box::pin(async move {
                o.lock().unwrap().push(2);
            })
        });
        let o3 = order.clone();
        chat.on_new_mention(move |_thread, _msg| {
            let o = o3.clone();
            Box::pin(async move {
                o.lock().unwrap().push(3);
            })
        });
        let mut msg = dispatched_message("msg-1", false);
        msg.is_mention = Some(true);
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "slack:C123:1234.5678", &mut msg),
        )
        .unwrap();
        assert_eq!(*order.lock().unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn on_new_mention_handler_receives_thread_bound_to_dispatching_adapter() {
        // Additive: verifies the Thread passed to the handler is
        // constructed from the adapter that dispatched the message.
        // The handler asserts adapter_name() matches.
        let (chat, adapter) = chat_with_in_memory_state();
        let observed: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let o = observed.clone();
        chat.on_new_mention(move |thread, _msg| {
            let o = o.clone();
            let name = thread.adapter_name().to_string();
            Box::pin(async move {
                *o.lock().unwrap() = Some(name);
            })
        });
        let mut msg = dispatched_message("msg-1", false);
        msg.is_mention = Some(true);
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "slack:C123:1234.5678", &mut msg),
        )
        .unwrap();
        assert_eq!(observed.lock().unwrap().as_deref(), Some("slack"));
    }

    // ---------- describe("onNewMention behavior in subscribed threads") — slice 416 ----------
    //
    // 1:1 with upstream `chat.test.ts > describe("onNewMention
    // behavior in subscribed threads")`. Phase B (slice 416) adds the
    // `Chat::on_subscribed_message` registration + the dispatcher's
    // subscribed-thread priority branch: subscribed threads route to
    // `onSubscribedMessage`, NOT to `onNewMention`, even when the
    // message contains a bot mention. State subscription is read via
    // `StateAdapter::is_subscribed`.
    //
    // Construction of the `is_subscribed=true` state is via direct
    // `state.subscribe(thread_id)` calls (the same path `thread.subscribe()`
    // uses). The InMemoryState test mock relies on the StateAdapter
    // trait's default `subscribe/is_subscribed` impl (set/get-backed).

    #[test]
    fn on_subscribed_message_fires_in_subscribed_threads_even_when_mentioned() {
        // 1:1 with upstream "should NOT call onNewMention for
        // mentions in subscribed threads". Both handlers registered;
        // thread subscribed; message has is_mention=true; only
        // subscribed handler should fire.
        let (chat, adapter) = chat_with_in_memory_state();
        let mention_calls = Arc::new(AtomicUsize::new(0));
        let subscribed_calls = Arc::new(AtomicUsize::new(0));
        let m = mention_calls.clone();
        chat.on_new_mention(move |_thread, _msg| {
            let c = m.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let s = subscribed_calls.clone();
        chat.on_subscribed_message(move |_thread, _msg| {
            let c = s.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        // Subscribe the thread BEFORE dispatching the message.
        futures_executor::block_on(chat.state.subscribe("slack:C123:1234.5678")).unwrap();
        let mut msg = dispatched_message("msg-1", false);
        msg.is_mention = Some(true);
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "slack:C123:1234.5678", &mut msg),
        )
        .unwrap();
        assert_eq!(subscribed_calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(mention_calls.load(AtomicOrdering::SeqCst), 0);
    }

    #[test]
    fn on_new_mention_only_fires_in_unsubscribed_threads_when_mentioned() {
        // 1:1 with upstream "should call onNewMention only for
        // mentions in unsubscribed threads". Both handlers
        // registered; thread NOT subscribed; message has
        // is_mention=true; only mention handler should fire.
        let (chat, adapter) = chat_with_in_memory_state();
        let mention_calls = Arc::new(AtomicUsize::new(0));
        let subscribed_calls = Arc::new(AtomicUsize::new(0));
        let m = mention_calls.clone();
        chat.on_new_mention(move |_thread, _msg| {
            let c = m.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let s = subscribed_calls.clone();
        chat.on_subscribed_message(move |_thread, _msg| {
            let c = s.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        // No state.subscribe() call — thread is unsubscribed.
        let mut msg = dispatched_message("msg-1", false);
        msg.is_mention = Some(true);
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "slack:C123:1234.5678", &mut msg),
        )
        .unwrap();
        assert_eq!(mention_calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(subscribed_calls.load(AtomicOrdering::SeqCst), 0);
    }

    #[test]
    fn on_subscribed_message_fires_for_non_mention_messages_in_subscribed_threads() {
        // Additive: subscribed threads fire onSubscribedMessage for
        // EVERY message, not just mentions. Verifies a plain message
        // (is_mention=false) in a subscribed thread still dispatches.
        let (chat, adapter) = chat_with_in_memory_state();
        let subscribed_calls = Arc::new(AtomicUsize::new(0));
        let s = subscribed_calls.clone();
        chat.on_subscribed_message(move |_thread, _msg| {
            let c = s.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        futures_executor::block_on(chat.state.subscribe("slack:C123:1234.5678")).unwrap();
        let mut msg = dispatched_message("msg-1", false);
        msg.is_mention = Some(false);
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "slack:C123:1234.5678", &mut msg),
        )
        .unwrap();
        assert_eq!(subscribed_calls.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn on_subscribed_message_does_not_fire_for_unsubscribed_threads() {
        // Additive: without a state.subscribe() call, the subscribed
        // handler must not fire even for messages that look subscribed
        // (no mention, no other signals).
        let (chat, adapter) = chat_with_in_memory_state();
        let subscribed_calls = Arc::new(AtomicUsize::new(0));
        let s = subscribed_calls.clone();
        chat.on_subscribed_message(move |_thread, _msg| {
            let c = s.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let mut msg = dispatched_message("msg-1", false);
        msg.is_mention = Some(false);
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "slack:C123:1234.5678", &mut msg),
        )
        .unwrap();
        assert_eq!(subscribed_calls.load(AtomicOrdering::SeqCst), 0);
    }

    #[test]
    fn on_subscribed_message_invokes_all_registered_handlers_in_order() {
        // Additive: like the mention multi-handler ordering test —
        // subscribed handlers fire in registration order via
        // sequential awaits.
        let (chat, adapter) = chat_with_in_memory_state();
        let order: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let o1 = order.clone();
        chat.on_subscribed_message(move |_thread, _msg| {
            let o = o1.clone();
            Box::pin(async move {
                o.lock().unwrap().push(1);
            })
        });
        let o2 = order.clone();
        chat.on_subscribed_message(move |_thread, _msg| {
            let o = o2.clone();
            Box::pin(async move {
                o.lock().unwrap().push(2);
            })
        });
        futures_executor::block_on(chat.state.subscribe("slack:C123:1234.5678")).unwrap();
        let mut msg = dispatched_message("msg-1", false);
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "slack:C123:1234.5678", &mut msg),
        )
        .unwrap();
        assert_eq!(*order.lock().unwrap(), vec![1, 2]);
    }

    // ---------- describe("onDirectMessage") — slice 417 ----------
    //
    // 1:1 with upstream `chat.test.ts > describe("onDirectMessage")`.
    // Phase C (slice 417) adds the `Chat::on_direct_message`
    // registration + the dispatcher's DM-thread priority branch:
    // when `adapter.is_dm(thread_id)` returns true AND at least one
    // DM handler is registered, the DM handler fires and the
    // subscribed/mention branches are skipped. The handler receives
    // a `Channel` as a third argument (matching upstream's
    // `(thread, message, channel)` signature).
    //
    // Falls through to the next priority level when no DM handlers
    // are registered (upstream's "fall through to onNewMention when
    // no DM handlers" semantics).

    fn chat_with_dm_adapter() -> (Chat, Arc<OpenDmAdapter>) {
        let state: Arc<dyn StateAdapter> = Arc::new(InMemoryState::default());
        let adapter = Arc::new(OpenDmAdapter {
            name: "slack".to_string(),
            ..Default::default()
        });
        let chat = Chat::new(ChatOptions {
            state,
            adapters: vec![adapter.clone() as Arc<dyn Adapter>],
            ..Default::default()
        });
        (chat, adapter)
    }

    #[test]
    fn on_direct_message_routes_dms_to_dm_handler_with_channel() {
        // 1:1 with upstream "should route DMs to directMessage
        // handler with channel". Both handlers registered; thread is
        // DM-shape (slack:D...); DM handler fires, mention handler
        // does NOT; handler receives the channel.
        let (chat, adapter) = chat_with_dm_adapter();
        let dm_calls = Arc::new(AtomicUsize::new(0));
        let mention_calls = Arc::new(AtomicUsize::new(0));
        let observed_channel: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let d = dm_calls.clone();
        let oc = observed_channel.clone();
        chat.on_direct_message(move |_thread, _msg, channel| {
            let d = d.clone();
            let oc = oc.clone();
            let cid = channel.channel_id().to_string();
            Box::pin(async move {
                d.fetch_add(1, AtomicOrdering::SeqCst);
                *oc.lock().unwrap() = Some(cid);
            })
        });
        let m = mention_calls.clone();
        chat.on_new_mention(move |_thread, _msg| {
            let c = m.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let mut msg = dispatched_message("msg-1", false);
        msg.is_mention = Some(true); // even with mention, DM wins
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "slack:DU123:1234.5678", &mut msg),
        )
        .unwrap();
        assert_eq!(dm_calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(mention_calls.load(AtomicOrdering::SeqCst), 0);
        // Channel id is derived from thread id via OpenDmAdapter
        // (default: returns None, so derive_channel_id falls back to
        // the thread id itself).
        assert_eq!(
            observed_channel.lock().unwrap().as_deref(),
            Some("slack:DU123:1234.5678")
        );
    }

    #[test]
    fn on_direct_message_falls_through_to_on_new_mention_when_no_dm_handlers_registered() {
        // 1:1 with upstream "should fall through to onNewMention
        // when no DM handlers registered". DM-shape thread, mention
        // handler registered but no DM handler — falls through to
        // mention dispatch.
        let (chat, adapter) = chat_with_dm_adapter();
        let mention_calls = Arc::new(AtomicUsize::new(0));
        let m = mention_calls.clone();
        chat.on_new_mention(move |_thread, _msg| {
            let c = m.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let mut msg = dispatched_message("msg-1", false);
        msg.is_mention = Some(true);
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "slack:DU123:1234.5678", &mut msg),
        )
        .unwrap();
        assert_eq!(mention_calls.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn on_direct_message_routes_subscribed_dm_threads_to_dm_not_subscribed() {
        // 1:1 with upstream "should route subscribed DM threads to
        // onDirectMessage, not onSubscribedMessage". DM thread that
        // is ALSO subscribed; both DM + subscribed handlers
        // registered. DM wins.
        let (chat, adapter) = chat_with_dm_adapter();
        let dm_calls = Arc::new(AtomicUsize::new(0));
        let subscribed_calls = Arc::new(AtomicUsize::new(0));
        let d = dm_calls.clone();
        chat.on_direct_message(move |_thread, _msg, _channel| {
            let c = d.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let s = subscribed_calls.clone();
        chat.on_subscribed_message(move |_thread, _msg| {
            let c = s.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        futures_executor::block_on(chat.state.subscribe("slack:DU123:1234.5678")).unwrap();
        let mut msg = dispatched_message("msg-1", false);
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "slack:DU123:1234.5678", &mut msg),
        )
        .unwrap();
        assert_eq!(dm_calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(subscribed_calls.load(AtomicOrdering::SeqCst), 0);
    }

    #[test]
    fn on_direct_message_does_not_route_non_dm_mentions_to_dm_handler() {
        // 1:1 with upstream "should not route non-DM mentions to
        // directMessage handler". Non-DM thread with a mention; DM
        // handler should NOT fire, mention handler SHOULD.
        let (chat, adapter) = chat_with_dm_adapter();
        let dm_calls = Arc::new(AtomicUsize::new(0));
        let mention_calls = Arc::new(AtomicUsize::new(0));
        let d = dm_calls.clone();
        chat.on_direct_message(move |_thread, _msg, _channel| {
            let c = d.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let m = mention_calls.clone();
        chat.on_new_mention(move |_thread, _msg| {
            let c = m.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let mut msg = dispatched_message("msg-1", false);
        msg.is_mention = Some(true);
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "slack:C123:1234.5678", &mut msg),
        )
        .unwrap();
        assert_eq!(dm_calls.load(AtomicOrdering::SeqCst), 0);
        assert_eq!(mention_calls.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn on_direct_message_invokes_all_registered_handlers_in_order() {
        // Additive: multi-handler dispatch ordering for DM handlers.
        let (chat, adapter) = chat_with_dm_adapter();
        let order: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let o1 = order.clone();
        chat.on_direct_message(move |_thread, _msg, _channel| {
            let o = o1.clone();
            Box::pin(async move {
                o.lock().unwrap().push(1);
            })
        });
        let o2 = order.clone();
        chat.on_direct_message(move |_thread, _msg, _channel| {
            let o = o2.clone();
            Box::pin(async move {
                o.lock().unwrap().push(2);
            })
        });
        let mut msg = dispatched_message("msg-1", false);
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "slack:DU123:1234.5678", &mut msg),
        )
        .unwrap();
        assert_eq!(*order.lock().unwrap(), vec![1, 2]);
    }

    // ---------- describe("message patterns") + DM backward-compat — slice 418 ----------
    //
    // 1:1 with upstream `chat.test.ts` pattern-handler cases and the
    // `Backward compat: treat DMs as mentions when no DM handlers
    // registered` branch.
    //
    // Phase D (slice 418) adds the regex pattern handler registration
    // surface + dispatcher fallback branch. Pattern handlers fire as
    // the lowest-priority class — only when DM (with handlers),
    // subscribed, and mention have all not handled the message.

    #[test]
    fn on_new_message_should_match_message_patterns() {
        // 1:1 with upstream "should match message patterns". Registers
        // a single pattern handler; sends a matching message; handler
        // fires.
        let (chat, adapter) = chat_with_in_memory_state();
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let re = regex::Regex::new("help").unwrap();
        chat.on_new_message(re, move |_thread, _msg| {
            let c = c.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let mut msg = dispatched_message("msg-1", false);
        msg.text = "Can someone help me?".to_string();
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "slack:C123:1234.5678", &mut msg),
        )
        .unwrap();
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn on_new_message_pattern_does_not_fire_when_text_does_not_match() {
        // Additive: regex non-match → no handler invocation.
        let (chat, adapter) = chat_with_in_memory_state();
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let re = regex::Regex::new("^!help").unwrap();
        chat.on_new_message(re, move |_thread, _msg| {
            let c = c.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let mut msg = dispatched_message("msg-1", false);
        msg.text = "hello everyone".to_string();
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "slack:C123:1234.5678", &mut msg),
        )
        .unwrap();
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 0);
    }

    #[test]
    fn on_new_message_pattern_fires_when_mention_handler_does_not_match() {
        // 1:1 with upstream "should not trigger onNewMention when
        // message event has no bot mention" — verifies mention
        // handler doesn't fire AND pattern handler does, when text
        // has no mention but matches the pattern.
        let (chat, adapter) = chat_with_in_memory_state();
        let mention_calls = Arc::new(AtomicUsize::new(0));
        let pattern_calls = Arc::new(AtomicUsize::new(0));
        let m = mention_calls.clone();
        chat.on_new_mention(move |_thread, _msg| {
            let c = m.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let p = pattern_calls.clone();
        let re = regex::Regex::new("hello").unwrap();
        chat.on_new_message(re, move |_thread, _msg| {
            let c = p.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let mut msg = dispatched_message("msg-1", false);
        msg.text = "hello everyone".to_string();
        msg.is_mention = Some(false);
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "slack:C123:1234.5678", &mut msg),
        )
        .unwrap();
        assert_eq!(mention_calls.load(AtomicOrdering::SeqCst), 0);
        assert_eq!(pattern_calls.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn on_new_message_pattern_does_not_fire_when_mention_handler_handles_message() {
        // Additive: when is_mention=true, mention handler fires and
        // returns; pattern never reached (upstream's early return).
        let (chat, adapter) = chat_with_in_memory_state();
        let mention_calls = Arc::new(AtomicUsize::new(0));
        let pattern_calls = Arc::new(AtomicUsize::new(0));
        let m = mention_calls.clone();
        chat.on_new_mention(move |_thread, _msg| {
            let c = m.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let p = pattern_calls.clone();
        let re = regex::Regex::new("hello").unwrap();
        chat.on_new_message(re, move |_thread, _msg| {
            let c = p.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let mut msg = dispatched_message("msg-1", false);
        msg.text = "hello bot".to_string();
        msg.is_mention = Some(true);
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "slack:C123:1234.5678", &mut msg),
        )
        .unwrap();
        assert_eq!(mention_calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(pattern_calls.load(AtomicOrdering::SeqCst), 0);
    }

    #[test]
    fn on_new_message_invokes_every_matching_pattern_in_order() {
        // Additive: every pattern whose regex matches the message
        // text fires its handler. Sequential ordering.
        let (chat, adapter) = chat_with_in_memory_state();
        let order: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let o1 = order.clone();
        chat.on_new_message(regex::Regex::new("hello").unwrap(), move |_thread, _msg| {
            let o = o1.clone();
            Box::pin(async move {
                o.lock().unwrap().push(1);
            })
        });
        let o2 = order.clone();
        chat.on_new_message(regex::Regex::new("world").unwrap(), move |_thread, _msg| {
            let o = o2.clone();
            Box::pin(async move {
                o.lock().unwrap().push(2);
            })
        });
        let o3 = order.clone();
        chat.on_new_message(regex::Regex::new("foo").unwrap(), move |_thread, _msg| {
            let o = o3.clone();
            Box::pin(async move {
                o.lock().unwrap().push(3);
            })
        });
        let mut msg = dispatched_message("msg-1", false);
        msg.text = "hello world".to_string();
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "slack:C123:1234.5678", &mut msg),
        )
        .unwrap();
        // patterns 1 + 2 match; 3 ("foo") does not.
        assert_eq!(*order.lock().unwrap(), vec![1, 2]);
    }

    #[test]
    fn on_direct_message_backward_compat_treats_dm_as_mention_when_no_dm_handlers() {
        // 1:1 with upstream's "Backward compat: treat DMs as
        // mentions when no DM handlers registered" branch. DM-shape
        // thread; no DM handlers registered; mention handler should
        // fire (with is_mention=true set by dispatcher even though
        // caller passed is_mention=false).
        let (chat, adapter) = chat_with_dm_adapter();
        let mention_calls = Arc::new(AtomicUsize::new(0));
        let m = mention_calls.clone();
        chat.on_new_mention(move |_thread, _msg| {
            let c = m.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let mut msg = dispatched_message("msg-1", false);
        msg.is_mention = Some(false); // dispatcher overrides to true
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "slack:DU123:1234.5678", &mut msg),
        )
        .unwrap();
        assert_eq!(mention_calls.load(AtomicOrdering::SeqCst), 1);
        // The dispatcher mutated message.is_mention to true.
        assert_eq!(msg.is_mention, Some(true));
    }

    // ---------- describe("Reactions") — slice 419 ----------
    //
    // 1:1 with upstream `chat.test.ts > describe("Reactions")`.
    // Phase E (slice 419) adds the `Chat::on_reaction` +
    // `Chat::on_reaction_filtered` registration surface +
    // `Chat::process_reaction` async dispatcher. The dispatcher
    // skips reactions from the bot itself (event.user.is_me) and
    // fires every registered handler whose filter matches.
    //
    // The handler receives a [`ReactionEvent`] with a Thread bound
    // to the dispatching adapter (matching upstream's behavior).

    fn make_reaction_event(emoji_name: &str, raw_emoji: &str, is_me: bool) -> ReactionEventInput {
        ReactionEventInput {
            emoji: crate::types::EmojiValue::new(emoji_name),
            raw_emoji: raw_emoji.to_string(),
            added: true,
            user: Author {
                user_id: "U123".to_string(),
                user_name: "user".to_string(),
                full_name: "Test User".to_string(),
                is_bot: BotStatus::Known(is_me),
                is_me,
            },
            message_id: "msg-1".to_string(),
            thread_id: "slack:C123:1234.5678".to_string(),
            raw: serde_json::json!({}),
        }
    }

    #[test]
    fn on_reaction_calls_handler_for_all_reactions() {
        // 1:1 with upstream "should call onReaction handler for all
        // reactions". No-filter overload fires for every reaction.
        let (chat, adapter) = chat_with_in_memory_state();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed_emoji: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let c = calls.clone();
        let oe = observed_emoji.clone();
        chat.on_reaction(move |event| {
            let c = c.clone();
            let oe = oe.clone();
            let name = event.emoji.name.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
                *oe.lock().unwrap() = Some(name);
            })
        });
        let event = make_reaction_event("thumbs_up", "+1", false);
        futures_executor::block_on(chat.process_reaction(adapter.as_ref(), event));
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(observed_emoji.lock().unwrap().as_deref(), Some("thumbs_up"));
    }

    #[test]
    fn on_reaction_filtered_calls_handler_for_matching_emoji_only() {
        // 1:1 with upstream "should call onReaction handler for
        // specific emoji". Filter is ["thumbs_up", "heart"]; only
        // thumbs_up matches, fire matches; another emoji (fire)
        // does not.
        let (chat, adapter) = chat_with_in_memory_state();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed_emoji: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let c = calls.clone();
        let oe = observed_emoji.clone();
        chat.on_reaction_filtered(["thumbs_up", "heart"], move |event| {
            let c = c.clone();
            let oe = oe.clone();
            let name = event.emoji.name.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
                *oe.lock().unwrap() = Some(name);
            })
        });
        let thumbs_event = make_reaction_event("thumbs_up", "+1", false);
        let fire_event = make_reaction_event("fire", "fire", false);
        futures_executor::block_on(chat.process_reaction(adapter.as_ref(), thumbs_event));
        futures_executor::block_on(chat.process_reaction(adapter.as_ref(), fire_event));
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(observed_emoji.lock().unwrap().as_deref(), Some("thumbs_up"));
    }

    #[test]
    fn on_reaction_skips_reactions_from_self() {
        // 1:1 with upstream "should skip reactions from self".
        let (chat, adapter) = chat_with_in_memory_state();
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        chat.on_reaction(move |_event| {
            let c = c.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let event = make_reaction_event("thumbs_up", "+1", true); // is_me=true
        futures_executor::block_on(chat.process_reaction(adapter.as_ref(), event));
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 0);
    }

    #[test]
    fn on_reaction_filtered_matches_by_raw_emoji_string() {
        // 1:1 with upstream "should match by rawEmoji when specified
        // in filter". Filter contains "+1" raw string; event with
        // raw_emoji="+1" matches.
        let (chat, adapter) = chat_with_in_memory_state();
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        chat.on_reaction_filtered(["+1"], move |_event| {
            let c = c.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let event = make_reaction_event("thumbs_up", "+1", false);
        futures_executor::block_on(chat.process_reaction(adapter.as_ref(), event));
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn on_reaction_handles_removed_reactions() {
        // 1:1 with upstream "should handle removed reactions". The
        // added=false event reaches the handler with added=false.
        let (chat, adapter) = chat_with_in_memory_state();
        let observed_added: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));
        let oa = observed_added.clone();
        chat.on_reaction(move |event| {
            let oa = oa.clone();
            let added = event.added;
            Box::pin(async move {
                *oa.lock().unwrap() = Some(added);
            })
        });
        let mut event = make_reaction_event("thumbs_up", "+1", false);
        event.added = false;
        futures_executor::block_on(chat.process_reaction(adapter.as_ref(), event));
        assert_eq!(*observed_added.lock().unwrap(), Some(false));
    }

    #[test]
    fn on_reaction_filtered_matches_teams_style_raw_emoji_via_normalized_name() {
        // 1:1 with upstream "should match Teams-style reactions
        // (EmojiValue with string filter)". Filter is an emoji name
        // ("thumbs_up"); event has raw_emoji="like" (Teams native)
        // but emoji.name="thumbs_up" (normalized). The normalized
        // name match should fire the handler.
        let (chat, adapter) = chat_with_in_memory_state();
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        chat.on_reaction_filtered(
            ["thumbs_up", "heart", "fire", "rocket"],
            move |_event| {
                let c = c.clone();
                Box::pin(async move {
                    c.fetch_add(1, AtomicOrdering::SeqCst);
                })
            },
        );
        let event = make_reaction_event("thumbs_up", "like", false);
        futures_executor::block_on(chat.process_reaction(adapter.as_ref(), event));
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn on_reaction_filtered_matches_by_emoji_value() {
        // 1:1 with upstream "should match EmojiValue by object
        // identity" — Rust port uses structural equality on
        // EmojiValue.name (no JS object identity), but same observable
        // contract.
        let (chat, adapter) = chat_with_in_memory_state();
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        chat.on_reaction_filtered(
            [crate::types::EmojiValue::new("thumbs_up")],
            move |_event| {
                let c = c.clone();
                Box::pin(async move {
                    c.fetch_add(1, AtomicOrdering::SeqCst);
                })
            },
        );
        let event = make_reaction_event("thumbs_up", "like", false);
        futures_executor::block_on(chat.process_reaction(adapter.as_ref(), event));
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn on_reaction_event_includes_thread_property() {
        // 1:1 with upstream "should include thread property in
        // ReactionEvent". The dispatcher constructs a Thread from
        // the event.thread_id and binds it to the dispatching
        // adapter; the handler observes thread.thread_id() and
        // adapter_name().
        let (chat, adapter) = chat_with_in_memory_state();
        let observed: Arc<Mutex<Option<(String, String)>>> = Arc::new(Mutex::new(None));
        let o = observed.clone();
        chat.on_reaction(move |event| {
            let o = o.clone();
            let tid = event.thread.thread_id().to_string();
            let name = event.thread.adapter_name().to_string();
            Box::pin(async move {
                *o.lock().unwrap() = Some((tid, name));
            })
        });
        let event = make_reaction_event("thumbs_up", "+1", false);
        futures_executor::block_on(chat.process_reaction(adapter.as_ref(), event));
        let obs = observed.lock().unwrap();
        assert_eq!(
            obs.as_ref().map(|(t, a)| (t.as_str(), a.as_str())),
            Some(("slack:C123:1234.5678", "slack"))
        );
    }

    #[test]
    fn on_reaction_invokes_multiple_handlers_in_order() {
        // Additive: multi-handler dispatch fires in registration
        // order; mix of no-filter and filtered handlers.
        let (chat, adapter) = chat_with_in_memory_state();
        let order: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let o1 = order.clone();
        chat.on_reaction(move |_event| {
            let o = o1.clone();
            Box::pin(async move {
                o.lock().unwrap().push(1);
            })
        });
        let o2 = order.clone();
        chat.on_reaction_filtered(["thumbs_up"], move |_event| {
            let o = o2.clone();
            Box::pin(async move {
                o.lock().unwrap().push(2);
            })
        });
        let o3 = order.clone();
        chat.on_reaction_filtered(["fire"], move |_event| {
            let o = o3.clone();
            Box::pin(async move {
                o.lock().unwrap().push(3);
            })
        });
        let event = make_reaction_event("thumbs_up", "+1", false);
        futures_executor::block_on(chat.process_reaction(adapter.as_ref(), event));
        // handlers 1 and 2 fire; 3 ("fire") does not match
        assert_eq!(*order.lock().unwrap(), vec![1, 2]);
    }

    // ---------- describe("Actions") — slice 420 ----------
    //
    // 1:1 with upstream `chat.test.ts > describe("Actions")`. Phase
    // F (slice 420) mirrors the slice-419 reaction pattern for
    // action events (button clicks, menu selections, etc.).
    //
    // Skips actions from self (event.user.is_me); fires every
    // registered handler whose filter matches; constructs the
    // Thread from event.thread_id bound to the dispatching adapter.

    fn make_action_event(action_id: &str, value: Option<&str>, is_me: bool) -> ActionEventInput {
        ActionEventInput {
            action_id: action_id.to_string(),
            value: value.map(str::to_string),
            user: Author {
                user_id: "U123".to_string(),
                user_name: "user".to_string(),
                full_name: "Test User".to_string(),
                is_bot: BotStatus::Known(is_me),
                is_me,
            },
            message_id: "msg-1".to_string(),
            thread_id: "slack:C123:1234.5678".to_string(),
            raw: serde_json::json!({}),
        }
    }

    #[test]
    fn on_action_calls_handler_for_all_actions() {
        // 1:1 with upstream "should call onAction handler for all
        // actions". No-filter overload fires for every action.
        let (chat, adapter) = chat_with_in_memory_state();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed: Arc<Mutex<Option<(String, Option<String>)>>> = Arc::new(Mutex::new(None));
        let c = calls.clone();
        let o = observed.clone();
        chat.on_action(move |event| {
            let c = c.clone();
            let o = o.clone();
            let id = event.action_id.clone();
            let v = event.value.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
                *o.lock().unwrap() = Some((id, v));
            })
        });
        let event = make_action_event("approve", Some("order-123"), false);
        futures_executor::block_on(chat.process_action(adapter.as_ref(), event));
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 1);
        let obs = observed.lock().unwrap();
        assert_eq!(
            obs.as_ref().map(|(id, v)| (id.as_str(), v.as_deref())),
            Some(("approve", Some("order-123")))
        );
    }

    #[test]
    fn on_action_filtered_calls_handler_for_matching_action_ids_only() {
        // 1:1 with upstream "should call onAction handler for
        // specific action IDs". Filter is ["approve", "reject"];
        // approve matches, skip does not.
        let (chat, adapter) = chat_with_in_memory_state();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let c = calls.clone();
        let o = observed.clone();
        chat.on_action_filtered(["approve", "reject"], move |event| {
            let c = c.clone();
            let o = o.clone();
            let id = event.action_id.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
                *o.lock().unwrap() = Some(id);
            })
        });
        let approve = make_action_event("approve", None, false);
        let skip = make_action_event("skip", None, false);
        futures_executor::block_on(chat.process_action(adapter.as_ref(), approve));
        futures_executor::block_on(chat.process_action(adapter.as_ref(), skip));
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(observed.lock().unwrap().as_deref(), Some("approve"));
    }

    #[test]
    fn on_action_filtered_accepts_single_action_id_string() {
        // 1:1 with upstream "should call onAction handler for
        // single action ID". The IntoIterator-of-String signature
        // accepts both `["approve"]` and (via the impl Iterator
        // for [&str; 1]) a single-element array, matching upstream's
        // `onAction(string, handler)` overload.
        let (chat, adapter) = chat_with_in_memory_state();
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        chat.on_action_filtered(["approve"], move |_event| {
            let c = c.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let event = make_action_event("approve", None, false);
        futures_executor::block_on(chat.process_action(adapter.as_ref(), event));
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn on_action_skips_actions_from_self() {
        // 1:1 with upstream "should skip actions from self".
        let (chat, adapter) = chat_with_in_memory_state();
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        chat.on_action(move |_event| {
            let c = c.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let event = make_action_event("approve", None, true); // is_me=true
        futures_executor::block_on(chat.process_action(adapter.as_ref(), event));
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 0);
    }

    #[test]
    fn on_action_event_includes_thread_property() {
        // 1:1 with upstream "should include thread property in
        // ActionEvent". The dispatcher constructs a Thread from
        // the event.thread_id bound to the dispatching adapter.
        let (chat, adapter) = chat_with_in_memory_state();
        let observed: Arc<Mutex<Option<(String, String)>>> = Arc::new(Mutex::new(None));
        let o = observed.clone();
        chat.on_action(move |event| {
            let o = o.clone();
            let tid = event.thread.thread_id().to_string();
            let name = event.thread.adapter_name().to_string();
            Box::pin(async move {
                *o.lock().unwrap() = Some((tid, name));
            })
        });
        let event = make_action_event("approve", None, false);
        futures_executor::block_on(chat.process_action(adapter.as_ref(), event));
        let obs = observed.lock().unwrap();
        assert_eq!(
            obs.as_ref().map(|(t, a)| (t.as_str(), a.as_str())),
            Some(("slack:C123:1234.5678", "slack"))
        );
    }

    #[test]
    fn on_action_invokes_multiple_handlers_in_order() {
        // Additive: multi-handler dispatch fires in registration
        // order; mix of no-filter and filtered handlers.
        let (chat, adapter) = chat_with_in_memory_state();
        let order: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let o1 = order.clone();
        chat.on_action(move |_event| {
            let o = o1.clone();
            Box::pin(async move {
                o.lock().unwrap().push(1);
            })
        });
        let o2 = order.clone();
        chat.on_action_filtered(["approve"], move |_event| {
            let o = o2.clone();
            Box::pin(async move {
                o.lock().unwrap().push(2);
            })
        });
        let o3 = order.clone();
        chat.on_action_filtered(["reject"], move |_event| {
            let o = o3.clone();
            Box::pin(async move {
                o.lock().unwrap().push(3);
            })
        });
        let event = make_action_event("approve", None, false);
        futures_executor::block_on(chat.process_action(adapter.as_ref(), event));
        // handlers 1 and 2 fire; 3 ("reject") does not match
        assert_eq!(*order.lock().unwrap(), vec![1, 2]);
    }

    // ---------- describe("Slash Commands") — slice 422 ----------
    //
    // 1:1 with upstream `chat.test.ts > describe("Slash Commands")`.
    // Phase G (slice 422) mirrors the slice-419/420 pattern for
    // slash-command events. Filter normalization adds a leading `/`
    // to filter strings without one (matches upstream's "should
    // normalize command names without leading slash" rule).

    fn make_slash_event(command: &str, text: &str, is_me: bool) -> SlashCommandEventInput {
        SlashCommandEventInput {
            command: command.to_string(),
            text: text.to_string(),
            user: Author {
                user_id: "U123".to_string(),
                user_name: "user".to_string(),
                full_name: "Test User".to_string(),
                is_bot: BotStatus::Known(is_me),
                is_me,
            },
            channel_id: "slack:C456".to_string(),
            trigger_id: Some("trigger-123".to_string()),
            raw: serde_json::json!({"channel_id": "C456"}),
        }
    }

    #[test]
    fn on_slash_command_calls_handler_for_all_commands() {
        // 1:1 with upstream "should call onSlashCommand handler for
        // all commands". No-filter overload fires for every command.
        let (chat, adapter) = chat_with_in_memory_state();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed: Arc<Mutex<Option<(String, String)>>> = Arc::new(Mutex::new(None));
        let c = calls.clone();
        let o = observed.clone();
        chat.on_slash_command(move |event| {
            let c = c.clone();
            let o = o.clone();
            let cmd = event.command.clone();
            let text = event.text.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
                *o.lock().unwrap() = Some((cmd, text));
            })
        });
        let event = make_slash_event("/help", "topic", false);
        futures_executor::block_on(chat.process_slash_command(adapter.as_ref(), event));
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 1);
        let obs = observed.lock().unwrap();
        assert_eq!(
            obs.as_ref().map(|(c, t)| (c.as_str(), t.as_str())),
            Some(("/help", "topic"))
        );
    }

    #[test]
    fn on_slash_command_filtered_calls_handler_for_specific_command() {
        // 1:1 with upstream "should call onSlashCommand handler for
        // specific command". Two handlers registered for `/help`
        // and `/status`; the `/help` event fires only the help
        // handler.
        let (chat, adapter) = chat_with_in_memory_state();
        let help_calls = Arc::new(AtomicUsize::new(0));
        let status_calls = Arc::new(AtomicUsize::new(0));
        let h = help_calls.clone();
        chat.on_slash_command_filtered(["/help"], move |_event| {
            let h = h.clone();
            Box::pin(async move {
                h.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let s = status_calls.clone();
        chat.on_slash_command_filtered(["/status"], move |_event| {
            let s = s.clone();
            Box::pin(async move {
                s.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let event = make_slash_event("/help", "", false);
        futures_executor::block_on(chat.process_slash_command(adapter.as_ref(), event));
        assert_eq!(help_calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(status_calls.load(AtomicOrdering::SeqCst), 0);
    }

    #[test]
    fn on_slash_command_filtered_matches_multiple_commands() {
        // 1:1 with upstream "should call onSlashCommand handler for
        // multiple commands". Filter `["/status", "/health"]` matches
        // status + health events but not help.
        let (chat, adapter) = chat_with_in_memory_state();
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        chat.on_slash_command_filtered(["/status", "/health"], move |_event| {
            let c = c.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let status = make_slash_event("/status", "", false);
        let health = make_slash_event("/health", "", false);
        let help = make_slash_event("/help", "", false);
        futures_executor::block_on(chat.process_slash_command(adapter.as_ref(), status));
        futures_executor::block_on(chat.process_slash_command(adapter.as_ref(), health));
        futures_executor::block_on(chat.process_slash_command(adapter.as_ref(), help));
        // Fires for /status and /health, not /help.
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 2);
    }

    #[test]
    fn on_slash_command_skips_commands_from_self() {
        // 1:1 with upstream "should skip slash commands from self".
        let (chat, adapter) = chat_with_in_memory_state();
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        chat.on_slash_command(move |_event| {
            let c = c.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let event = make_slash_event("/help", "", true); // is_me=true
        futures_executor::block_on(chat.process_slash_command(adapter.as_ref(), event));
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 0);
    }

    #[test]
    fn on_slash_command_filter_normalizes_command_names_without_leading_slash() {
        // 1:1 with upstream "should normalize command names without
        // leading slash". Registering with `"help"` matches `/help`
        // events.
        let (chat, adapter) = chat_with_in_memory_state();
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        chat.on_slash_command_filtered(["help"], move |_event| {
            let c = c.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let event = make_slash_event("/help", "", false);
        futures_executor::block_on(chat.process_slash_command(adapter.as_ref(), event));
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn on_slash_command_event_includes_channel_property() {
        // 1:1 with upstream "should call onSlashCommand handler for
        // all commands" — receivedEvent.channel is defined. The
        // dispatcher constructs Channel(adapter, channel_id).
        let (chat, adapter) = chat_with_in_memory_state();
        let observed: Arc<Mutex<Option<(String, String)>>> = Arc::new(Mutex::new(None));
        let o = observed.clone();
        chat.on_slash_command(move |event| {
            let o = o.clone();
            let cid = event.channel.channel_id().to_string();
            let name = event.channel.adapter_name().to_string();
            Box::pin(async move {
                *o.lock().unwrap() = Some((cid, name));
            })
        });
        let event = make_slash_event("/help", "", false);
        futures_executor::block_on(chat.process_slash_command(adapter.as_ref(), event));
        let obs = observed.lock().unwrap();
        assert_eq!(
            obs.as_ref().map(|(c, a)| (c.as_str(), a.as_str())),
            Some(("slack:C456", "slack"))
        );
    }

    #[test]
    fn on_slash_command_invokes_multiple_handlers_in_order() {
        // Additive: multi-handler dispatch fires in registration
        // order; mix of no-filter and filtered handlers.
        let (chat, adapter) = chat_with_in_memory_state();
        let order: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let o1 = order.clone();
        chat.on_slash_command(move |_event| {
            let o = o1.clone();
            Box::pin(async move {
                o.lock().unwrap().push(1);
            })
        });
        let o2 = order.clone();
        chat.on_slash_command_filtered(["/help"], move |_event| {
            let o = o2.clone();
            Box::pin(async move {
                o.lock().unwrap().push(2);
            })
        });
        let o3 = order.clone();
        chat.on_slash_command_filtered(["/status"], move |_event| {
            let o = o3.clone();
            Box::pin(async move {
                o.lock().unwrap().push(3);
            })
        });
        let event = make_slash_event("/help", "", false);
        futures_executor::block_on(chat.process_slash_command(adapter.as_ref(), event));
        // handlers 1 and 2 fire; 3 ("/status") does not match
        assert_eq!(*order.lock().unwrap(), vec![1, 2]);
    }

    // ---------- describe("Options Load") — slice 423 ----------
    //
    // 1:1 with upstream `chat.test.ts > describe("Options Load")`.
    // Phase H (slice 423). Unlike the other handler classes,
    // process_options_load RETURNS the options payload (it's a
    // request/response model, not fire-and-forget). Dispatch:
    // specific handlers first, then catch-all on no-specific-match,
    // continuing past errors per upstream.

    fn make_options_event(action_id: &str, query: &str) -> OptionsLoadEvent {
        OptionsLoadEvent {
            action_id: action_id.to_string(),
            query: query.to_string(),
            user: Author {
                user_id: "U123".to_string(),
                user_name: "user".to_string(),
                full_name: "Test User".to_string(),
                is_bot: BotStatus::Known(false),
                is_me: false,
            },
            raw: serde_json::json!({}),
        }
    }

    #[test]
    fn on_options_load_calls_handler_for_matching_action_id() {
        // 1:1 with upstream "should call onOptionsLoad handler for
        // a matching action ID". Specific handler returns the
        // options payload; dispatcher relays it as the result.
        let (chat, adapter) = chat_with_in_memory_state();
        let observed: Arc<Mutex<Option<(String, String)>>> = Arc::new(Mutex::new(None));
        let o = observed.clone();
        chat.on_options_load_filtered(["person_select"], move |event| {
            let o = o.clone();
            let aid = event.action_id.clone();
            let q = event.query.clone();
            Box::pin(async move {
                *o.lock().unwrap() = Some((aid, q));
                Ok(serde_json::json!([
                    {"label": "Maria Garcia", "value": "person_123"}
                ]))
            })
        });
        let event = make_options_event("person_select", "mar");
        let result =
            futures_executor::block_on(chat.process_options_load(adapter.as_ref(), event));
        let obs = observed.lock().unwrap();
        assert_eq!(
            obs.as_ref().map(|(a, q)| (a.as_str(), q.as_str())),
            Some(("person_select", "mar"))
        );
        assert_eq!(
            result.unwrap(),
            serde_json::json!([{"label": "Maria Garcia", "value": "person_123"}])
        );
    }

    #[test]
    fn on_options_load_prefers_specific_handlers_before_catch_all() {
        // 1:1 with upstream "should prefer specific handlers before
        // catch-all handlers". Both registered (catch-all FIRST in
        // registration); specific runs, catch-all does NOT.
        let (chat, adapter) = chat_with_in_memory_state();
        let catchall_calls = Arc::new(AtomicUsize::new(0));
        let specific_calls = Arc::new(AtomicUsize::new(0));
        let ca = catchall_calls.clone();
        chat.on_options_load(move |_event| {
            let ca = ca.clone();
            Box::pin(async move {
                ca.fetch_add(1, AtomicOrdering::SeqCst);
                Ok(serde_json::json!([{"label": "Fallback", "value": "fallback"}]))
            })
        });
        let sp = specific_calls.clone();
        chat.on_options_load_filtered(["person_select"], move |_event| {
            let sp = sp.clone();
            Box::pin(async move {
                sp.fetch_add(1, AtomicOrdering::SeqCst);
                Ok(serde_json::json!([{"label": "Specific", "value": "specific"}]))
            })
        });
        let event = make_options_event("person_select", "mar");
        let result =
            futures_executor::block_on(chat.process_options_load(adapter.as_ref(), event));
        assert_eq!(specific_calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(catchall_calls.load(AtomicOrdering::SeqCst), 0);
        assert_eq!(
            result.unwrap(),
            serde_json::json!([{"label": "Specific", "value": "specific"}])
        );
    }

    #[test]
    fn on_options_load_falls_back_to_catch_all_when_no_specific_match() {
        // 1:1 with upstream "should fall back to catch-all handlers
        // when no specific handler matches". Only catch-all
        // registered; event with unknown action_id fires it.
        let (chat, adapter) = chat_with_in_memory_state();
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        chat.on_options_load(move |_event| {
            let c = c.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
                Ok(serde_json::json!([{"label": "Fallback", "value": "fallback"}]))
            })
        });
        let event = make_options_event("unknown_select", "test");
        let result =
            futures_executor::block_on(chat.process_options_load(adapter.as_ref(), event));
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(
            result.unwrap(),
            serde_json::json!([{"label": "Fallback", "value": "fallback"}])
        );
    }

    #[test]
    fn on_options_load_continues_after_handler_errors() {
        // 1:1 with upstream "should continue after handler errors".
        // First specific handler errors; second handler (catch-all
        // in this test) provides the fallback result.
        let (chat, adapter) = chat_with_in_memory_state();
        let failing_calls = Arc::new(AtomicUsize::new(0));
        let fallback_calls = Arc::new(AtomicUsize::new(0));
        let f = failing_calls.clone();
        chat.on_options_load_filtered(["person_select"], move |_event| {
            let f = f.clone();
            Box::pin(async move {
                f.fetch_add(1, AtomicOrdering::SeqCst);
                Err::<serde_json::Value, _>(Box::new(std::io::Error::other("boom"))
                    as Box<dyn std::error::Error + Send + Sync>)
            })
        });
        let fb = fallback_calls.clone();
        chat.on_options_load(move |_event| {
            let fb = fb.clone();
            Box::pin(async move {
                fb.fetch_add(1, AtomicOrdering::SeqCst);
                Ok(serde_json::json!([{"label": "Recovered", "value": "recovered"}]))
            })
        });
        let event = make_options_event("person_select", "mar");
        let result =
            futures_executor::block_on(chat.process_options_load(adapter.as_ref(), event));
        assert_eq!(failing_calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(fallback_calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(
            result.unwrap(),
            serde_json::json!([{"label": "Recovered", "value": "recovered"}])
        );
    }

    #[test]
    fn on_options_load_supports_returning_option_groups() {
        // 1:1 with upstream "should support returning option groups".
        // Rust port uses serde_json::Value for the return type so
        // both flat options and grouped options work without a
        // distinct type union.
        let (chat, adapter) = chat_with_in_memory_state();
        chat.on_options_load_filtered(["user_select"], move |_event| {
            Box::pin(async move {
                Ok(serde_json::json!([
                    {"label": "Recent", "options": [{"label": "Alice", "value": "u1"}]},
                    {"label": "All", "options": [
                        {"label": "Bob", "value": "u2"},
                        {"label": "Carol", "value": "u3"}
                    ]}
                ]))
            })
        });
        let event = make_options_event("user_select", "");
        let result =
            futures_executor::block_on(chat.process_options_load(adapter.as_ref(), event))
                .unwrap();
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["label"], "Recent");
    }

    #[test]
    fn on_options_load_returns_none_when_no_handler_matches() {
        // Additive: when no specific or catch-all handler matches /
        // succeeds, process_options_load returns None.
        let (chat, adapter) = chat_with_in_memory_state();
        chat.on_options_load_filtered(["specific_only"], move |_event| {
            Box::pin(async move {
                Ok(serde_json::json!([{"label": "X", "value": "x"}]))
            })
        });
        let event = make_options_event("unknown_select", "test");
        let result =
            futures_executor::block_on(chat.process_options_load(adapter.as_ref(), event));
        assert!(result.is_none());
    }

    #[test]
    fn on_new_mention_handler_does_not_fire_for_self_messages() {
        // 1:1 with the existing skip-self early-exit covering the
        // mention dispatch path — even if is_mention=true, an
        // author.is_me message short-circuits before handlers run.
        let (chat, adapter) = chat_with_in_memory_state();
        let invocations = Arc::new(AtomicUsize::new(0));
        let counter = invocations.clone();
        chat.on_new_mention(move |_thread, _msg| {
            let c = counter.clone();
            Box::pin(async move {
                c.fetch_add(1, AtomicOrdering::SeqCst);
            })
        });
        let mut msg = dispatched_message("msg-self", true);
        msg.is_mention = Some(true);
        futures_executor::block_on(
            chat.handle_incoming_message(adapter.as_ref(), "slack:C123:1234.5678", &mut msg),
        )
        .unwrap();
        assert_eq!(invocations.load(AtomicOrdering::SeqCst), 0);
    }
}
