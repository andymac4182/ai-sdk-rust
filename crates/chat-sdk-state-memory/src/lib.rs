//! In-memory state adapter for development and testing.
//!
//! 1:1 port of `packages/state-memory/src/index.ts`. Provides
//! [`MemoryStateAdapter`] — an in-process backend with subscriptions,
//! locks, key/value cache, lists, and per-thread queues. Suitable for
//! tests and local dev; **not** production-safe (no persistence).
//!
//! ## Async strategy
//!
//! Upstream returns `Promise<T>` from every method. The Rust port
//! exposes synchronous `&self` methods backed by interior `Mutex`es:
//! the in-memory backend has no real I/O, so async-wrapping every
//! operation only adds executor overhead and forces a runtime
//! dependency on callers. Production state backends (Redis, ioredis,
//! Postgres) DO perform I/O and will expose async methods through the
//! [`chat_sdk_chat::types::StateAdapter`] trait once that trait is
//! extended in a future slice. The semantics observable through the
//! upstream test suite are identical.
//!
//! ## What this crate ships (slice 45)
//!
//! - [`MemoryStateAdapter`] struct with `connect` / `disconnect`,
//!   subscriptions, lock acquisition/release/extension/force-release,
//!   get/set/setIfNotExists/delete/appendToList/getList,
//!   enqueue/dequeue/queueDepth.
//! - [`MemoryStateAdapterOptions`] (empty for API symmetry with future
//!   state backends).
//! - [`create_memory_state`] factory function (1:1 with upstream
//!   `createMemoryState`).
//! - [`StateError`] for the upstream `"not connected"` error path.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use chat_sdk_chat::types::{Lock, QueueEntry};
use rand::Rng;
use rand::distributions::Alphanumeric;

/// Errors returned by [`MemoryStateAdapter`]. The single "not
/// connected" variant maps to upstream `throw new Error(...)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    /// `connect()` was not called before another method.
    NotConnected,
}

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StateError::NotConnected => {
                f.write_str("MemoryStateAdapter is not connected. Call connect() first.")
            }
        }
    }
}

impl std::error::Error for StateError {}

#[derive(Debug, Clone)]
struct MemoryLock {
    expires_at: u64,
    thread_id: String,
    token: String,
}

