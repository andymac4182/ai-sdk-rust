//! User info caching utilities for the Google Chat adapter.
//!
//! 1:1 port of `packages/adapter-gchat/src/user-info.ts`. Google Chat
//! Pub/Sub messages don't include user display names, so we cache them
//! from direct webhook messages for later use.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chat_sdk_chat::logger::Logger;
use chat_sdk_chat::types::{StateAdapter, StateResult};
use serde::{Deserialize, Serialize};

/// Key prefix for user info cache. 1:1 with upstream
/// `USER_INFO_KEY_PREFIX = "gchat:user:"`.
const USER_INFO_KEY_PREFIX: &str = "gchat:user:";

/// TTL for user info cache (7 days). 1:1 with upstream
/// `USER_INFO_CACHE_TTL_MS = 7 * 24 * 60 * 60 * 1000`.
const USER_INFO_CACHE_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// Cached user info. 1:1 port of upstream `interface CachedUserInfo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedUserInfo {
    #[serde(rename = "avatarUrl", default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(rename = "isBot", default, skip_serializing_if = "Option::is_none")]
    pub is_bot: Option<bool>,
}

/// User info cache that stores display names for Google Chat users.
/// Uses both an in-memory cache (fast path) and an optional persistent
/// state adapter. 1:1 port of upstream `class UserInfoCache`.
pub struct UserInfoCache {
    in_memory_cache: Mutex<HashMap<String, CachedUserInfo>>,
    state: Option<Arc<dyn StateAdapter>>,
    logger: Arc<dyn Logger>,
}

impl std::fmt::Debug for UserInfoCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserInfoCache")
            .field(
                "in_memory_cache_size",
                &self.in_memory_cache.lock().map(|m| m.len()).unwrap_or(0),
            )
            .field("state", &self.state.as_ref().map(|_| "<state>"))
            .finish()
    }
}

impl UserInfoCache {
    /// Create a new cache. `state` is optional - when `None`, only the
    /// in-memory cache is used. 1:1 with upstream
    /// `new UserInfoCache(state, logger)`.
    pub fn new(state: Option<Arc<dyn StateAdapter>>, logger: Arc<dyn Logger>) -> Self {
        Self {
            in_memory_cache: Mutex::new(HashMap::new()),
            state,
            logger,
        }
    }

    /// Cache user info for later lookup. 1:1 with upstream
    /// `set(userId, displayName, email?, isBot?, avatarUrl?)`. Skips
    /// empty / `"unknown"` display names without erroring.
    pub async fn set(
        &self,
        user_id: &str,
        display_name: &str,
        email: Option<&str>,
        is_bot: Option<bool>,
        avatar_url: Option<&str>,
    ) -> StateResult<()> {
        if display_name.is_empty() || display_name == "unknown" {
            return Ok(());
        }

        let user_info = CachedUserInfo {
            avatar_url: avatar_url.map(str::to_owned),
            display_name: display_name.to_string(),
            email: email.map(str::to_owned),
            is_bot,
        };

        // Always update in-memory cache.
        if let Ok(mut cache) = self.in_memory_cache.lock() {
            cache.insert(user_id.to_string(), user_info.clone());
        }

        // Also persist to state adapter when available.
        if let Some(state) = &self.state {
            let key = format!("{USER_INFO_KEY_PREFIX}{user_id}");
            let value = serde_json::to_value(&user_info)
                .map_err(|err| chat_sdk_chat::types::StateAdapterError::Io(Box::new(err)))?;
            state.set(&key, value, Some(USER_INFO_CACHE_TTL_MS)).await?;
        }

        Ok(())
    }

    /// Get cached user info. Checks in-memory cache first, then falls
    /// back to the state adapter when configured. 1:1 with upstream
    /// `get(userId)`.
    pub async fn get(&self, user_id: &str) -> StateResult<Option<CachedUserInfo>> {
        // Check in-memory cache first (fast path).
        if let Ok(cache) = self.in_memory_cache.lock()
            && let Some(value) = cache.get(user_id)
        {
            return Ok(Some(value.clone()));
        }

        // Fall back to state adapter.
        let Some(state) = &self.state else {
            return Ok(None);
        };

        let key = format!("{USER_INFO_KEY_PREFIX}{user_id}");
        let stored = state.get(&key).await?;
        let Some(value) = stored else {
            return Ok(None);
        };

        let parsed: CachedUserInfo = serde_json::from_value(value)
            .map_err(|err| chat_sdk_chat::types::StateAdapterError::Io(Box::new(err)))?;

        // Populate in-memory cache for next time.
        if let Ok(mut cache) = self.in_memory_cache.lock() {
            cache.insert(user_id.to_string(), parsed.clone());
        }

        Ok(Some(parsed))
    }

