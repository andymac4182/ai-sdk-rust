//! Emoji conversion utilities and singleton emoji values.
//!
//! 1:1 port of `packages/chat/src/emoji.ts`. The upstream module keeps
//! an internal `Map<string, EmojiValue>` registry so that
//! `getEmoji("thumbs_up") === getEmoji("thumbs_up")` always holds
//! (object identity). The Rust port models that by returning
//! `Arc<EmojiValue>` from [`get_emoji`]; identity is then checked via
//! `Arc::ptr_eq`. The cloned `EmojiValue` content is also
//! value-equivalent across calls (`==`), which is the more common
//! comparison Rust callers will reach for.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

use crate::types::{EmojiFormats, EmojiValue, StringOrList};

/// Default emoji map covering all well-known emoji shortcodes. 1:1
/// port of upstream `DEFAULT_EMOJI_MAP`.
///
/// Lazy-initialized; iteration order is not guaranteed (HashMap)
/// because no upstream caller depends on it.
pub static DEFAULT_EMOJI_MAP: LazyLock<HashMap<&'static str, EmojiFormats>> =
    LazyLock::new(build_default_emoji_map);

fn s(s: &str) -> StringOrList {
    StringOrList::One(s.to_string())
}
fn m(items: &[&str]) -> StringOrList {
    StringOrList::Many(items.iter().map(|x| x.to_string()).collect())
}
fn ef(slack: StringOrList, gchat: StringOrList) -> EmojiFormats {
    EmojiFormats { slack, gchat }
}