impl From<&MemoryLock> for Lock {
    fn from(value: &MemoryLock) -> Self {
        Lock {
            expires_at: value.expires_at,
            thread_id: value.thread_id.clone(),
            token: value.token.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct CachedValue {
    expires_at: Option<u64>,
    value: serde_json::Value,
}

#[derive(Debug, Default)]
struct State {
    subscriptions: HashSet<String>,
    locks: HashMap<String, MemoryLock>,
    cache: HashMap<String, CachedValue>,
    queues: HashMap<String, Vec<QueueEntry>>,
    connected: bool,
}

/// In-memory state adapter for development and testing. 1:1 port of
/// upstream `class MemoryStateAdapter`.
#[derive(Debug, Default)]
pub struct MemoryStateAdapter {
    state: Mutex<State>,
}

/// Options for [`create_memory_state`]. 1:1 port of upstream
/// `MemoryStateAdapterOptions = {}` — currently empty; type exists for
/// API symmetry with future state backends.
#[derive(Debug, Clone, Copy, Default)]
pub struct MemoryStateAdapterOptions;

/// Factory function. 1:1 port of upstream
/// `createMemoryState(options?): MemoryStateAdapter`.
pub fn create_memory_state(_options: Option<MemoryStateAdapterOptions>) -> MemoryStateAdapter {
    MemoryStateAdapter::default()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn generate_token() -> String {
    let suffix: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(13)
        .map(char::from)
        .collect::<String>()
        .to_lowercase();
    format!("mem_{}_{}", now_ms(), suffix)
}

impl MemoryStateAdapter {
    /// Construct a new disconnected adapter.
    pub fn new() -> Self {
        Self::default()
    }

    fn with_state<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut State) -> R,
    {
        let mut guard = self.state.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut guard)
    }

    fn ensure_connected(state: &State) -> Result<(), StateError> {
        if state.connected {
            Ok(())
        } else {
            Err(StateError::NotConnected)
        }
    }

    fn clean_expired_locks(state: &mut State) {
        let now = now_ms();
        state.locks.retain(|_, lock| lock.expires_at > now);
    }

    /// Connect the adapter. 1:1 port of upstream `connect`. Subsequent
    /// calls are no-ops (idempotent).
    pub fn connect(&self) -> Result<(), StateError> {
        self.with_state(|s| {
            s.connected = true;
            Ok(())
        })
    }

    /// Disconnect and clear all state. 1:1 port of upstream
    /// `disconnect`.
    pub fn disconnect(&self) -> Result<(), StateError> {
        self.with_state(|s| {
            s.connected = false;
            s.subscriptions.clear();
            s.locks.clear();
            s.queues.clear();
            Ok(())
        })
    }

    /// Subscribe to a thread. 1:1 port of upstream `subscribe`.
    pub fn subscribe(&self, thread_id: &str) -> Result<(), StateError> {
        self.with_state(|s| {
            Self::ensure_connected(s)?;
            s.subscriptions.insert(thread_id.to_string());
            Ok(())
        })
    }

    /// Unsubscribe from a thread. 1:1 port of upstream `unsubscribe`.
    pub fn unsubscribe(&self, thread_id: &str) -> Result<(), StateError> {
        self.with_state(|s| {
            Self::ensure_connected(s)?;
            s.subscriptions.remove(thread_id);
            Ok(())
        })
    }

    /// Check whether a thread is subscribed. 1:1 port of upstream
    /// `isSubscribed`.
    pub fn is_subscribed(&self, thread_id: &str) -> Result<bool, StateError> {
        self.with_state(|s| {
            Self::ensure_connected(s)?;
            Ok(s.subscriptions.contains(thread_id))
        })
    }

    /// Acquire a lock. 1:1 port of upstream `acquireLock`. Returns
    /// `None` if a non-expired lock is already held for the thread.
    pub fn acquire_lock(&self, thread_id: &str, ttl_ms: u64) -> Result<Option<Lock>, StateError> {
        self.with_state(|s| {
            Self::ensure_connected(s)?;
            Self::clean_expired_locks(s);
            if let Some(existing) = s.locks.get(thread_id) {
                if existing.expires_at > now_ms() {
                    return Ok(None);
                }
            }
            let lock = MemoryLock {
                expires_at: now_ms() + ttl_ms,
                thread_id: thread_id.to_string(),
                token: generate_token(),
            };
            let result = Lock::from(&lock);
            s.locks.insert(thread_id.to_string(), lock);
            Ok(Some(result))
        })
    }

    /// Force-release a lock without checking ownership. 1:1 port of
    /// upstream `forceReleaseLock`. Silently no-ops for nonexistent
    /// threads.
    pub fn force_release_lock(&self, thread_id: &str) -> Result<(), StateError> {
        self.with_state(|s| {
            Self::ensure_connected(s)?;
            s.locks.remove(thread_id);
            Ok(())
        })
    }

    /// Release a lock, validating the token. 1:1 port of upstream
    /// `releaseLock`. Silently no-ops if the token doesn't match.
    pub fn release_lock(&self, lock: &Lock) -> Result<(), StateError> {
        self.with_state(|s| {
            Self::ensure_connected(s)?;
            if let Some(existing) = s.locks.get(&lock.thread_id) {
                if existing.token == lock.token {
                    s.locks.remove(&lock.thread_id);
                }
            }
            Ok(())
        })
    }

    /// Extend an existing lock by `ttl_ms`. 1:1 port of upstream
    /// `extendLock`. Returns `false` when the lock is missing,
    /// token-mismatched, or already expired.
    pub fn extend_lock(&self, lock: &Lock, ttl_ms: u64) -> Result<bool, StateError> {
        self.with_state(|s| {
            Self::ensure_connected(s)?;
            let now = now_ms();
            let Some(existing) = s.locks.get_mut(&lock.thread_id) else {
                return Ok(false);
            };
            if existing.token != lock.token {
                return Ok(false);
            }
            if existing.expires_at < now {
                s.locks.remove(&lock.thread_id);
                return Ok(false);
            }
            existing.expires_at = now + ttl_ms;
            Ok(true)
        })
    }

    /// Read a cached value. 1:1 port of upstream `get<T>`. Returns
    /// `None` when missing or expired.
    pub fn get(&self, key: &str) -> Result<Option<serde_json::Value>, StateError> {
        self.with_state(|s| {
            Self::ensure_connected(s)?;
            let Some(cached) = s.cache.get(key) else {
                return Ok(None);
            };
            if matches!(cached.expires_at, Some(exp) if exp <= now_ms()) {
                s.cache.remove(key);
                return Ok(None);
            }
            Ok(Some(cached.value.clone()))
        })
    }

    /// Write a cached value with an optional TTL. 1:1 port of upstream
    /// `set<T>(key, value, ttlMs?)`.
    pub fn set(
        &self,
        key: &str,
        value: serde_json::Value,
        ttl_ms: Option<u64>,
    ) -> Result<(), StateError> {
        self.with_state(|s| {
            Self::ensure_connected(s)?;
            s.cache.insert(
                key.to_string(),
                CachedValue {
                    expires_at: ttl_ms.map(|t| now_ms() + t),
                    value,
                },
            );
            Ok(())
        })
    }

    /// Set only if the key is absent or expired. 1:1 port of upstream
    /// `setIfNotExists`.
    pub fn set_if_not_exists(
        &self,
        key: &str,
        value: serde_json::Value,
        ttl_ms: Option<u64>,
    ) -> Result<bool, StateError> {
        self.with_state(|s| {
            Self::ensure_connected(s)?;
            if let Some(existing) = s.cache.get(key).cloned() {
                if matches!(existing.expires_at, Some(exp) if exp <= now_ms()) {
                    s.cache.remove(key);
                } else {
                    return Ok(false);
                }
            }
            s.cache.insert(
                key.to_string(),
                CachedValue {
                    expires_at: ttl_ms.map(|t| now_ms() + t),
                    value,
                },
            );
            Ok(true)
        })
    }

    /// Delete a cached key. 1:1 port of upstream `delete`.
    pub fn delete(&self, key: &str) -> Result<(), StateError> {
        self.with_state(|s| {
            Self::ensure_connected(s)?;
            s.cache.remove(key);
            Ok(())
        })
    }

    /// Options for [`MemoryStateAdapter::append_to_list`]. 1:1 port of
    /// upstream `{ maxLength?: number; ttlMs?: number }`.
    pub fn append_to_list(
        &self,
        key: &str,
        value: serde_json::Value,
        max_length: Option<usize>,
        ttl_ms: Option<u64>,
    ) -> Result<(), StateError> {
        self.with_state(|s| {
            Self::ensure_connected(s)?;
            let mut list: Vec<serde_json::Value> = match s.cache.get(key) {
                Some(cached) if matches!(cached.expires_at, Some(exp) if exp <= now_ms()) => {
                    Vec::new()
                }
                Some(cached) => match &cached.value {
                    serde_json::Value::Array(arr) => arr.clone(),
                    _ => Vec::new(),
                },
                None => Vec::new(),
            };
            list.push(value);
            if let Some(max) = max_length {
                if list.len() > max {
                    let start = list.len() - max;
                    list = list.split_off(start);
                }
            }
            s.cache.insert(
                key.to_string(),
                CachedValue {
                    expires_at: ttl_ms.map(|t| now_ms() + t),
                    value: serde_json::Value::Array(list),
                },
            );
            Ok(())
        })
    }

    /// Read a list. 1:1 port of upstream `getList<T>`. Returns the
    /// empty vector when missing, expired, or holding a non-array
    /// value.
    pub fn get_list(&self, key: &str) -> Result<Vec<serde_json::Value>, StateError> {
        self.with_state(|s| {
            Self::ensure_connected(s)?;
            let Some(cached) = s.cache.get(key) else {
                return Ok(Vec::new());
            };
            if matches!(cached.expires_at, Some(exp) if exp <= now_ms()) {
                s.cache.remove(key);
                return Ok(Vec::new());
            }
            if let serde_json::Value::Array(arr) = &cached.value {
                Ok(arr.clone())
            } else {
                Ok(Vec::new())
            }
        })
    }

    /// Enqueue a [`QueueEntry`] for a thread. 1:1 port of upstream
    /// `enqueue(threadId, entry, maxSize)`. Returns the new queue
    /// length. When the queue overflows past `max_size`, the oldest
    /// entries are dropped.
    pub fn enqueue(
        &self,
        thread_id: &str,
        entry: QueueEntry,
        max_size: usize,
    ) -> Result<usize, StateError> {
        self.with_state(|s| {
            Self::ensure_connected(s)?;
            let queue = s.queues.entry(thread_id.to_string()).or_default();
            queue.push(entry);
            if queue.len() > max_size {
                let drop_n = queue.len() - max_size;
                queue.drain(0..drop_n);
            }
            Ok(queue.len())
        })
    }

    /// Dequeue the oldest [`QueueEntry`]. 1:1 port of upstream
    /// `dequeue`. Returns `None` when the queue is empty or absent.
    pub fn dequeue(&self, thread_id: &str) -> Result<Option<QueueEntry>, StateError> {
        self.with_state(|s| {
            Self::ensure_connected(s)?;
            let Some(queue) = s.queues.get_mut(thread_id) else {
                return Ok(None);
            };
            if queue.is_empty() {
                return Ok(None);
            }
            let entry = queue.remove(0);
            if queue.is_empty() {
                s.queues.remove(thread_id);
            }
            Ok(Some(entry))
        })
    }

    /// Current queue depth for a thread. 1:1 port of upstream
    /// `queueDepth`.
    pub fn queue_depth(&self, thread_id: &str) -> Result<usize, StateError> {
        self.with_state(|s| {
            Self::ensure_connected(s)?;
            Ok(s.queues.get(thread_id).map(|q| q.len()).unwrap_or(0))
        })
    }

    /// Internal helper for upstream `_getSubscriptionCount`.
    pub fn subscription_count(&self) -> usize {
        self.with_state(|s| s.subscriptions.len())
    }

    /// Internal helper for upstream `_getLockCount`. Cleans expired
    /// locks before counting.
    pub fn lock_count(&self) -> usize {
        self.with_state(|s| {
            Self::clean_expired_locks(s);
            s.locks.len()
        })
    }
}

impl From<StateError> for chat_sdk_chat::types::StateAdapterError {
    fn from(err: StateError) -> Self {
        match err {
            StateError::NotConnected => Self::NotConnected,
        }
    }
}

/// `StateAdapter` impl for the in-memory backend. Phase 1.5 (slice 117).
///
/// The in-memory backend's internals are sync (no real I/O), so each
/// async-trait method here trivially awaits the matching synchronous
/// inherent method. The trait surface stays async because production
/// state backends (Redis, ioredis, Postgres) WILL perform I/O.
#[async_trait::async_trait]
impl chat_sdk_chat::types::StateAdapter for MemoryStateAdapter {
    async fn get(&self, key: &str) -> chat_sdk_chat::types::StateResult<Option<serde_json::Value>> {
        MemoryStateAdapter::get(self, key).map_err(Into::into)
    }

    async fn set(
        &self,
        key: &str,
        value: serde_json::Value,
        ttl_ms: Option<u64>,
    ) -> chat_sdk_chat::types::StateResult<()> {
        MemoryStateAdapter::set(self, key, value, ttl_ms).map_err(Into::into)
    }

    async fn delete(&self, key: &str) -> chat_sdk_chat::types::StateResult<()> {
        MemoryStateAdapter::delete(self, key).map_err(Into::into)
    }

    async fn append_to_list(
        &self,
        key: &str,
        value: serde_json::Value,
        max_length: Option<usize>,
        ttl_ms: Option<u64>,
    ) -> chat_sdk_chat::types::StateResult<()> {
        MemoryStateAdapter::append_to_list(self, key, value, max_length, ttl_ms).map_err(Into::into)
    }

    async fn get_list(
        &self,
        key: &str,
        limit: Option<usize>,
    ) -> chat_sdk_chat::types::StateResult<Vec<serde_json::Value>> {
        let list: Vec<serde_json::Value> = MemoryStateAdapter::get_list(self, key)
            .map_err(chat_sdk_chat::types::StateAdapterError::from)?;
        Ok(match limit {
            Some(n) if list.len() > n => list[list.len() - n..].to_vec(),
            _ => list,
        })
    }

    async fn set_if_not_exists(
        &self,
        key: &str,
        value: serde_json::Value,
        ttl_ms: Option<u64>,
    ) -> chat_sdk_chat::types::StateResult<bool> {
        MemoryStateAdapter::set_if_not_exists(self, key, value, ttl_ms).map_err(Into::into)
    }

    async fn acquire_lock(
        &self,
        thread_id: &str,
        ttl_ms: u64,
    ) -> chat_sdk_chat::types::StateResult<Option<chat_sdk_chat::types::Lock>> {
        MemoryStateAdapter::acquire_lock(self, thread_id, ttl_ms).map_err(Into::into)
    }

    async fn release_lock(
        &self,
        lock: &chat_sdk_chat::types::Lock,
    ) -> chat_sdk_chat::types::StateResult<()> {
        MemoryStateAdapter::release_lock(self, lock).map_err(Into::into)
    }

    async fn force_release_lock(&self, thread_id: &str) -> chat_sdk_chat::types::StateResult<()> {
        MemoryStateAdapter::force_release_lock(self, thread_id).map_err(Into::into)
    }

    async fn extend_lock(
        &self,
        lock: &chat_sdk_chat::types::Lock,
        ttl_ms: u64,
    ) -> chat_sdk_chat::types::StateResult<bool> {
        MemoryStateAdapter::extend_lock(self, lock, ttl_ms).map_err(Into::into)
    }

    async fn subscribe(&self, thread_id: &str) -> chat_sdk_chat::types::StateResult<()> {
        MemoryStateAdapter::subscribe(self, thread_id).map_err(Into::into)
    }

    async fn unsubscribe(&self, thread_id: &str) -> chat_sdk_chat::types::StateResult<()> {
        MemoryStateAdapter::unsubscribe(self, thread_id).map_err(Into::into)
    }

    async fn is_subscribed(&self, thread_id: &str) -> chat_sdk_chat::types::StateResult<bool> {
        MemoryStateAdapter::is_subscribed(self, thread_id).map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    //! 1:1 port of `packages/state-memory/src/index.test.ts`.
    //!
    //! Upstream uses Vitest's fake timers (`vi.useFakeTimers()`) for
    //! the "refresh TTL on appends" test; the Rust port uses real
    //! `std::thread::sleep` since the in-memory backend reads
    //! `SystemTime::now()` directly. The semantics tested (later writes
    //! reset the expiry) are identical.
    use super::*;
    use serde_json::json;
    use std::thread::sleep;
    use std::time::Duration;

    fn fresh() -> MemoryStateAdapter {
        let adapter = create_memory_state(None);
        adapter.connect().unwrap();
        adapter
    }

    // ---------- subscriptions ----------

    #[test]
    fn subscribe_should_subscribe_to_a_thread() {
        let adapter = fresh();
        adapter.subscribe("slack:C123:1234.5678").unwrap();
        assert!(adapter.is_subscribed("slack:C123:1234.5678").unwrap());
    }

    #[test]
    fn unsubscribe_should_unsubscribe_from_a_thread() {
        let adapter = fresh();
        adapter.subscribe("slack:C123:1234.5678").unwrap();
        adapter.unsubscribe("slack:C123:1234.5678").unwrap();
        assert!(!adapter.is_subscribed("slack:C123:1234.5678").unwrap());
    }

    // ---------- locking ----------

    #[test]
    fn locking_should_acquire_a_lock() {
        let adapter = fresh();
        let lock = adapter.acquire_lock("thread1", 5000).unwrap();
        let lock = lock.expect("expected lock to be Some");
        assert_eq!(lock.thread_id, "thread1");
        assert!(!lock.token.is_empty());
    }

    #[test]
    fn locking_should_prevent_double_locking() {
        let adapter = fresh();
        let lock1 = adapter.acquire_lock("thread1", 5000).unwrap();
        let lock2 = adapter.acquire_lock("thread1", 5000).unwrap();
        assert!(lock1.is_some());
        assert!(lock2.is_none());
    }

    #[test]
    fn locking_should_release_a_lock() {
        let adapter = fresh();
        let lock = adapter.acquire_lock("thread1", 5000).unwrap().unwrap();
        adapter.release_lock(&lock).unwrap();
        let lock2 = adapter.acquire_lock("thread1", 5000).unwrap();
        assert!(lock2.is_some());
    }

    #[test]
    fn locking_should_not_release_a_lock_with_wrong_token() {
        let adapter = fresh();
        let lock = adapter.acquire_lock("thread1", 5000).unwrap().unwrap();
        adapter
            .release_lock(&Lock {
                thread_id: "thread1".to_string(),
                token: "fake-token".to_string(),
                expires_at: now_ms() + 5000,
            })
            .unwrap();
        // Original lock should still be held.
        let lock2 = adapter.acquire_lock("thread1", 5000).unwrap();
        assert!(lock2.is_none());
        adapter.release_lock(&lock).unwrap();
    }

    #[test]
    fn locking_should_allow_re_locking_after_expiry() {
        let adapter = fresh();
        let lock1 = adapter.acquire_lock("thread1", 10).unwrap().unwrap();
        sleep(Duration::from_millis(25));
        let lock2 = adapter.acquire_lock("thread1", 5000).unwrap().unwrap();
        assert_ne!(lock2.token, lock1.token);
    }

    #[test]
    fn locking_should_extend_a_lock() {
        let adapter = fresh();
        let lock = adapter.acquire_lock("thread1", 100).unwrap().unwrap();
        let extended = adapter.extend_lock(&lock, 5000).unwrap();
        assert!(extended);
        let lock2 = adapter.acquire_lock("thread1", 5000).unwrap();
        assert!(lock2.is_none());
    }

    #[test]
    fn locking_should_force_release_regardless_of_token() {
        let adapter = fresh();
        let lock = adapter.acquire_lock("thread1", 5000).unwrap().unwrap();
        adapter.force_release_lock("thread1").unwrap();
        let lock2 = adapter.acquire_lock("thread1", 5000).unwrap().unwrap();
        assert_ne!(lock2.token, lock.token);
    }

    #[test]
    fn locking_should_no_op_when_force_releasing_a_nonexistent_lock() {
        let adapter = fresh();
        // Must not error.
        adapter.force_release_lock("nonexistent").unwrap();
    }

    #[test]
    fn locking_should_not_extend_an_expired_lock() {
        let adapter = fresh();
        let lock = adapter.acquire_lock("thread1", 10).unwrap().unwrap();
        sleep(Duration::from_millis(25));
        let extended = adapter.extend_lock(&lock, 5000).unwrap();
        assert!(!extended);
    }

    // ---------- setIfNotExists ----------

    #[test]
    fn set_if_not_exists_should_set_when_key_does_not_exist() {
        let adapter = fresh();
        let result = adapter
            .set_if_not_exists("key1", json!("value1"), None)
            .unwrap();
        assert!(result);
        assert_eq!(adapter.get("key1").unwrap(), Some(json!("value1")));
    }

    #[test]
    fn set_if_not_exists_should_not_overwrite_an_existing_key() {
        let adapter = fresh();
        adapter
            .set_if_not_exists("key1", json!("first"), None)
            .unwrap();
        let result = adapter
            .set_if_not_exists("key1", json!("second"), None)
            .unwrap();
        assert!(!result);
        assert_eq!(adapter.get("key1").unwrap(), Some(json!("first")));
    }

    #[test]
    fn set_if_not_exists_should_allow_setting_after_ttl_expiry() {
        let adapter = fresh();
        adapter
            .set_if_not_exists("key1", json!("first"), Some(10))
            .unwrap();
        sleep(Duration::from_millis(25));
        let result = adapter
            .set_if_not_exists("key1", json!("second"), None)
            .unwrap();
        assert!(result);
        assert_eq!(adapter.get("key1").unwrap(), Some(json!("second")));
    }

    #[test]
    fn set_if_not_exists_should_respect_ttl_on_the_new_value() {
        let adapter = fresh();
        adapter
            .set_if_not_exists("key1", json!("value"), Some(10))
            .unwrap();
        sleep(Duration::from_millis(25));
        assert_eq!(adapter.get("key1").unwrap(), None);
    }

    // ---------- appendToList / getList ----------

    #[test]
    fn append_to_list_should_append_and_retrieve_items() {
        let adapter = fresh();
        adapter
            .append_to_list("list1", json!({"id": 1}), None, None)
            .unwrap();
        adapter
            .append_to_list("list1", json!({"id": 2}), None, None)
            .unwrap();
        let result = adapter.get_list("list1").unwrap();
        assert_eq!(result, vec![json!({"id": 1}), json!({"id": 2})]);
    }

    #[test]
    fn get_list_should_return_empty_for_nonexistent_list() {
        let adapter = fresh();
        let result = adapter.get_list("nonexistent").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn append_to_list_should_trim_to_max_length_keeping_newest() {
        let adapter = fresh();
        for i in 1..=5 {
            adapter
                .append_to_list("list1", json!({"id": i}), Some(3), None)
                .unwrap();
        }
        let result = adapter.get_list("list1").unwrap();
        assert_eq!(
            result,
            vec![json!({"id": 3}), json!({"id": 4}), json!({"id": 5})]
        );
    }

    #[test]
    fn append_to_list_should_respect_ttl() {
        let adapter = fresh();
        adapter
            .append_to_list("list1", json!({"id": 1}), None, Some(10))
            .unwrap();
        sleep(Duration::from_millis(25));
        let result = adapter.get_list("list1").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn append_to_list_should_refresh_ttl_on_subsequent_appends() {
        let adapter = fresh();
        adapter
            .append_to_list("list1", json!({"id": 1}), None, Some(50))
            .unwrap();
        sleep(Duration::from_millis(25));
        adapter
            .append_to_list("list1", json!({"id": 2}), None, Some(50))
            .unwrap();
        // After 25ms more, total elapsed since first append is ~50ms
        // but the refresh sets a new 50ms window, so list survives.
        sleep(Duration::from_millis(25));
        let result = adapter.get_list("list1").unwrap();
        assert_eq!(result, vec![json!({"id": 1}), json!({"id": 2})]);
    }

    #[test]
    fn append_to_list_keeps_lists_isolated_by_key() {
        let adapter = fresh();
        adapter
            .append_to_list("list-a", json!("a"), None, None)
            .unwrap();
        adapter
            .append_to_list("list-b", json!("b"), None, None)
            .unwrap();
        assert_eq!(adapter.get_list("list-a").unwrap(), vec![json!("a")]);
        assert_eq!(adapter.get_list("list-b").unwrap(), vec![json!("b")]);
    }

    #[test]
    fn append_to_list_should_start_fresh_after_expired_list() {
        let adapter = fresh();
        adapter
            .append_to_list("list1", json!({"id": 1}), None, Some(10))
            .unwrap();
        sleep(Duration::from_millis(25));
        adapter
            .append_to_list("list1", json!({"id": 2}), None, None)
            .unwrap();
        let result = adapter.get_list("list1").unwrap();
        assert_eq!(result, vec![json!({"id": 2})]);
    }

    // ---------- enqueue / dequeue / queueDepth ----------

    fn entry(id: &str, enqueued_at: u64) -> QueueEntry {
        QueueEntry {
            message: json!({"id": id}),
            enqueued_at,
            expires_at: now_ms() + 90_000,
        }
    }

    #[test]
    fn enqueue_dequeue_handles_single_entry() {
        let adapter = fresh();
        let e = entry("m1", 1000);
        let depth = adapter.enqueue("thread1", e.clone(), 10).unwrap();
        assert_eq!(depth, 1);
        let result = adapter.dequeue("thread1").unwrap();
        assert_eq!(result, Some(e));
    }

    #[test]
    fn dequeue_returns_none_when_queue_is_empty() {
        let adapter = fresh();
        let result = adapter.dequeue("thread1").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn dequeue_returns_none_for_nonexistent_thread() {
        let adapter = fresh();
        let result = adapter.dequeue("nonexistent").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn queue_depth_returns_zero_for_empty_queue() {
        let adapter = fresh();
        assert_eq!(adapter.queue_depth("thread1").unwrap(), 0);
    }

    #[test]
    fn dequeue_returns_entries_in_fifo_order() {
        let adapter = fresh();
        let e1 = entry("m1", 1000);
        let e2 = entry("m2", 2000);
        let e3 = entry("m3", 3000);
        adapter.enqueue("thread1", e1.clone(), 10).unwrap();
        adapter.enqueue("thread1", e2.clone(), 10).unwrap();
        adapter.enqueue("thread1", e3.clone(), 10).unwrap();
        assert_eq!(adapter.queue_depth("thread1").unwrap(), 3);
        assert_eq!(
            adapter.dequeue("thread1").unwrap().unwrap().message,
            json!({"id": "m1"})
        );
        assert_eq!(
            adapter.dequeue("thread1").unwrap().unwrap().message,
            json!({"id": "m2"})
        );
        assert_eq!(
            adapter.dequeue("thread1").unwrap().unwrap().message,
            json!({"id": "m3"})
        );
        assert_eq!(adapter.dequeue("thread1").unwrap(), None);
        assert_eq!(adapter.queue_depth("thread1").unwrap(), 0);
    }

    #[test]
    fn enqueue_trims_to_max_size_keeping_newest() {
        let adapter = fresh();
        for i in 1..=5 {
            adapter
                .enqueue("thread1", entry(&format!("m{i}"), i * 1000), 3)
                .unwrap();
        }
        assert_eq!(adapter.queue_depth("thread1").unwrap(), 3);
        assert_eq!(
            adapter.dequeue("thread1").unwrap().unwrap().message,
            json!({"id": "m3"})
        );
        assert_eq!(
            adapter.dequeue("thread1").unwrap().unwrap().message,
            json!({"id": "m4"})
        );
        assert_eq!(
            adapter.dequeue("thread1").unwrap().unwrap().message,
            json!({"id": "m5"})
        );
    }

    #[test]
    fn enqueue_handles_max_size_of_one_debounce_behavior() {
        let adapter = fresh();
        adapter.enqueue("thread1", entry("m1", 1000), 1).unwrap();
        adapter.enqueue("thread1", entry("m2", 2000), 1).unwrap();
        adapter.enqueue("thread1", entry("m3", 3000), 1).unwrap();
        assert_eq!(adapter.queue_depth("thread1").unwrap(), 1);
        assert_eq!(
            adapter.dequeue("thread1").unwrap().unwrap().message,
            json!({"id": "m3"})
        );
    }

    #[test]
    fn enqueue_keeps_queues_isolated_by_thread() {
        let adapter = fresh();
        adapter.enqueue("thread-a", entry("a1", 1000), 10).unwrap();
        adapter.enqueue("thread-b", entry("b1", 1000), 10).unwrap();
        assert_eq!(adapter.queue_depth("thread-a").unwrap(), 1);
        assert_eq!(adapter.queue_depth("thread-b").unwrap(), 1);
        assert_eq!(
            adapter.dequeue("thread-a").unwrap().unwrap().message,
            json!({"id": "a1"})
        );
        assert_eq!(
            adapter.dequeue("thread-b").unwrap().unwrap().message,
            json!({"id": "b1"})
        );
    }

    #[test]
    fn disconnect_clears_queues_and_reconnect_starts_empty() {
        let adapter = fresh();
        adapter.enqueue("thread1", entry("m1", 1000), 10).unwrap();
        adapter.disconnect().unwrap();
        adapter.connect().unwrap();
        assert_eq!(adapter.queue_depth("thread1").unwrap(), 0);
        assert_eq!(adapter.dequeue("thread1").unwrap(), None);
    }

    // ---------- connection ----------

    #[test]
    fn methods_error_when_adapter_is_not_connected() {
        let adapter = create_memory_state(None);
        let err = adapter.subscribe("test").unwrap_err();
        assert_eq!(err, StateError::NotConnected);
        assert!(err.to_string().contains("not connected"));
    }

    #[test]
    fn disconnect_clears_subscriptions_and_locks_so_reconnect_starts_fresh() {
        let adapter = fresh();
        adapter.subscribe("thread1").unwrap();
        adapter.acquire_lock("thread1", 5000).unwrap();
        adapter.disconnect().unwrap();
        adapter.connect().unwrap();
        assert!(!adapter.is_subscribed("thread1").unwrap());
        let lock = adapter.acquire_lock("thread1", 5000).unwrap();
        assert!(lock.is_some());
    }
}