    /// Resolve a display name, using cache if available. 1:1 with
    /// upstream `resolveDisplayName(userId, providedDisplayName?,
    /// botUserId?, botUserName)`.
    pub async fn resolve_display_name(
        &self,
        user_id: &str,
        provided_display_name: Option<&str>,
        bot_user_id: Option<&str>,
        bot_user_name: &str,
    ) -> String {
        // If display name is provided and not "unknown", use it
        // and cache it (best-effort - errors logged, not propagated).
        if let Some(name) = provided_display_name
            && !name.is_empty()
            && name != "unknown"
        {
            if let Err(err) = self.set(user_id, name, None, None, None).await {
                self.logger
                    .error("Failed to cache user info", &[&user_id, &format!("{err}")]);
            }
            return name.to_string();
        }

        // Bot self-identification.
        if let Some(bot_id) = bot_user_id
            && user_id == bot_id
        {
            return bot_user_name.to_string();
        }

        // Try cache.
        if let Ok(Some(cached)) = self.get(user_id).await
            && !cached.display_name.is_empty()
        {
            return cached.display_name;
        }

        // Fall back to formatted user id (e.g. "users/999" -> "User 999").
        user_id.replacen("users/", "User ", 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chat_sdk_chat::logger::Logger;
    use chat_sdk_chat::types::StateAdapter;
    use chat_sdk_state_memory::MemoryStateAdapter;
    use std::fmt;

    /// Test logger that just discards messages. The upstream test mock
    /// uses `vi.fn()` spies; the Rust port doesn't need to observe log
    /// calls so a no-op logger suffices for these cases.
    #[derive(Debug)]
    struct NoopLogger;

    impl Logger for NoopLogger {
        fn debug(&self, _: &str, _: &[&dyn fmt::Display]) {}
        fn info(&self, _: &str, _: &[&dyn fmt::Display]) {}
        fn warn(&self, _: &str, _: &[&dyn fmt::Display]) {}
        fn error(&self, _: &str, _: &[&dyn fmt::Display]) {}
        fn child(&self, _: &str) -> Box<dyn Logger> {
            Box::new(NoopLogger)
        }
    }

    fn make_state() -> Arc<dyn StateAdapter> {
        let s = MemoryStateAdapter::new();
        s.connect().unwrap();
        Arc::new(s)
    }

    fn make_logger() -> Arc<dyn Logger> {
        Arc::new(NoopLogger)
    }

    // ---------- set (4 upstream cases) ----------

    #[test]
    fn set_stores_in_memory_and_persists_to_state() {
        futures_executor::block_on(async {
            let state = make_state();
            let cache = UserInfoCache::new(Some(state), make_logger());

            cache
                .set(
                    "users/123",
                    "John Doe",
                    Some("john@example.com"),
                    None,
                    None,
                )
                .await
                .unwrap();

            let result = cache.get("users/123").await.unwrap().unwrap();
            assert_eq!(result.display_name, "John Doe");
            assert_eq!(result.email.as_deref(), Some("john@example.com"));
        });
    }

    #[test]
    fn set_skips_empty_display_names() {
        futures_executor::block_on(async {
            let state = make_state();
            let cache = UserInfoCache::new(Some(state), make_logger());

            cache.set("users/123", "", None, None, None).await.unwrap();

            let result = cache.get("users/123").await.unwrap();
            assert!(result.is_none());
        });
    }

    #[test]
    fn set_skips_unknown_display_name() {
        futures_executor::block_on(async {
            let state = make_state();
            let cache = UserInfoCache::new(Some(state), make_logger());

            cache
                .set("users/123", "unknown", None, None, None)
                .await
                .unwrap();

            let result = cache.get("users/123").await.unwrap();
            assert!(result.is_none());
        });
    }

    #[test]
    fn set_works_without_state_adapter() {
        futures_executor::block_on(async {
            let cache = UserInfoCache::new(None, make_logger());

            cache
                .set("users/123", "John Doe", None, None, None)
                .await
                .unwrap();

            let result = cache.get("users/123").await.unwrap().unwrap();
            assert_eq!(result.display_name, "John Doe");
            assert!(result.email.is_none());
        });
    }

    // ---------- get (5 upstream cases) ----------

    #[test]
    fn get_returns_from_in_memory_cache_first() {
        futures_executor::block_on(async {
            let state = make_state();
            let cache = UserInfoCache::new(Some(state.clone()), make_logger());

            cache
                .set("users/123", "John Doe", None, None, None)
                .await
                .unwrap();

            // Clear state to verify in-memory is used.
            state
                .delete(&format!("{USER_INFO_KEY_PREFIX}users/123"))
                .await
                .unwrap();

            let result = cache.get("users/123").await.unwrap().unwrap();
            assert_eq!(result.display_name, "John Doe");
            assert!(result.email.is_none());
        });
    }

    #[test]
    fn get_falls_back_to_state_adapter() {
        futures_executor::block_on(async {
            let state = make_state();
            let cache = UserInfoCache::new(Some(state.clone()), make_logger());

            // Seed state directly to simulate cold cache.
            state
                .set(
                    &format!("{USER_INFO_KEY_PREFIX}users/456"),
                    serde_json::json!({
                        "displayName": "Jane",
                        "email": "jane@example.com",
                    }),
                    None,
                )
                .await
                .unwrap();

            let result = cache.get("users/456").await.unwrap().unwrap();
            assert_eq!(result.display_name, "Jane");
            assert_eq!(result.email.as_deref(), Some("jane@example.com"));
        });
    }

    #[test]
    fn get_populates_in_memory_cache_on_state_hit() {
        futures_executor::block_on(async {
            let state = make_state();
            let cache = UserInfoCache::new(Some(state.clone()), make_logger());

            state
                .set(
                    &format!("{USER_INFO_KEY_PREFIX}users/789"),
                    serde_json::json!({"displayName": "Bob"}),
                    None,
                )
                .await
                .unwrap();

            // First get populates in-memory.
            let _ = cache.get("users/789").await.unwrap();

            // Clear state; second get should use in-memory.
            state
                .delete(&format!("{USER_INFO_KEY_PREFIX}users/789"))
                .await
                .unwrap();
            let result = cache.get("users/789").await.unwrap().unwrap();
            assert_eq!(result.display_name, "Bob");
        });
    }

    #[test]
    fn get_returns_none_for_unknown_users() {
        futures_executor::block_on(async {
            let state = make_state();
            let cache = UserInfoCache::new(Some(state), make_logger());

            let result = cache.get("users/unknown").await.unwrap();
            assert!(result.is_none());
        });
    }

    #[test]
    fn get_returns_none_without_state_adapter_for_uncached_user() {
        futures_executor::block_on(async {
            let cache = UserInfoCache::new(None, make_logger());

            let result = cache.get("users/unknown").await.unwrap();
            assert!(result.is_none());
        });
    }

    // ---------- resolveDisplayName (5 upstream cases) ----------

    #[test]
    fn resolve_display_name_uses_provided_display_name() {
        futures_executor::block_on(async {
            let state = make_state();
            let cache = UserInfoCache::new(Some(state), make_logger());

            let name = cache
                .resolve_display_name("users/123", Some("John Doe"), Some("users/bot"), "chatbot")
                .await;
            assert_eq!(name, "John Doe");
        });
    }

    #[test]
    fn resolve_display_name_skips_unknown_provided_name() {
        futures_executor::block_on(async {
            let state = make_state();
            let cache = UserInfoCache::new(Some(state), make_logger());

            cache
                .set("users/123", "Cached Name", None, None, None)
                .await
                .unwrap();

            let name = cache
                .resolve_display_name("users/123", Some("unknown"), Some("users/bot"), "chatbot")
                .await;
            assert_eq!(name, "Cached Name");
        });
    }

    #[test]
    fn resolve_display_name_returns_bot_name_for_bot_user_id() {
        futures_executor::block_on(async {
            let state = make_state();
            let cache = UserInfoCache::new(Some(state), make_logger());

            let name = cache
                .resolve_display_name("users/bot", None, Some("users/bot"), "chatbot")
                .await;
            assert_eq!(name, "chatbot");
        });
    }

    #[test]
    fn resolve_display_name_uses_cache_for_unknown_display_name() {
        futures_executor::block_on(async {
            let state = make_state();
            let cache = UserInfoCache::new(Some(state), make_logger());

            cache
                .set("users/456", "Cached User", None, None, None)
                .await
                .unwrap();

            let name = cache
                .resolve_display_name("users/456", None, Some("users/bot"), "chatbot")
                .await;
            assert_eq!(name, "Cached User");
        });
    }

    #[test]
    fn resolve_display_name_falls_back_to_formatted_user_id() {
        futures_executor::block_on(async {
            let state = make_state();
            let cache = UserInfoCache::new(Some(state), make_logger());

            let name = cache
                .resolve_display_name("users/999", None, Some("users/bot"), "chatbot")
                .await;
            assert_eq!(name, "User 999");
        });
    }
}