#[rustfmt::skip]
fn build_default_emoji_map() -> HashMap<&'static str, EmojiFormats> {
    let mut map: HashMap<&'static str, EmojiFormats> = HashMap::new();
    // Reactions & Gestures
    map.insert("thumbs_up",      ef(m(&["+1", "thumbsup"]), s("\u{1F44D}")));
    map.insert("thumbs_down",    ef(m(&["-1", "thumbsdown"]), s("\u{1F44E}")));
    map.insert("clap",           ef(s("clap"), s("\u{1F44F}")));
    map.insert("wave",           ef(s("wave"), s("\u{1F44B}")));
    map.insert("pray",           ef(s("pray"), s("\u{1F64F}")));
    map.insert("muscle",         ef(s("muscle"), s("\u{1F4AA}")));
    map.insert("ok_hand",        ef(s("ok_hand"), s("\u{1F44C}")));
    map.insert("point_up",       ef(s("point_up"), s("\u{1F446}")));
    map.insert("point_down",     ef(s("point_down"), s("\u{1F447}")));
    map.insert("point_left",     ef(s("point_left"), s("\u{1F448}")));
    map.insert("point_right",    ef(s("point_right"), s("\u{1F449}")));
    map.insert("raised_hands",   ef(s("raised_hands"), s("\u{1F64C}")));
    map.insert("shrug",          ef(s("shrug"), s("\u{1F937}")));
    map.insert("facepalm",       ef(s("facepalm"), s("\u{1F926}")));
    // Emotions & Faces
    map.insert("heart",          ef(s("heart"), m(&["\u{2764}\u{FE0F}", "\u{2764}"])));
    map.insert("smile",          ef(m(&["smile", "slightly_smiling_face"]), s("\u{1F60A}")));
    map.insert("laugh",          ef(m(&["laughing", "satisfied", "joy"]), m(&["\u{1F602}", "\u{1F606}"])));
    map.insert("thinking",       ef(s("thinking_face"), s("\u{1F914}")));
    map.insert("sad",            ef(m(&["cry", "sad", "white_frowning_face"]), s("\u{1F622}")));
    map.insert("cry",            ef(s("sob"), s("\u{1F62D}")));
    map.insert("angry",          ef(s("angry"), s("\u{1F620}")));
    map.insert("love_eyes",      ef(s("heart_eyes"), s("\u{1F60D}")));
    map.insert("cool",           ef(s("sunglasses"), s("\u{1F60E}")));
    map.insert("wink",           ef(s("wink"), s("\u{1F609}")));
    map.insert("surprised",      ef(s("open_mouth"), s("\u{1F62E}")));
    map.insert("worried",        ef(s("worried"), s("\u{1F61F}")));
    map.insert("confused",       ef(s("confused"), s("\u{1F615}")));
    map.insert("neutral",        ef(s("neutral_face"), s("\u{1F610}")));
    map.insert("sleeping",       ef(s("sleeping"), s("\u{1F634}")));
    map.insert("sick",           ef(s("nauseated_face"), s("\u{1F922}")));
    map.insert("mind_blown",     ef(s("exploding_head"), s("\u{1F92F}")));
    map.insert("relieved",       ef(s("relieved"), s("\u{1F60C}")));
    map.insert("grimace",        ef(s("grimacing"), s("\u{1F62C}")));
    map.insert("rolling_eyes",   ef(s("rolling_eyes"), s("\u{1F644}")));
    map.insert("hug",            ef(s("hugging_face"), s("\u{1F917}")));
    map.insert("zany",           ef(s("zany_face"), s("\u{1F92A}")));
    // Status & Symbols
    map.insert("check",          ef(m(&["white_check_mark", "heavy_check_mark"]), m(&["\u{2705}", "\u{2714}\u{FE0F}"])));
    map.insert("x",              ef(m(&["x", "heavy_multiplication_x"]), m(&["\u{274C}", "\u{2716}\u{FE0F}"])));
    map.insert("question",       ef(s("question"), m(&["\u{2753}", "?"])));
    map.insert("exclamation",    ef(s("exclamation"), s("\u{2757}")));
    map.insert("warning",        ef(s("warning"), s("\u{26A0}\u{FE0F}")));
    map.insert("stop",           ef(s("octagonal_sign"), s("\u{1F6D1}")));
    map.insert("info",           ef(s("information_source"), s("\u{2139}\u{FE0F}")));
    map.insert("100",            ef(s("100"), s("\u{1F4AF}")));
    map.insert("fire",           ef(s("fire"), s("\u{1F525}")));
    map.insert("star",           ef(s("star"), s("\u{2B50}")));
    map.insert("sparkles",       ef(s("sparkles"), s("\u{2728}")));
    map.insert("lightning",      ef(s("zap"), s("\u{26A1}")));
    map.insert("boom",           ef(s("boom"), s("\u{1F4A5}")));
    map.insert("eyes",           ef(s("eyes"), s("\u{1F440}")));
    // Status Indicators
    map.insert("green_circle",   ef(s("large_green_circle"), s("\u{1F7E2}")));
    map.insert("yellow_circle",  ef(s("large_yellow_circle"), s("\u{1F7E1}")));
    map.insert("red_circle",     ef(s("red_circle"), s("\u{1F534}")));
    map.insert("blue_circle",    ef(s("large_blue_circle"), s("\u{1F535}")));
    map.insert("white_circle",   ef(s("white_circle"), s("\u{26AA}")));
    map.insert("black_circle",   ef(s("black_circle"), s("\u{26AB}")));
    // Objects & Tools
    map.insert("rocket",         ef(s("rocket"), s("\u{1F680}")));
    map.insert("party",          ef(m(&["tada", "partying_face"]), m(&["\u{1F389}", "\u{1F973}"])));
    map.insert("confetti",       ef(s("confetti_ball"), s("\u{1F38A}")));
    map.insert("balloon",        ef(s("balloon"), s("\u{1F388}")));
    map.insert("gift",           ef(s("gift"), s("\u{1F381}")));
    map.insert("trophy",         ef(s("trophy"), s("\u{1F3C6}")));
    map.insert("medal",          ef(s("first_place_medal"), s("\u{1F947}")));
    map.insert("lightbulb",      ef(s("bulb"), s("\u{1F4A1}")));
    map.insert("gear",           ef(s("gear"), s("\u{2699}\u{FE0F}")));
    map.insert("wrench",         ef(s("wrench"), s("\u{1F527}")));
    map.insert("hammer",         ef(s("hammer"), s("\u{1F528}")));
    map.insert("bug",            ef(s("bug"), s("\u{1F41B}")));
    map.insert("link",           ef(s("link"), s("\u{1F517}")));
    map.insert("lock",           ef(s("lock"), s("\u{1F512}")));
    map.insert("unlock",         ef(s("unlock"), s("\u{1F513}")));
    map.insert("key",            ef(s("key"), s("\u{1F511}")));
    map.insert("pin",            ef(s("pushpin"), s("\u{1F4CC}")));
    map.insert("memo",           ef(s("memo"), s("\u{1F4DD}")));
    map.insert("clipboard",      ef(s("clipboard"), s("\u{1F4CB}")));
    map.insert("calendar",       ef(s("calendar"), s("\u{1F4C5}")));
    map.insert("clock",          ef(s("clock1"), s("\u{1F550}")));
    map.insert("hourglass",      ef(s("hourglass"), s("\u{23F3}")));
    map.insert("bell",           ef(s("bell"), s("\u{1F514}")));
    map.insert("megaphone",      ef(s("mega"), s("\u{1F4E2}")));
    map.insert("speech_bubble",  ef(s("speech_balloon"), s("\u{1F4AC}")));
    map.insert("email",          ef(s("email"), s("\u{1F4E7}")));
    map.insert("inbox",          ef(s("inbox_tray"), s("\u{1F4E5}")));
    map.insert("outbox",         ef(s("outbox_tray"), s("\u{1F4E4}")));
    map.insert("package",        ef(s("package"), s("\u{1F4E6}")));
    map.insert("folder",         ef(s("file_folder"), s("\u{1F4C1}")));
    map.insert("file",           ef(s("page_facing_up"), s("\u{1F4C4}")));
    map.insert("chart_up",       ef(s("chart_with_upwards_trend"), s("\u{1F4C8}")));
    map.insert("chart_down",     ef(s("chart_with_downwards_trend"), s("\u{1F4C9}")));
    map.insert("coffee",         ef(s("coffee"), s("\u{2615}")));
    map.insert("pizza",          ef(s("pizza"), s("\u{1F355}")));
    map.insert("beer",           ef(s("beer"), s("\u{1F37A}")));
    // Arrows & Directions
    map.insert("arrow_up",       ef(s("arrow_up"), s("\u{2B06}\u{FE0F}")));
    map.insert("arrow_down",     ef(s("arrow_down"), s("\u{2B07}\u{FE0F}")));
    map.insert("arrow_left",     ef(s("arrow_left"), s("\u{2B05}\u{FE0F}")));
    map.insert("arrow_right",    ef(s("arrow_right"), s("\u{27A1}\u{FE0F}")));
    map.insert("refresh",        ef(s("arrows_counterclockwise"), s("\u{1F504}")));
    // Nature & Weather
    map.insert("sun",            ef(s("sunny"), s("\u{2600}\u{FE0F}")));
    map.insert("cloud",          ef(s("cloud"), s("\u{2601}\u{FE0F}")));
    map.insert("rain",           ef(s("rain_cloud"), s("\u{1F327}\u{FE0F}")));
    map.insert("snow",           ef(s("snowflake"), s("\u{2744}\u{FE0F}")));
    map.insert("rainbow",        ef(s("rainbow"), s("\u{1F308}")));
    map
}

