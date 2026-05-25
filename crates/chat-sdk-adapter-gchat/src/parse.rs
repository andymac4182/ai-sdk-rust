//! Pure parsing/normalization helpers for the Google Chat adapter.
//!
//! 1:1 port (helper extraction) of pure logic embedded inside upstream
//! `packages/adapter-gchat/src/index.ts`:
//!
//! - `createAttachment` MIME -> attachment-type classification
//!   (`image` / `video` / `audio` / `file`).
//! - `parseGoogleChatMessage` direct-webhook bot-mention rewrite +
//!   bot-user-id learning + sender self-detection.
//! - `parsePubSubMessage` displayName fallback (`User <numeric>` /
//!   provided / cached / botName).
//! - `handleMessageEvent` DM-vs-room thread-name selection.
//! - `handleCardClick` actionId / value selection + form-input lookup.
//! - `handleMessageEvent` `isDM` detection (`type == "DM"` or
//!   `spaceType == "DIRECT_MESSAGE"`).
//! - `fetchChannelMessages` thread-root filter.
//! - `handlePubSubMessage` event-type allowlist.
//! - `handleGoogleChatError` 429 -> RateLimit classification.
//!
//! The HTTP / runtime surfaces (vi.fn() mocks for chatApi /
//! workspaceevents / Request shapes) are not portable; the helpers
//! here cover the pure data-shape behavior the runtime path flows
//! through.

use std::collections::HashMap;

/// Attachment type after MIME classification. 1:1 with upstream
/// `Attachment["type"]` discriminant in `createAttachment`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GchatAttachmentType {
    /// `image/*` content types.
    Image,
    /// `video/*` content types.
    Video,
    /// `audio/*` content types.
    Audio,
    /// Everything else (including absent content types).
    File,
}

impl GchatAttachmentType {
    /// Stable lowercase string discriminator matching upstream's
    /// literal-union values.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::File => "file",
        }
    }
}

/// Classify a Google Chat attachment by MIME type. 1:1 with upstream
/// `createAttachment` MIME branch:
///
/// ```text
/// if (att.contentType?.startsWith("image/")) type = "image";
/// else if (att.contentType?.startsWith("video/")) type = "video";
/// else if (att.contentType?.startsWith("audio/")) type = "audio";
/// else type = "file";
/// ```
///
/// `None` / unknown content types fall through to [`GchatAttachmentType::File`].
pub fn classify_attachment_type(content_type: Option<&str>) -> GchatAttachmentType {
    match content_type {
        Some(ct) if ct.starts_with("image/") => GchatAttachmentType::Image,
        Some(ct) if ct.starts_with("video/") => GchatAttachmentType::Video,
        Some(ct) if ct.starts_with("audio/") => GchatAttachmentType::Audio,
        _ => GchatAttachmentType::File,
    }
}

/// Predicate: is the space a DM. 1:1 with upstream `handleMessageEvent`
/// inline check `messagePayload.space.type === "DM" ||
/// messagePayload.space.spaceType === "DIRECT_MESSAGE"`. Either field
/// may be present depending on the webhook variant.
pub fn is_dm_space(space_type: Option<&str>, space_type_alt: Option<&str>) -> bool {
    space_type == Some("DM") || space_type_alt == Some("DIRECT_MESSAGE")
}

/// Pick the `threadName` for the encoded thread id. 1:1 with upstream
/// `handleMessageEvent`:
///
/// ```text
/// const threadName = isDM ? undefined : message.thread?.name || message.name;
/// ```
///
/// For DMs the thread name is dropped so all DM messages share the
/// space-only thread id (matches the DM subscription).
pub fn select_event_thread_name<'a>(
    is_dm: bool,
    message_thread_name: Option<&'a str>,
    message_name: Option<&'a str>,
) -> Option<&'a str> {
    if is_dm {
        return None;
    }
    match message_thread_name.filter(|s| !s.is_empty()) {
        Some(n) => Some(n),
        None => message_name.filter(|s| !s.is_empty()),
    }
}

/// Predicate: is a message from this bot. 1:1 with upstream
/// `isMessageFromSelf`. When `bot_user_id` is known and `sender_id` is
/// present, exact match wins. Otherwise upstream returns `false`
/// (logging at debug); the Rust port returns `false` too.
pub fn is_message_from_self(bot_user_id: Option<&str>, sender_id: Option<&str>) -> bool {
    match (bot_user_id, sender_id) {
        (Some(bot), Some(sender)) => bot == sender,
        _ => false,
    }
}

