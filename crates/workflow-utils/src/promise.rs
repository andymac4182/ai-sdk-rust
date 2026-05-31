use std::{
    fmt,
    sync::{
        Mutex, OnceLock,
        mpsc::{self, Receiver, Sender},
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromiseSendError;

impl fmt::Display for PromiseSendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("promise resolver was already consumed")
    }
}

impl std::error::Error for PromiseSendError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromiseRecvError;

impl fmt::Display for PromiseRecvError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("promise channel closed before resolution")
    }
}

impl std::error::Error for PromiseRecvError {}

pub struct PromiseWithResolvers<T, E = String> {
    sender: Option<Sender<Result<T, E>>>,
    receiver: Receiver<Result<T, E>>,
}

impl<T, E> PromiseWithResolvers<T, E> {
    pub fn resolve(&mut self, value: T) -> Result<(), PromiseSendError> {
        self.send(Ok(value))
    }

    pub fn reject(&mut self, reason: E) -> Result<(), PromiseSendError> {
        self.send(Err(reason))
    }

    pub fn recv(self) -> Result<Result<T, E>, PromiseRecvError> {
        self.receiver.recv().map_err(|_| PromiseRecvError)
    }

    pub fn has_resolvers(&self) -> bool {
        self.sender.is_some()
    }

    fn send(&mut self, result: Result<T, E>) -> Result<(), PromiseSendError> {
        let sender = self.sender.take().ok_or(PromiseSendError)?;
        sender.send(result).map_err(|_| PromiseSendError)
    }
}

pub fn with_resolvers<T, E>() -> PromiseWithResolvers<T, E> {
    let (sender, receiver) = mpsc::channel();
    PromiseWithResolvers {
        sender: Some(sender),
        receiver,
    }
}

pub struct OnceValue<T, F>
where
    F: FnOnce() -> T,
{
    value: OnceLock<T>,
    initializer: Mutex<Option<F>>,
}

impl<T, F> OnceValue<T, F>
where
    F: FnOnce() -> T,
{
    pub fn value(&self) -> &T {
        self.value.get_or_init(|| {
            let initializer = self
                .initializer
                .lock()
                .expect("once initializer mutex poisoned")
                .take()
                .expect("once initializer was already consumed");
            initializer()
        })
    }
}

pub fn once<T, F>(initializer: F) -> OnceValue<T, F>
where
    F: FnOnce() -> T,
{
    OnceValue {
        value: OnceLock::new(),
        initializer: Mutex::new(Some(initializer)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn upstream_promise_cases() {
        let created = with_resolvers::<String, String>();
        assert!(created.has_resolvers());

        let mut resolved = with_resolvers::<String, String>();
        resolved.resolve("test".to_owned()).unwrap();
        assert_eq!(resolved.recv().unwrap().unwrap(), "test");

        let mut rejected = with_resolvers::<String, String>();
        rejected.reject("test error".to_owned()).unwrap();
        assert_eq!(rejected.recv().unwrap().unwrap_err(), "test error");

        let call_count = AtomicUsize::new(0);
        let memoized = once(|| {
            call_count.fetch_add(1, Ordering::SeqCst);
            "result".to_owned()
        });
        assert_eq!(memoized.value(), "result");
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
        assert_eq!(memoized.value(), "result");
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        let cached = once(std::time::SystemTime::now);
        let first = *cached.value();
        let second = *cached.value();
        assert_eq!(first, second);
    }
}