/// All well-known emoji shortcodes, in the upstream-documented order.
/// Used by [`create_emoji`] to build the emoji helper.
pub const WELL_KNOWN_EMOJI: &[&str] = &[
    // Reactions & Gestures
    "thumbs_up",
    "thumbs_down",
    "clap",
    "wave",
    "pray",
    "muscle",
    "ok_hand",
    "point_up",
    "point_down",
    "point_left",
    "point_right",
    "raised_hands",
    "shrug",
    "facepalm",
    // Emotions & Faces
    "heart",
    "smile",
    "laugh",
    "thinking",
    "sad",
    "cry",
    "angry",
    "love_eyes",
    "cool",
    "wink",
    "surprised",
    "worried",
    "confused",
    "neutral",
    "sleeping",
    "sick",
    "mind_blown",
    "relieved",
    "grimace",
    "rolling_eyes",
    "hug",
    "zany",
    // Status & Symbols
    "check",
    "x",
    "question",
    "exclamation",
    "warning",
    "stop",
    "info",
    "100",
    "fire",
    "star",
    "sparkles",
    "lightning",
    "boom",
    "eyes",
    // Status Indicators
    "green_circle",
    "yellow_circle",
    "red_circle",
    "blue_circle",
    "white_circle",
    "black_circle",
    // Objects & Tools
    "rocket",
    "party",
    "confetti",
    "balloon",
    "gift",
    "trophy",
    "medal",
    "lightbulb",
    "gear",
    "wrench",
    "hammer",
    "bug",
    "link",
    "lock",
    "unlock",
    "key",
    "pin",
    "memo",
    "clipboard",
    "calendar",
    "clock",
    "hourglass",
    "bell",
    "megaphone",
    "speech_bubble",
    "email",
    "inbox",
    "outbox",
    "package",
    "folder",
    "file",
    "chart_up",
    "chart_down",
    "coffee",
    "pizza",
    "beer",
    // Arrows & Directions
    "arrow_up",
    "arrow_down",
    "arrow_left",
    "arrow_right",
    "refresh",
    // Nature & Weather
    "sun",
    "cloud",
    "rain",
    "snow",
    "rainbow",
];

/// Internal emoji registry for singleton instances. 1:1 port of
/// upstream `const emojiRegistry = new Map<string, EmojiValue>()`.
static EMOJI_REGISTRY: LazyLock<RwLock<HashMap<String, Arc<EmojiValue>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Get or create an immutable singleton [`EmojiValue`]. 1:1 port of
/// upstream `getEmoji(name): EmojiValue`.
///
/// Always returns the same `Arc<EmojiValue>` for the same name,
/// enabling `Arc::ptr_eq` for emoji identity comparisons.
pub fn get_emoji(name: &str) -> Arc<EmojiValue> {
    {
        let registry = EMOJI_REGISTRY.read().unwrap_or_else(|p| p.into_inner());
        if let Some(value) = registry.get(name) {
            return Arc::clone(value);
        }
    }
    let mut registry = EMOJI_REGISTRY.write().unwrap_or_else(|p| p.into_inner());
    // Re-check after acquiring write lock (race between checks).
    if let Some(value) = registry.get(name) {
        return Arc::clone(value);
    }
    let value = Arc::new(EmojiValue::new(name));
    registry.insert(name.to_string(), Arc::clone(&value));
    value
}

fn as_slice(formats_value: &StringOrList) -> Vec<&str> {
    match formats_value {
        StringOrList::One(s) => vec![s.as_str()],
        StringOrList::Many(v) => v.iter().map(String::as_str).collect(),
    }
}

fn first(formats_value: &StringOrList) -> &str {
    match formats_value {
        StringOrList::One(s) => s.as_str(),
        StringOrList::Many(v) => v.first().map(String::as_str).unwrap_or(""),
    }
}

/// Emoji resolver that handles conversion between platform formats
/// and normalized names. 1:1 port of upstream `class EmojiResolver`.
#[derive(Debug, Clone)]
pub struct EmojiResolver {
    emoji_map: HashMap<String, EmojiFormats>,
    slack_to_normalized: HashMap<String, String>,
    gchat_to_normalized: HashMap<String, String>,
}

impl Default for EmojiResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl EmojiResolver {
    /// Construct a resolver pre-seeded with [`DEFAULT_EMOJI_MAP`]. 1:1
    /// port of upstream `new EmojiResolver()`.
    pub fn new() -> Self {
        Self::with_custom_map(None)
    }