/// Resolve `actionId` for a card click. 1:1 with upstream
/// `handleCardClick`:
///
/// ```text
/// const actionId = commonEvent?.parameters?.actionId || commonEvent?.invokedFunction;
/// ```
///
/// `parameters.actionId` wins when present and non-empty; otherwise
/// `invokedFunction` is the fallback. Returns `None` when neither has
/// a usable value.
pub fn resolve_card_action_id<'a>(
    parameters_action_id: Option<&'a str>,
    invoked_function: Option<&'a str>,
) -> Option<&'a str> {
    parameters_action_id
        .filter(|s| !s.is_empty())
        .or_else(|| invoked_function.filter(|s| !s.is_empty()))
}

/// Read a form-input string value. 1:1 with upstream
/// `getFormInputValue(formInputs, actionId)`:
///
/// ```text
/// return formInputs?.[actionId]?.stringInputs?.value?.[0];
/// ```
pub fn get_form_input_value<'a>(
    form_inputs: Option<&'a HashMap<String, Vec<String>>>,
    action_id: &str,
) -> Option<&'a str> {
    form_inputs?.get(action_id)?.first().map(String::as_str)
}

/// Resolve the `value` field for a card click. 1:1 with upstream
/// `handleCardClick`:
///
/// ```text
/// const value = commonEvent?.parameters?.value
///   ?? this.getFormInputValue(commonEvent?.formInputs, actionId);
/// ```
///
/// `parameters.value` wins (note: nullish coalesce, so empty string
/// is preserved); otherwise the first formInputs string for
/// `action_id`.
pub fn resolve_card_value<'a>(
    parameters_value: Option<&'a str>,
    form_inputs: Option<&'a HashMap<String, Vec<String>>>,
    action_id: &str,
) -> Option<&'a str> {
    parameters_value.or_else(|| get_form_input_value(form_inputs, action_id))
}

/// Resolve a display name fallback for a Google Chat user. 1:1 with
/// the empty-cache branch of upstream `userInfoCache.resolveDisplayName`:
///
/// ```text
/// return `User ${userId.replace(/^users\//, "")}`;
/// ```
pub fn fallback_display_name(user_id: &str) -> String {
    let suffix = user_id.strip_prefix("users/").unwrap_or(user_id);
    format!("User {suffix}")
}

/// Predicate: should a Pub/Sub Workspace Event be processed. 1:1 with
/// upstream `handlePubSubMessage` event-type allowlist (the set of
/// `ce-type` values the adapter routes; any other type is ignored
/// with a 200 to avoid Pub/Sub retries).
pub fn is_supported_pubsub_event_type(event_type: &str) -> bool {
    matches!(
        event_type,
        "google.workspace.chat.message.v1.created"
            | "google.workspace.chat.reaction.v1.created"
            | "google.workspace.chat.reaction.v1.deleted"
            | "google.workspace.chat.space.v1.updated"
            | "google.workspace.chat.membership.v1.created"
            | "google.workspace.chat.membership.v1.deleted"
    )
}

/// Predicate: is a Google Chat message the root of its thread. 1:1
/// with upstream `isThreadRoot(msg)`:
///
/// Google encodes thread membership as a sortable message name where
/// the root's name suffix equals the thread name's suffix. E.g.
/// `spaces/S1/messages/ABC.ABC` is the root of thread
/// `spaces/S1/threads/ABC`; `spaces/S1/messages/ABC.DEF` is a reply
/// inside the same thread. Messages with no `thread.name` are treated
/// as top-level.
pub fn is_thread_root(message_name: Option<&str>, thread_name: Option<&str>) -> bool {
    let Some(msg) = message_name else { return true };
    let Some(thread) = thread_name else {
        return true;
    };
    let msg_suffix = msg.rsplit('/').next().unwrap_or("");
    let thread_suffix = thread.rsplit('/').next().unwrap_or("");
    if thread_suffix.is_empty() {
        return true;
    }
    // Upstream parses `<thread>.<reply>` form; the root has matching
    // halves.
    let mut parts = msg_suffix.splitn(2, '.');
    let first = parts.next().unwrap_or("");
    let second = parts.next();
    match second {
        Some(second_part) => first == thread_suffix && second_part == thread_suffix,
        None => first == thread_suffix,
    }
}

