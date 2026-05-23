//! 1:1 port of `packages/chat/src/errors.test.ts` from upstream
//! `vercel/chat` @ `aba6aa94fe5a2ed909ec4daa7db0e21887507fa4`.
//!
//! Each Rust test name mirrors the original `it(...)` description. The
//! upstream `expect(err).toBeInstanceOf(...)` assertions become `matches!`
//! checks against the `ChatError` enum variants and `is_*` helpers.

use std::error::Error as _;

use chat_sdk_chat::errors::ChatError;

// describe("ChatError")

#[test]
fn chat_error_should_set_message_code_and_name() {
    let err = ChatError::new("something broke", "SOME_CODE");
    assert_eq!(err.message(), "something broke");
    assert_eq!(err.code(), "SOME_CODE");
    assert_eq!(err.name(), "ChatError");
}

#[test]
fn chat_error_should_be_instanceof_error() {
    // Rust equivalent of `instanceof Error && instanceof ChatError`:
    // the type itself implements `std::error::Error`, and the value is a
    // `ChatError`, so both assertions hold structurally.
    let err = ChatError::new("fail", "ERR");
    let _as_error: &dyn std::error::Error = &err;
    assert!(matches!(err, ChatError::Base { .. }));
}

#[test]
fn chat_error_should_propagate_cause() {
    let cause: Box<dyn std::error::Error + Send + Sync> =
        Box::new(std::io::Error::other("root cause"));
    let err = ChatError::with_cause("wrapped", "WRAP", cause);
    assert!(err.source().is_some());
    assert_eq!(err.source().unwrap().to_string(), "root cause");
}

#[test]
fn chat_error_should_allow_undefined_cause() {
    let err = ChatError::new("no cause", "NC");
    assert!(err.source().is_none());
}

// describe("RateLimitError")

#[test]
fn rate_limit_error_should_set_code_to_rate_limited() {
    let err = ChatError::rate_limit("slow down");
    assert_eq!(err.code(), "RATE_LIMITED");
    assert_eq!(err.name(), "RateLimitError");
}

#[test]
fn rate_limit_error_should_store_retry_after_ms() {
    let err = ChatError::rate_limit_after("slow down", 5000);
    assert_eq!(err.retry_after_ms(), Some(5000));
}

#[test]
fn rate_limit_error_should_allow_undefined_retry_after_ms() {
    let err = ChatError::rate_limit("slow down");
    assert_eq!(err.retry_after_ms(), None);
}

#[test]
fn rate_limit_error_should_be_instanceof_chat_error_and_error() {
    let err = ChatError::rate_limit("slow down");
    assert!(err.is_rate_limit());
    // Subclass-of-ChatError check: the type IS ChatError in Rust's enum model,
    // so all variants are unconditionally ChatError.
    let _as_error: &dyn std::error::Error = &err;
}

#[test]
fn rate_limit_error_should_propagate_cause() {
    let cause: Box<dyn std::error::Error + Send + Sync> =
        Box::new(std::io::Error::other("api error"));
    let err = ChatError::rate_limit_after("rate limited", 1000).with_source(cause);
    assert!(err.source().is_some());
    assert_eq!(err.source().unwrap().to_string(), "api error");
}

// describe("LockError")

#[test]
fn lock_error_should_set_code_to_lock_failed() {
    let err = ChatError::lock("lock failed");
    assert_eq!(err.code(), "LOCK_FAILED");
    assert_eq!(err.name(), "LockError");
}

#[test]
fn lock_error_should_be_instanceof_chat_error() {
    let err = ChatError::lock("lock failed");
    assert!(err.is_lock());
}

#[test]
fn lock_error_should_propagate_cause() {
    let cause: Box<dyn std::error::Error + Send + Sync> =
        Box::new(std::io::Error::other("redis down"));
    let err = ChatError::lock("lock failed").with_source(cause);
    assert!(err.source().is_some());
    assert_eq!(err.source().unwrap().to_string(), "redis down");
}

// describe("NotImplementedError")

#[test]
fn not_implemented_error_should_set_code_to_not_implemented() {
    let err = ChatError::not_implemented("not yet");
    assert_eq!(err.code(), "NOT_IMPLEMENTED");
    assert_eq!(err.name(), "NotImplementedError");
}

#[test]
fn not_implemented_error_should_store_feature_field() {
    let err = ChatError::not_implemented_feature("not yet", "reactions");
    assert_eq!(err.feature(), Some("reactions"));
}

#[test]
fn not_implemented_error_should_allow_undefined_feature() {
    let err = ChatError::not_implemented("not yet");
    assert_eq!(err.feature(), None);
}

#[test]
fn not_implemented_error_should_be_instanceof_chat_error() {
    let err = ChatError::not_implemented("not yet");
    assert!(err.is_not_implemented());
}

#[test]
fn not_implemented_error_should_propagate_cause() {
    let cause: Box<dyn std::error::Error + Send + Sync> =
        Box::new(std::io::Error::other("underlying"));
    let err = ChatError::not_implemented_feature("not yet", "modals").with_source(cause);
    assert!(err.source().is_some());
    assert_eq!(err.source().unwrap().to_string(), "underlying");
}