    /// Construct a resolver pre-seeded with [`DEFAULT_EMOJI_MAP`] then
    /// optionally overlaid with `custom_map`. 1:1 port of upstream
    /// `new EmojiResolver(customMap)`.
    pub fn with_custom_map(custom_map: Option<HashMap<String, EmojiFormats>>) -> Self {
        let mut emoji_map: HashMap<String, EmojiFormats> = DEFAULT_EMOJI_MAP
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect();
        if let Some(custom) = custom_map {
            for (k, v) in custom {
                emoji_map.insert(k, v);
            }
        }
        let mut resolver = Self {
            emoji_map,
            slack_to_normalized: HashMap::new(),
            gchat_to_normalized: HashMap::new(),
        };
        resolver.build_reverse_maps();
        resolver
    }

    fn build_reverse_maps(&mut self) {
        self.slack_to_normalized.clear();
        self.gchat_to_normalized.clear();
        for (normalized, formats) in &self.emoji_map {
            for slack in as_slice(&formats.slack) {
                self.slack_to_normalized
                    .insert(slack.to_lowercase(), normalized.clone());
            }
            for gchat in as_slice(&formats.gchat) {
                self.gchat_to_normalized
                    .insert(gchat.to_string(), normalized.clone());
            }
        }
    }

    /// Convert a Slack emoji name to a normalized [`EmojiValue`]. 1:1
    /// port of upstream `fromSlack`.
    pub fn from_slack(&self, slack_emoji: &str) -> Arc<EmojiValue> {
        let cleaned = slack_emoji
            .trim_start_matches(':')
            .trim_end_matches(':')
            .to_lowercase();
        let normalized = self
            .slack_to_normalized
            .get(&cleaned)
            .cloned()
            .unwrap_or_else(|| slack_emoji.to_string());
        get_emoji(&normalized)
    }

    /// Convert a Google Chat unicode emoji to a normalized
    /// [`EmojiValue`]. 1:1 port of upstream `fromGChat`.
    pub fn from_gchat(&self, gchat_emoji: &str) -> Arc<EmojiValue> {
        let normalized = self
            .gchat_to_normalized
            .get(gchat_emoji)
            .cloned()
            .unwrap_or_else(|| gchat_emoji.to_string());
        get_emoji(&normalized)
    }

    /// Convert a Teams reaction type to a normalized [`EmojiValue`].
    /// 1:1 port of upstream `fromTeams`.
    pub fn from_teams(&self, teams_reaction: &str) -> Arc<EmojiValue> {
        let normalized = match teams_reaction {
            "like" => "thumbs_up",
            "heart" => "heart",
            "laugh" => "laugh",
            "surprised" => "surprised",
            "sad" => "sad",
            "angry" => "angry",
            other => other,
        };
        get_emoji(normalized)
    }

    /// Convert a normalized name (or [`EmojiValue`]) to Slack format.
    /// Returns the first Slack format if multiple exist. 1:1 port of
    /// upstream `toSlack`.
    pub fn to_slack(&self, emoji: &str) -> String {
        match self.emoji_map.get(emoji) {
            Some(formats) => first(&formats.slack).to_string(),
            None => emoji.to_string(),
        }
    }

    /// Convert a normalized name to Google Chat format. 1:1 port of
    /// upstream `toGChat`.
    pub fn to_gchat(&self, emoji: &str) -> String {
        match self.emoji_map.get(emoji) {
            Some(formats) => first(&formats.gchat).to_string(),
            None => emoji.to_string(),
        }
    }

    /// Convert a normalized name to Discord format (unicode). 1:1 port
    /// of upstream `toDiscord`, which delegates to `toGChat`.
    pub fn to_discord(&self, emoji: &str) -> String {
        self.to_gchat(emoji)
    }

    /// Check if `raw_emoji` (in any format) matches `normalized`. 1:1
    /// port of upstream `matches`.
    pub fn matches(&self, raw_emoji: &str, normalized: &str) -> bool {
        let formats = match self.emoji_map.get(normalized) {
            Some(f) => f,
            None => return raw_emoji == normalized,
        };
        let cleaned_raw = raw_emoji
            .trim_start_matches(':')
            .trim_end_matches(':')
            .to_lowercase();
        as_slice(&formats.slack)
            .iter()
            .any(|s| s.to_lowercase() == cleaned_raw)
            || as_slice(&formats.gchat).iter().any(|g| *g == raw_emoji)
    }

    /// Add or override emoji mappings. 1:1 port of upstream `extend`.
    pub fn extend(&mut self, custom_map: HashMap<String, EmojiFormats>) {
        for (k, v) in custom_map {
            self.emoji_map.insert(k, v);
        }
        self.build_reverse_maps();
    }
}

/// Default emoji resolver instance. 1:1 port of upstream
/// `defaultEmojiResolver`. Backed by a `RwLock` so the upstream
/// `defaultEmojiResolver.extend(...)` mutation path remains reachable
/// from `create_emoji` and other callers.
pub static DEFAULT_EMOJI_RESOLVER: LazyLock<RwLock<EmojiResolver>> =
    LazyLock::new(|| RwLock::new(EmojiResolver::new()));

/// Target platform for [`convert_emoji_placeholders`]. 1:1 port of
/// upstream's string-literal union argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaceholderPlatform {
    /// Slack — wraps emoji name in colons (`:+1:`).
    Slack,
    /// Google Chat — unicode emoji.
    Gchat,
    /// Microsoft Teams — unicode emoji (same as Google Chat).
    Teams,
    /// Discord — unicode emoji.
    Discord,
    /// Facebook Messenger — unicode emoji.
    Messenger,
    /// GitHub — unicode emoji.
    Github,
    /// Linear — unicode emoji.
    Linear,
    /// WhatsApp — unicode emoji.
    Whatsapp,
}