/// Classify a Google Chat API error. 1:1 with upstream
/// `handleGoogleChatError(error, context)`: 429 -> `AdapterRateLimitError`;
/// every other code rethrows the original error. The Rust port models
/// this as a typed enum so callers can `match` on it. The `context`
/// arg upstream is logger-only and js-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GchatErrorKind {
    /// HTTP 429. 1:1 with upstream `throw new AdapterRateLimitError("gchat")`.
    RateLimit,
    /// Any other code is rethrown as-is. 1:1 with upstream's `throw error`.
    Rethrow,
}

/// Pure classification of a Google Chat API error code. 1:1 with
/// upstream's `handleGoogleChatError` branch (429 -> RateLimit).
pub fn classify_gchat_error(code: Option<u16>) -> GchatErrorKind {
    if code == Some(429) {
        GchatErrorKind::RateLimit
    } else {
        GchatErrorKind::Rethrow
    }
}

/// Replace a bot mention in `text` using upstream's annotation-driven
/// substring rewrite. 1:1 with the `startIndex`/`length` branch of
/// `normalizeBotMentions`:
///
/// ```text
/// text = text.slice(0, startIndex) + `@${userName}` + text.slice(startIndex + length);
/// ```
///
/// Returns the modified text. Indexes are byte offsets into `text`
/// (matches Google Chat's API which uses UTF-16 code units; for the
/// pure-ASCII fixtures upstream tests use, byte and UTF-16 offsets
/// coincide). Out-of-range indexes return `text` unchanged.
pub fn replace_bot_mention_by_index(
    text: &str,
    start_index: usize,
    length: usize,
    user_name: &str,
) -> String {
    let end = start_index.saturating_add(length);
    if end > text.len() || !text.is_char_boundary(start_index) || !text.is_char_boundary(end) {
        return text.to_string();
    }
    let head = &text[..start_index];
    let tail = &text[end..];
    format!("{head}@{user_name}{tail}")
}

/// Fall back to display-name-based replacement. 1:1 with the
/// `else if (botDisplayName)` branch of upstream's
/// `normalizeBotMentions`:
///
/// ```text
/// text = text.replace(`@${botDisplayName}`, `@${userName}`);
/// ```
///
/// Note upstream uses string `replace` (first occurrence only),
/// not `replaceAll`.
pub fn replace_bot_mention_by_display_name(
    text: &str,
    bot_display_name: &str,
    user_name: &str,
) -> String {
    let needle = format!("@{bot_display_name}");
    let replacement = format!("@{user_name}");
    match text.find(&needle) {
        Some(idx) => {
            let mut out = String::with_capacity(text.len() + replacement.len());
            out.push_str(&text[..idx]);
            out.push_str(&replacement);
            out.push_str(&text[idx + needle.len()..]);
            out
        }
        None => text.to_string(),
    }
}

