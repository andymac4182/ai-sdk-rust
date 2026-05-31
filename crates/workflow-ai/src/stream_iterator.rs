use std::collections::VecDeque;
use std::error::Error;
use std::fmt;

/// Runtime environment used by [`iterator_to_stream`] for browser-yield parity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamIteratorEnvironment {
    /// Browser-equivalent environment; draining records a yield between chunks.
    Browser,

    /// Server-equivalent environment; draining does not record browser yields.
    Server,
}

/// Error returned by the portable stream iterator helpers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamIteratorError {
    /// The stream was aborted before or during consumption.
    Aborted(String),

    /// The underlying iterator failed.
    Iterator(String),
}

impl fmt::Display for StreamIteratorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Aborted(reason) => write!(formatter, "stream aborted: {reason}"),
            Self::Iterator(error) => formatter.write_str(error),
        }
    }
}

impl Error for StreamIteratorError {}

/// Portable, deterministic equivalent of upstream `iteratorToStream`.
#[derive(Clone, Debug)]
pub struct IteratorStream<T> {
    items: VecDeque<Result<T, StreamIteratorError>>,
    environment: StreamIteratorEnvironment,
    abort_reason: Option<String>,
    returned: bool,
    browser_yield_count: usize,
}

impl<T> IteratorStream<T> {
    /// Creates a stream from already-normalized iterator items.
    pub fn new(
        items: impl IntoIterator<Item = Result<T, StreamIteratorError>>,
        environment: StreamIteratorEnvironment,
    ) -> Self {
        Self {
            items: items.into_iter().collect(),
            environment,
            abort_reason: None,
            returned: false,
            browser_yield_count: 0,
        }
    }

    /// Creates a stream that has already been aborted.
    pub fn already_aborted(reason: impl Into<String>) -> Self {
        Self {
            items: VecDeque::new(),
            environment: StreamIteratorEnvironment::Server,
            abort_reason: Some(reason.into()),
            returned: true,
            browser_yield_count: 0,
        }
    }

    /// Aborts the stream and marks the underlying iterator as returned.
    pub fn abort(&mut self, reason: impl Into<String>) {
        self.abort_reason = Some(reason.into());
        self.items.clear();
        self.returned = true;
    }

    /// Cancels the stream and marks the underlying iterator as returned.
    pub fn cancel(&mut self) {
        self.items.clear();
        self.returned = true;
    }

    /// Reads the next stream item.
    pub fn next(&mut self) -> Result<Option<T>, StreamIteratorError> {
        if let Some(reason) = &self.abort_reason {
            return Err(StreamIteratorError::Aborted(reason.clone()));
        }

        let Some(item) = self.items.pop_front() else {
            self.returned = true;
            return Ok(None);
        };

        let value = item?;
        if self.environment == StreamIteratorEnvironment::Browser {
            self.browser_yield_count += 1;
        }
        Ok(Some(value))
    }

    /// Drains the stream into a vector.
    pub fn collect(mut self) -> Result<Vec<T>, StreamIteratorError> {
        let mut values = Vec::new();
        while let Some(value) = self.next()? {
            values.push(value);
        }
        Ok(values)
    }

    /// Number of browser-yield checkpoints observed while draining chunks.
    pub fn browser_yield_count(&self) -> usize {
        self.browser_yield_count
    }

    /// Whether the underlying iterator has been returned or completed.
    pub fn returned(&self) -> bool {
        self.returned
    }
}

/// Converts an iterator into a portable stream.
pub fn iterator_to_stream<T>(
    iterator: impl IntoIterator<Item = T>,
    environment: StreamIteratorEnvironment,
) -> IteratorStream<T> {
    IteratorStream::new(iterator.into_iter().map(Ok), environment)
}

/// Converts a portable stream back into an iterator-shaped vector.
pub fn stream_to_iterator<T>(stream: IteratorStream<T>) -> Result<Vec<T>, StreamIteratorError> {
    stream.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iterator_to_stream_upstream_should_convert_async_generator_to_readable_stream() {
        let stream = iterator_to_stream([1, 2, 3], StreamIteratorEnvironment::Server);

        assert_eq!(
            stream_to_iterator(stream).expect("stream drains"),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn iterator_to_stream_upstream_should_yield_to_macrotask_queue_between_chunks_in_browser() {
        let mut stream = iterator_to_stream(["a", "b"], StreamIteratorEnvironment::Browser);

        assert_eq!(stream.next().expect("first chunk"), Some("a"));
        assert_eq!(stream.browser_yield_count(), 1);
        assert_eq!(stream.next().expect("second chunk"), Some("b"));
        assert_eq!(stream.browser_yield_count(), 2);
    }

    #[test]
    fn iterator_to_stream_upstream_should_skip_macrotask_yield_in_non_browser_environments() {
        let mut stream = iterator_to_stream(["a", "b"], StreamIteratorEnvironment::Server);

        assert_eq!(stream.next().expect("first chunk"), Some("a"));
        assert_eq!(stream.next().expect("second chunk"), Some("b"));
        assert_eq!(stream.browser_yield_count(), 0);
    }

    #[test]
    fn iterator_to_stream_upstream_should_handle_abort_signal() {
        let mut stream = iterator_to_stream([1, 2], StreamIteratorEnvironment::Server);

        stream.abort("Aborted");

        assert_eq!(
            stream.next().expect_err("aborted"),
            StreamIteratorError::Aborted("Aborted".to_string())
        );
        assert!(stream.returned());
    }

    #[test]
    fn iterator_to_stream_upstream_should_handle_already_aborted_signal() {
        let mut stream = IteratorStream::<i32>::already_aborted("Aborted");

        assert_eq!(
            stream.next().expect_err("aborted"),
            StreamIteratorError::Aborted("Aborted".to_string())
        );
        assert!(stream.returned());
    }

    #[test]
    fn iterator_to_stream_upstream_should_propagate_generator_errors() {
        let mut stream = IteratorStream::new(
            [
                Ok("first"),
                Err(StreamIteratorError::Iterator(
                    "generator failed".to_string(),
                )),
            ],
            StreamIteratorEnvironment::Server,
        );

        assert_eq!(stream.next().expect("first item"), Some("first"));
        assert_eq!(
            stream.next().expect_err("generator error"),
            StreamIteratorError::Iterator("generator failed".to_string())
        );
    }

    #[test]
    fn stream_to_iterator_upstream_should_convert_readable_stream_to_async_iterator() {
        let stream = iterator_to_stream(["x", "y"], StreamIteratorEnvironment::Server);

        assert_eq!(
            stream_to_iterator(stream).expect("stream drains"),
            vec!["x", "y"]
        );
    }
}