/// Convert emoji placeholders (`{{emoji:name}}`) in `text` to the
/// platform-specific format. 1:1 port of upstream
/// `convertEmojiPlaceholders(text, platform, resolver?)`.
///
/// Pass `None` for `resolver` to use the global [`DEFAULT_EMOJI_RESOLVER`].
pub fn convert_emoji_placeholders(
    text: &str,
    platform: PlaceholderPlatform,
    resolver: Option<&EmojiResolver>,
) -> String {
    let owned_default;
    let resolver: &EmojiResolver = match resolver {
        Some(r) => r,
        None => {
            owned_default = DEFAULT_EMOJI_RESOLVER
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .clone();
            return convert_emoji_placeholders(text, platform, Some(&owned_default));
        }
    };

    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{emoji:") {
        out.push_str(&rest[..start]);
        let after_open = start + "{{emoji:".len();
        let tail = &rest[after_open..];
        let close_rel = match tail.find("}}") {
            Some(i) => i,
            None => {
                // No closing marker; treat the rest as literal.
                out.push_str(&rest[start..]);
                rest = "";
                break;
            }
        };
        let name = &tail[..close_rel];
        // Upstream uses /\{\{emoji:([a-z0-9_]+)\}\}/gi; reject names
        // with characters outside that class and pass through.
        let name_ok =
            !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !name_ok {
            out.push_str(&rest[start..after_open + close_rel + 2]);
        } else {
            let replacement = match platform {
                PlaceholderPlatform::Slack => format!(":{}:", resolver.to_slack(name)),
                PlaceholderPlatform::Gchat
                | PlaceholderPlatform::Teams
                | PlaceholderPlatform::Messenger
                | PlaceholderPlatform::Github
                | PlaceholderPlatform::Linear
                | PlaceholderPlatform::Whatsapp => resolver.to_gchat(name),
                PlaceholderPlatform::Discord => resolver.to_discord(name),
            };
            out.push_str(&replacement);
        }
        rest = &rest[after_open + close_rel + 2..];
    }
    out.push_str(rest);
    out
}

/// Create an emoji helper backed by [`get_emoji`] singletons. 1:1 port
/// of upstream `createEmoji(customEmoji?)`.
///
/// All well-known emoji are pre-populated. Custom entries are added to
/// the returned [`EmojiHelper`] AND registered with the global
/// [`DEFAULT_EMOJI_RESOLVER`] so [`convert_emoji_placeholders`] can
/// translate their placeholders.
pub fn create_emoji(custom_emoji: Option<HashMap<String, EmojiFormats>>) -> EmojiHelper {
    let mut helper = HashMap::with_capacity(WELL_KNOWN_EMOJI.len() + 16);
    for name in WELL_KNOWN_EMOJI {
        helper.insert((*name).to_string(), get_emoji(name));
    }
    if let Some(custom) = &custom_emoji {
        for key in custom.keys() {
            helper.insert(key.clone(), get_emoji(key));
        }
    }
    if let Some(custom) = custom_emoji {
        DEFAULT_EMOJI_RESOLVER
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .extend(custom);
    }
    EmojiHelper { entries: helper }
}

/// Map of name -> singleton [`EmojiValue`] returned by
/// [`create_emoji`]. Closest Rust analogue to upstream's
/// type-augmented `BaseEmojiHelper` object — Rust can't synthesize
/// fields from a type literal, so consumers access entries by name
/// via [`EmojiHelper::get`] or the `Index` impl.
#[derive(Debug, Clone)]
pub struct EmojiHelper {
    entries: HashMap<String, Arc<EmojiValue>>,
}

impl EmojiHelper {
    /// Look up an emoji by name. Returns `None` if the name was not in
    /// the well-known set or among the custom entries.
    pub fn get(&self, name: &str) -> Option<Arc<EmojiValue>> {
        self.entries.get(name).cloned()
    }

    /// Look up a custom emoji singleton by name. 1:1 port of upstream
    /// `helper.custom(name)`. Always returns an [`EmojiValue`] (creates
    /// it via [`get_emoji`] if missing).
    pub fn custom(&self, name: &str) -> Arc<EmojiValue> {
        get_emoji(name)
    }
}

impl std::ops::Index<&str> for EmojiHelper {
    type Output = Arc<EmojiValue>;
    fn index(&self, name: &str) -> &Self::Output {
        self.entries
            .get(name)
            .unwrap_or_else(|| panic!("EmojiHelper has no entry for `{name}`"))
    }
}

/// Global emoji helper. 1:1 port of upstream
/// `export const emoji = createEmoji()`. Lazily initialized.
pub fn emoji() -> EmojiHelper {
    static EMOJI: LazyLock<EmojiHelper> = LazyLock::new(|| create_emoji(None));
    EMOJI.clone()
}

#[cfg(test)]
mod tests {
    //! 1:1 port of `packages/chat/src/emoji.test.ts` (42 cases).
    use super::*;