/// Should the adapter learn (persist) a bot's user id from a USER_MENTION
/// annotation. 1:1 with the `if (botUser.name && !this.botUserId)`
/// guard in `normalizeBotMentions`: only learn when the annotation
/// carries a sender name *and* the adapter doesn't already have one.
pub fn should_learn_bot_user_id(
    current_bot_user_id: Option<&str>,
    mention_user_name: Option<&str>,
) -> bool {
    current_bot_user_id.is_none() && mention_user_name.is_some_and(|n| !n.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- describe("parseMessage") > attachment classification ----------
    // 1:1 with upstream `index.test.ts > describe("parseMessage")` cases:
    // L487 "should include attachments in parsed message" (image/png -> image)
    // L507 "should classify video and audio attachment types" (video, audio)

    #[test]
    fn classify_attachment_should_include_attachments_in_parsed_message() {
        // 1:1 with upstream "should include attachments in parsed message".
        // image/png -> image.
        assert_eq!(
            classify_attachment_type(Some("image/png")),
            GchatAttachmentType::Image
        );
        assert_eq!(GchatAttachmentType::Image.as_str(), "image");
    }

    #[test]
    fn classify_attachment_should_classify_video_and_audio_types() {
        // 1:1 with upstream "should classify video and audio attachment types".
        assert_eq!(
            classify_attachment_type(Some("video/mp4")),
            GchatAttachmentType::Video
        );
        assert_eq!(
            classify_attachment_type(Some("audio/mpeg")),
            GchatAttachmentType::Audio
        );
    }

    #[test]
    fn classify_attachment_falls_back_to_file_for_unknown_or_absent_types() {
        // 1:1 with the `else type = "file"` branch of upstream's
        // `createAttachment` (covered indirectly by upstream's
        // "should not provide fetchData when neither resourceName nor
        // downloadUri exist" case which uses application/octet-stream).
        assert_eq!(
            classify_attachment_type(Some("application/octet-stream")),
            GchatAttachmentType::File
        );
        assert_eq!(
            classify_attachment_type(Some("text/plain")),
            GchatAttachmentType::File
        );
        assert_eq!(classify_attachment_type(None), GchatAttachmentType::File);
    }

    #[test]
    fn classify_attachment_includes_attachments_from_pubsub_messages() {
        // 1:1 with upstream `index.test.ts > describe("parsePubSubMessage") >
        // it("should include attachments from Pub/Sub messages")` (L1395):
        // application/pdf -> file.
        assert_eq!(
            classify_attachment_type(Some("application/pdf")),
            GchatAttachmentType::File
        );
    }

    // ---------- describe("handleMessageEvent") DM-vs-room thread id ----------
    // 1:1 with upstream `index.test.ts > describe("handleMessageEvent")`
    // cases L1236/L1256.

    #[test]
    fn handle_message_event_uses_space_only_thread_id_for_dm_messages() {
        // 1:1 with upstream "should use space-only thread ID for DM messages":
        // DM spaces drop the thread name entirely so the thread id ends
        // with :dm (no encoded thread suffix).
        let is_dm = is_dm_space(Some("DM"), None);
        assert!(is_dm);
        let chosen = select_event_thread_name(
            is_dm,
            Some("spaces/DM_SPACE/threads/thread1"),
            Some("spaces/DM_SPACE/messages/m1"),
        );
        assert_eq!(chosen, None);
    }

    #[test]
    fn handle_message_event_uses_spacetype_alias_direct_message() {
        // Pub/Sub messages carry `spaceType` instead of `type`.
        let is_dm = is_dm_space(None, Some("DIRECT_MESSAGE"));
        assert!(is_dm);
    }

    #[test]
    fn handle_message_event_includes_thread_name_for_room_messages() {
        // 1:1 with upstream "should include thread name for room messages":
        // room spaces keep the thread.name as-is.
        let is_dm = is_dm_space(Some("ROOM"), None);
        assert!(!is_dm);
        let chosen = select_event_thread_name(
            is_dm,
            Some("spaces/ABC123/threads/XYZ"),
            Some("spaces/ABC123/messages/m1"),
        );
        assert_eq!(chosen, Some("spaces/ABC123/threads/XYZ"));
    }

    #[test]
    fn handle_message_event_falls_back_to_message_name_when_no_thread() {
        // 1:1 with upstream's `message.thread?.name || message.name`
        // fallback when the message is the thread root.
        let chosen = select_event_thread_name(false, None, Some("spaces/ABC/messages/root"));
        assert_eq!(chosen, Some("spaces/ABC/messages/root"));
    }

    // ---------- describe("isMessageFromSelf (via parseMessage)") ----------
    // 1:1 with upstream `index.test.ts > describe("isMessageFromSelf
    // (via parseMessage)")` (3 cases).

    #[test]
    fn is_message_from_self_detects_self_when_bot_user_id_is_known() {
        // 1:1 with upstream "should detect self messages when botUserId is known".
        assert!(is_message_from_self(
            Some("users/BOT123"),
            Some("users/BOT123")
        ));
    }

    #[test]
    fn is_message_from_self_does_not_mark_other_bots_as_self() {
        // 1:1 with upstream "should not mark other bots as self".
        assert!(!is_message_from_self(
            Some("users/BOT123"),
            Some("users/OTHER_BOT")
        ));
    }

    #[test]
    fn is_message_from_self_returns_false_when_bot_user_id_is_unknown() {
        // 1:1 with upstream "should return false when botUserId is unknown".
        assert!(!is_message_from_self(None, Some("users/ANY")));
    }

    // ---------- describe("handleCardClick (via handleWebhook)") ----------
    // 1:1 with upstream cases L1031, L1065, L1102, L1151.

    #[test]
    fn handle_card_click_ignores_when_missing_actionid() {
        // 1:1 with upstream "should ignore card click when missing actionId".
        let resolved = resolve_card_action_id(None, None);
        assert!(resolved.is_none());
    }

    #[test]
    fn handle_card_click_uses_invoked_function_as_actionid() {
        // 1:1 with upstream "should use invokedFunction as actionId":
        // no parameters.actionId, falls back to invokedFunction.
        let resolved = resolve_card_action_id(None, Some("handleApprove"));
        assert_eq!(resolved, Some("handleApprove"));
    }

    #[test]
    fn handle_card_click_prefers_parameters_actionid_over_invoked_function() {
        // Additive Rust coverage of upstream's `||` precedence
        // (parameters.actionId wins when present).
        let resolved = resolve_card_action_id(Some("paramAction"), Some("invokedFn"));
        assert_eq!(resolved, Some("paramAction"));
    }

    #[test]
    fn handle_card_click_reads_selection_values_from_form_inputs_when_value_is_missing() {
        // 1:1 with upstream "should read selection values from formInputs
        // when parameters.value is missing".
        let mut inputs = HashMap::new();
        inputs.insert("selection".to_string(), vec!["option-1".to_string()]);
        let resolved = resolve_card_value(None, Some(&inputs), "selection");
        assert_eq!(resolved, Some("option-1"));
    }

    #[test]
    fn handle_card_click_prefers_parameters_value_over_form_inputs() {
        // 1:1 with upstream "should prefer parameters.value when both
        // parameters and formInputs are present".
        let mut inputs = HashMap::new();
        inputs.insert("selection".to_string(), vec!["dropdown-value".to_string()]);
        let resolved = resolve_card_value(Some("button-value"), Some(&inputs), "selection");
        assert_eq!(resolved, Some("button-value"));
    }

    #[test]
    fn get_form_input_value_returns_none_when_action_id_absent() {
        // 1:1 with upstream `formInputs?.[actionId]?.stringInputs?.value?.[0]`
        // optional chain returning undefined when actionId is missing.
        let inputs = HashMap::new();
        assert!(get_form_input_value(Some(&inputs), "missing").is_none());
        assert!(get_form_input_value(None, "any").is_none());
    }

    // ---------- describe("user info caching") fallback display name ----------
    // 1:1 with upstream `index.test.ts > describe("user info caching") >
    // it("should fall back to User ID when cache miss")` (L2791).

    #[test]
    fn user_info_caching_falls_back_to_user_id_when_cache_miss() {
        // 1:1 with upstream "should fall back to User ID when cache miss".
        assert_eq!(fallback_display_name("users/987654321"), "User 987654321");
    }

    #[test]
    fn user_info_caching_fallback_handles_non_prefixed_ids() {
        // Additive: the prefix-strip is `replace(/^users\//, "")`.
        assert_eq!(fallback_display_name("BOT_ID"), "User BOT_ID");
    }

    // ---------- describe("handleWebhook") Pub/Sub event-type allowlist ----------
    // 1:1 with upstream `index.test.ts > describe("handleWebhook") >
    // it("should skip unsupported Pub/Sub event types")` (L942):
    // `google.workspace.chat.message.v1.updated` is not in the allowed
    // list and should be skipped.

    #[test]
    fn handle_webhook_skips_unsupported_pubsub_event_types() {
        // 1:1 with upstream "should skip unsupported Pub/Sub event types".
        assert!(!is_supported_pubsub_event_type(
            "google.workspace.chat.message.v1.updated"
        ));
    }

    #[test]
    fn handle_webhook_routes_supported_pubsub_event_types() {
        // 1:1 with upstream `describe("Pub/Sub message handling")` cases
        // L1277/L1313 (reaction.created / reaction.deleted) and the
        // L917 "should route Pub/Sub push messages" case
        // (message.v1.created).
        assert!(is_supported_pubsub_event_type(
            "google.workspace.chat.message.v1.created"
        ));
        assert!(is_supported_pubsub_event_type(
            "google.workspace.chat.reaction.v1.created"
        ));
        assert!(is_supported_pubsub_event_type(
            "google.workspace.chat.reaction.v1.deleted"
        ));
    }

    // ---------- describe("fetchChannelMessages") thread-root filter ----------
    // 1:1 with upstream cases L2387 + L2434.

    #[test]
    fn fetch_channel_messages_filters_to_thread_roots_only_backward() {
        // 1:1 with upstream "should filter to thread roots only (backward)":
        // ABC.ABC matches threads/ABC -> root; ABC.DEF doesn't -> reply.
        assert!(is_thread_root(
            Some("spaces/S1/messages/ABC.ABC"),
            Some("spaces/S1/threads/ABC")
        ));
        assert!(!is_thread_root(
            Some("spaces/S1/messages/ABC.DEF"),
            Some("spaces/S1/threads/ABC")
        ));
        assert!(is_thread_root(
            Some("spaces/S1/messages/XYZ.XYZ"),
            Some("spaces/S1/threads/XYZ")
        ));
    }

    #[test]
    fn fetch_channel_messages_handles_messages_without_thread_info_as_top_level() {
        // 1:1 with upstream "should handle messages without thread info
        // as top-level": no thread name -> treated as root.
        assert!(is_thread_root(Some("spaces/S1/messages/simple"), None));
    }

    // ---------- describe("handleGoogleChatError") ----------
    // 1:1 with upstream cases L1724 + L1738.

    #[test]
    fn handle_google_chat_error_throws_rate_limit_for_429() {
        // 1:1 with upstream "should throw AdapterRateLimitError for 429".
        assert_eq!(classify_gchat_error(Some(429)), GchatErrorKind::RateLimit);
    }

    #[test]
    fn handle_google_chat_error_rethrows_for_non_429() {
        // 1:1 with upstream "should rethrow the original error for non-429 codes".
        assert_eq!(classify_gchat_error(Some(500)), GchatErrorKind::Rethrow);
        assert_eq!(classify_gchat_error(Some(404)), GchatErrorKind::Rethrow);
        assert_eq!(classify_gchat_error(Some(403)), GchatErrorKind::Rethrow);
        assert_eq!(classify_gchat_error(None), GchatErrorKind::Rethrow);
    }

    // ---------- describe("normalizeBotMentions (via parseMessage)") ----------
    // 1:1 with upstream cases L637 / L664 / L693.

    #[test]
    fn normalize_bot_mentions_replaces_bot_mention_with_adapter_user_name() {
        // 1:1 with upstream "should replace bot mention with adapter
        // userName": annotation startIndex=0, length=14, text="@Chat SDK
        // Demo hello" -> "@mybot hello".
        let result = replace_bot_mention_by_index("@Chat SDK Demo hello", 0, 14, "mybot");
        assert_eq!(result, "@mybot hello");
        assert!(result.contains("@mybot"));
        assert!(!result.contains("@Chat SDK Demo"));
    }

    #[test]
    fn normalize_bot_mentions_falls_back_to_display_name_replace() {
        // 1:1 with the `else if (botDisplayName)` branch of upstream's
        // `normalizeBotMentions` (covers direct-webhook events where
        // startIndex/length aren't carried).
        let result = replace_bot_mention_by_display_name("@MyBot says hi", "MyBot", "bot");
        assert_eq!(result, "@bot says hi");
    }

    #[test]
    fn normalize_bot_mentions_learns_bot_user_id_from_annotations() {
        // 1:1 with upstream "should learn bot user ID from annotations":
        // no prior id + annotation carries a user name -> learn.
        assert!(should_learn_bot_user_id(None, Some("users/LEARNED_BOT_ID")));
    }

    #[test]
    fn normalize_bot_mentions_does_not_overwrite_bot_user_id_once_learned() {
        // 1:1 with upstream "should not overwrite botUserId once learned":
        // prior id present -> do not learn again.
        assert!(!should_learn_bot_user_id(
            Some("users/FIRST_BOT"),
            Some("users/SECOND_BOT")
        ));
    }

    #[test]
    fn normalize_bot_mentions_does_not_learn_without_a_name() {
        // Guard: empty / missing mention user name doesn't trigger learn.
        assert!(!should_learn_bot_user_id(None, None));
        assert!(!should_learn_bot_user_id(None, Some("")));
    }

    #[test]
    fn replace_bot_mention_by_index_is_a_noop_when_indexes_out_of_range() {
        // Guard: out-of-range slice returns text unchanged (matches
        // upstream's defensive behavior where invalid annotations are
        // skipped without crashing).
        let result = replace_bot_mention_by_index("short", 100, 5, "bot");
        assert_eq!(result, "short");
    }
}