    fn custom(pairs: &[(&str, EmojiFormats)]) -> HashMap<String, EmojiFormats> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect()
    }

    // ---------- EmojiResolver::from_slack ----------

    #[test]
    fn from_slack_converts_slack_name_to_normalized_emoji_value() {
        let r = EmojiResolver::new();
        assert_eq!(r.from_slack("+1").name, "thumbs_up");
        assert_eq!(r.from_slack("thumbsup").name, "thumbs_up");
        assert_eq!(r.from_slack("-1").name, "thumbs_down");
        assert_eq!(r.from_slack("heart").name, "heart");
        assert_eq!(r.from_slack("fire").name, "fire");
    }

    #[test]
    fn from_slack_handles_colons_around_emoji_names() {
        let r = EmojiResolver::new();
        assert_eq!(r.from_slack(":+1:").name, "thumbs_up");
        assert_eq!(r.from_slack(":fire:").name, "fire");
    }

    #[test]
    fn from_slack_is_case_insensitive() {
        let r = EmojiResolver::new();
        assert_eq!(r.from_slack("FIRE").name, "fire");
        assert_eq!(r.from_slack("Heart").name, "heart");
    }

    #[test]
    fn from_slack_returns_raw_name_when_no_mapping_exists() {
        let r = EmojiResolver::new();
        let result = r.from_slack("custom_emoji");
        assert_eq!(result.name, "custom_emoji");
        assert_eq!(result.to_string(), "{{emoji:custom_emoji}}");
    }

    // ---------- from_gchat ----------

    #[test]
    fn from_gchat_converts_unicode_emoji_to_normalized_value() {
        let r = EmojiResolver::new();
        assert_eq!(r.from_gchat("\u{1F44D}").name, "thumbs_up");
        assert_eq!(r.from_gchat("\u{1F44E}").name, "thumbs_down");
        assert_eq!(r.from_gchat("\u{2764}\u{FE0F}").name, "heart");
        assert_eq!(r.from_gchat("\u{1F525}").name, "fire");
        assert_eq!(r.from_gchat("\u{1F680}").name, "rocket");
    }

    #[test]
    fn from_gchat_handles_multiple_unicode_variants_for_heart_and_check() {
        let r = EmojiResolver::new();
        assert_eq!(r.from_gchat("\u{2764}").name, "heart");
        assert_eq!(r.from_gchat("\u{2764}\u{FE0F}").name, "heart");
        assert_eq!(r.from_gchat("\u{2705}").name, "check");
        assert_eq!(r.from_gchat("\u{2714}\u{FE0F}").name, "check");
    }

    #[test]
    fn from_gchat_returns_raw_emoji_when_no_mapping_exists() {
        let r = EmojiResolver::new();
        let result = r.from_gchat("\u{1F984}");
        assert_eq!(result.name, "\u{1F984}");
        assert_eq!(result.to_string(), "{{emoji:\u{1F984}}}");
    }

    // ---------- from_teams ----------

    #[test]
    fn from_teams_converts_teams_reaction_types_to_normalized_value() {
        let r = EmojiResolver::new();
        assert_eq!(r.from_teams("like").name, "thumbs_up");
        assert_eq!(r.from_teams("heart").name, "heart");
        assert_eq!(r.from_teams("laugh").name, "laugh");
        assert_eq!(r.from_teams("surprised").name, "surprised");
        assert_eq!(r.from_teams("sad").name, "sad");
        assert_eq!(r.from_teams("angry").name, "angry");
    }

    #[test]
    fn from_teams_returns_raw_name_when_no_mapping_exists() {
        let r = EmojiResolver::new();
        assert_eq!(r.from_teams("custom_reaction").name, "custom_reaction");
    }

    // ---------- to_slack / to_gchat ----------

    #[test]
    fn to_slack_converts_normalized_to_slack_format() {
        let r = EmojiResolver::new();
        assert_eq!(r.to_slack("thumbs_up"), "+1");
        assert_eq!(r.to_slack("fire"), "fire");
        assert_eq!(r.to_slack("heart"), "heart");
    }

    #[test]
    fn to_slack_returns_raw_emoji_when_no_mapping_exists() {
        let r = EmojiResolver::new();
        assert_eq!(r.to_slack("custom"), "custom");
    }

    #[test]
    fn to_gchat_converts_normalized_to_gchat_format() {
        let r = EmojiResolver::new();
        assert_eq!(r.to_gchat("thumbs_up"), "\u{1F44D}");
        assert_eq!(r.to_gchat("fire"), "\u{1F525}");
        assert_eq!(r.to_gchat("rocket"), "\u{1F680}");
    }

    #[test]
    fn to_gchat_returns_raw_emoji_when_no_mapping_exists() {
        let r = EmojiResolver::new();
        assert_eq!(r.to_gchat("custom"), "custom");
    }

    // ---------- matches ----------

    #[test]
    fn matches_recognizes_slack_format_against_normalized() {
        let r = EmojiResolver::new();
        assert!(r.matches("+1", "thumbs_up"));
        assert!(r.matches("thumbsup", "thumbs_up"));
        assert!(r.matches(":+1:", "thumbs_up"));
        assert!(r.matches("fire", "fire"));
    }

    #[test]
    fn matches_recognizes_gchat_format_against_normalized() {
        let r = EmojiResolver::new();
        assert!(r.matches("\u{1F44D}", "thumbs_up"));
        assert!(r.matches("\u{1F525}", "fire"));
        assert!(r.matches("\u{2764}\u{FE0F}", "heart"));
    }

    #[test]
    fn matches_returns_false_for_different_emoji() {
        let r = EmojiResolver::new();
        assert!(!r.matches("+1", "thumbs_down"));
        assert!(!r.matches("\u{1F44D}", "fire"));
    }

    #[test]
    fn matches_unmapped_emoji_falls_back_to_equality() {
        let r = EmojiResolver::new();
        assert!(r.matches("custom", "custom"));
        assert!(!r.matches("custom", "other"));
    }

    // ---------- extend ----------

    #[test]
    fn extend_adds_new_emoji_mappings_to_resolver() {
        let mut r = EmojiResolver::new();
        r.extend(custom(&[(
            "unicorn",
            ef(s("unicorn_face"), s("\u{1F984}")),
        )]));
        assert_eq!(r.from_slack("unicorn_face").name, "unicorn");
        assert_eq!(r.from_gchat("\u{1F984}").name, "unicorn");
        assert_eq!(r.to_slack("unicorn"), "unicorn_face");
        assert_eq!(r.to_gchat("unicorn"), "\u{1F984}");
    }

    #[test]
    fn extend_overrides_existing_mappings() {
        let mut r = EmojiResolver::new();
        r.extend(custom(&[("fire", ef(s("flames"), s("\u{1F525}")))]));
        assert_eq!(r.from_slack("flames").name, "fire");
        assert_eq!(r.to_slack("fire"), "flames");
    }

    // ---------- default_emoji_resolver / DEFAULT_EMOJI_MAP ----------

    #[test]
    fn default_emoji_resolver_is_pre_configured() {
        let r = DEFAULT_EMOJI_RESOLVER
            .read()
            .unwrap_or_else(|p| p.into_inner());
        assert_eq!(r.from_slack("+1").name, "thumbs_up");
    }

    #[test]
    fn default_emoji_map_contains_all_well_known_emoji() {
        for e in WELL_KNOWN_EMOJI {
            let formats = DEFAULT_EMOJI_MAP
                .get(*e)
                .unwrap_or_else(|| panic!("missing entry for {e}"));
            assert!(!first(&formats.slack).is_empty(), "slack empty for {e}");
            assert!(!first(&formats.gchat).is_empty(), "gchat empty for {e}");
        }
    }

    // ---------- emoji helper ----------

    #[test]
    fn emoji_helper_exposes_well_known_emoji_values() {
        let e = emoji();
        assert_eq!(e["thumbs_up"].name, "thumbs_up");
        assert_eq!(e["fire"].name, "fire");
        assert_eq!(e["rocket"].name, "rocket");
        assert_eq!(e["100"].name, "100");
    }

    #[test]
    fn emoji_helper_to_string_returns_upstream_placeholder() {
        let e = emoji();
        assert_eq!(e["thumbs_up"].to_string(), "{{emoji:thumbs_up}}");
        assert_eq!(e["fire"].to_string(), "{{emoji:fire}}");
        assert_eq!(format!("{}", e["rocket"]), "{{emoji:rocket}}");
    }

    #[test]
    fn emoji_helper_has_object_identity_for_same_name() {
        let e = emoji();
        let a = e.get("thumbs_up").unwrap();
        let b = e.get("thumbs_up").unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        let from_get_emoji = get_emoji("thumbs_up");
        assert!(Arc::ptr_eq(&a, &from_get_emoji));
    }

    #[test]
    fn emoji_helper_custom_method_returns_emoji_value() {
        let e = emoji();
        let unicorn = e.custom("unicorn");
        assert_eq!(unicorn.name, "unicorn");
        assert_eq!(unicorn.to_string(), "{{emoji:unicorn}}");
        let custom = e.custom("custom_team_emoji");
        assert_eq!(custom.name, "custom_team_emoji");
        assert_eq!(format!("{custom}"), "{{emoji:custom_team_emoji}}");
    }

    #[test]
    fn emoji_helper_custom_returns_same_singleton_for_same_name() {
        let e = emoji();
        let first = e.custom("test_emoji_singleton");
        let second = e.custom("test_emoji_singleton");
        assert!(Arc::ptr_eq(&first, &second));
    }

    // ---------- convert_emoji_placeholders ----------

    fn p(s: &str) -> String {
        format!("{{{{emoji:{s}}}}}")
    }

    #[test]
    fn convert_emoji_placeholders_to_slack_format() {
        let text = format!("Thanks! {} Great work! {}", p("thumbs_up"), p("fire"));
        let r = convert_emoji_placeholders(&text, PlaceholderPlatform::Slack, None);
        assert_eq!(r, "Thanks! :+1: Great work! :fire:");
    }

    #[test]
    fn convert_emoji_placeholders_to_gchat_format() {
        let text = format!("Thanks! {} Great work! {}", p("thumbs_up"), p("fire"));
        let r = convert_emoji_placeholders(&text, PlaceholderPlatform::Gchat, None);
        assert_eq!(r, "Thanks! \u{1F44D} Great work! \u{1F525}");
    }

    #[test]
    fn convert_emoji_placeholders_to_teams_format_uses_unicode() {
        let text = format!("Thanks! {} Great work! {}", p("thumbs_up"), p("fire"));
        let r = convert_emoji_placeholders(&text, PlaceholderPlatform::Teams, None);
        assert_eq!(r, "Thanks! \u{1F44D} Great work! \u{1F525}");
    }

    #[test]
    fn convert_emoji_placeholders_passes_through_unknown_emoji_for_slack() {
        let r = convert_emoji_placeholders(
            "Check this {{emoji:unknown_emoji}}!",
            PlaceholderPlatform::Slack,
            None,
        );
        assert_eq!(r, "Check this :unknown_emoji:!");
    }

    #[test]
    fn convert_emoji_placeholders_handles_multiple_emoji_in_a_message() {
        let text = format!(
            "{} Hello! {} How are you? {}",
            p("wave"),
            p("smile"),
            p("thumbs_up")
        );
        let r = convert_emoji_placeholders(&text, PlaceholderPlatform::Gchat, None);
        assert_eq!(r, "\u{1F44B} Hello! \u{1F60A} How are you? \u{1F44D}");
    }

    #[test]
    fn convert_emoji_placeholders_returns_text_unchanged_when_no_emoji() {
        let r =
            convert_emoji_placeholders("Just a regular message", PlaceholderPlatform::Slack, None);
        assert_eq!(r, "Just a regular message");
    }

    #[test]
    fn convert_emoji_placeholders_to_messenger_uses_unicode() {
        let text = format!("Thanks! {} Great work! {}", p("thumbs_up"), p("fire"));
        let r = convert_emoji_placeholders(&text, PlaceholderPlatform::Messenger, None);
        assert_eq!(r, "Thanks! \u{1F44D} Great work! \u{1F525}");
    }

    #[test]
    fn convert_emoji_placeholders_handles_multiple_messenger_emoji() {
        let text = format!(
            "{} Hello! {} How are you? {}",
            p("wave"),
            p("smile"),
            p("rocket")
        );
        let r = convert_emoji_placeholders(&text, PlaceholderPlatform::Messenger, None);
        assert_eq!(r, "\u{1F44B} Hello! \u{1F60A} How are you? \u{1F680}");
    }

    #[test]
    fn convert_emoji_placeholders_passes_through_unknown_emoji_for_messenger() {
        let r = convert_emoji_placeholders(
            "Check this {{emoji:unknown_emoji}}!",
            PlaceholderPlatform::Messenger,
            None,
        );
        assert_eq!(r, "Check this unknown_emoji!");
    }

    #[test]
    fn convert_emoji_placeholders_handles_no_emoji_for_messenger() {
        let r = convert_emoji_placeholders(
            "Plain message with no emoji",
            PlaceholderPlatform::Messenger,
            None,
        );
        assert_eq!(r, "Plain message with no emoji");
    }

    #[test]
    fn convert_emoji_placeholders_produces_identical_output_for_messenger_and_gchat() {
        let text = format!("{} {} {} {}", p("heart"), p("check"), p("star"), p("party"));
        let messenger = convert_emoji_placeholders(&text, PlaceholderPlatform::Messenger, None);
        let gchat = convert_emoji_placeholders(&text, PlaceholderPlatform::Gchat, None);
        assert_eq!(messenger, gchat);
    }

    // ---------- create_emoji ----------

    #[test]
    fn create_emoji_returns_helper_with_well_known_values() {
        let e = create_emoji(None);
        assert_eq!(e["thumbs_up"].name, "thumbs_up");
        assert_eq!(e["fire"].name, "fire");
        assert_eq!(e["rocket"].name, "rocket");
        assert_eq!(format!("{}", e["thumbs_up"]), "{{emoji:thumbs_up}}");
    }

    #[test]
    fn create_emoji_helper_custom_returns_emoji_value() {
        let e = create_emoji(None);
        let unicorn = e.custom("unicorn_helper");
        assert_eq!(unicorn.name, "unicorn_helper");
        assert_eq!(unicorn.to_string(), "{{emoji:unicorn_helper}}");
    }

    #[test]
    fn create_emoji_adds_custom_emoji_to_helper_as_emoji_values() {
        let e = create_emoji(Some(custom(&[
            ("ce_unicorn", ef(s("ce_unicorn_face"), s("\u{1F984}"))),
            ("ce_company_logo", ef(s("ce_company"), s("\u{1F3E2}"))),
        ])));
        assert_eq!(e["ce_unicorn"].name, "ce_unicorn");
        assert_eq!(e["ce_company_logo"].name, "ce_company_logo");
        assert_eq!(format!("{}", e["ce_unicorn"]), "{{emoji:ce_unicorn}}");
        assert_eq!(
            format!("{}", e["ce_company_logo"]),
            "{{emoji:ce_company_logo}}"
        );
        assert_eq!(e["thumbs_up"].name, "thumbs_up");
    }

    #[test]
    fn create_emoji_registers_custom_emoji_with_default_resolver() {
        let e = create_emoji(Some(custom(&[(
            "ce_custom_test",
            ef(s("ce_custom_slack"), s("\u{1F3AF}")),
        )])));
        let text = format!("{} Magic!", e["ce_custom_test"]);
        assert_eq!(
            convert_emoji_placeholders(&text, PlaceholderPlatform::Slack, None),
            ":ce_custom_slack: Magic!"
        );
        assert_eq!(
            convert_emoji_placeholders(&text, PlaceholderPlatform::Gchat, None),
            "\u{1F3AF} Magic!"
        );
    }

    #[test]
    fn create_emoji_returns_same_singleton_as_emoji_helper() {
        let e = create_emoji(None);
        let h = emoji();
        assert!(Arc::ptr_eq(&e["thumbs_up"], &h["thumbs_up"]));
        assert!(Arc::ptr_eq(&e["fire"], &h["fire"]));
    }
}
