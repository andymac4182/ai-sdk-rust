/// Polling interval mirrored from upstream `flushable-stream.ts`.
pub const LOCK_POLL_INTERVAL_MS: u64 = 10;

/// Deterministic state tracker for flushable stream operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlushableStreamState {
    pending_ops: usize,
    done_resolved: bool,
    stream_ended: bool,
    writable_polling: bool,
    readable_polling: bool,
    writable_locked: bool,
    readable_locked: bool,
    rejected: Option<String>,
    cancel_reason: Option<String>,
}

impl Default for FlushableStreamState {
    fn default() -> Self {
        Self::new()
    }
}

impl FlushableStreamState {
    pub fn new() -> Self {
        Self {
            pending_ops: 0,
            done_resolved: false,
            stream_ended: false,
            writable_polling: false,
            readable_polling: false,
            writable_locked: true,
            readable_locked: true,
            rejected: None,
            cancel_reason: None,
        }
    }

    pub fn pending_ops(&self) -> usize {
        self.pending_ops
    }

    pub fn is_done(&self) -> bool {
        self.done_resolved
    }

    pub fn stream_ended(&self) -> bool {
        self.stream_ended
    }

    pub fn rejection(&self) -> Option<&str> {
        self.rejected.as_deref()
    }

    pub fn cancel_reason(&self) -> Option<&str> {
        self.cancel_reason.as_deref()
    }

    pub fn writable_polling_active(&self) -> bool {
        self.writable_polling
    }

    pub fn readable_polling_active(&self) -> bool {
        self.readable_polling
    }

    pub fn begin_write(&mut self) {
        self.pending_ops += 1;
    }

    pub fn finish_write(&mut self) {
        self.pending_ops = self.pending_ops.saturating_sub(1);
        self.resolve_if_flushable();
    }

    pub fn release_writable_lock(&mut self) {
        self.writable_locked = false;
        self.resolve_if_flushable();
    }

    pub fn release_readable_lock(&mut self) {
        self.readable_locked = false;
        self.resolve_if_flushable();
    }

    pub fn poll_writable_lock(&mut self) {
        if self.done_resolved || self.stream_ended || self.writable_polling {
            return;
        }
        self.writable_polling = true;
        self.resolve_if_flushable();
    }

    pub fn poll_readable_lock(&mut self) {
        if self.done_resolved || self.stream_ended || self.readable_polling {
            return;
        }
        self.readable_polling = true;
        self.resolve_if_flushable();
    }

    pub fn close_stream(&mut self) {
        self.stream_ended = true;
        self.writable_polling = false;
        self.readable_polling = false;
        self.resolve_done();
    }

    pub fn fail_stream(&mut self, reason: impl Into<String>) {
        let reason = reason.into();
        self.stream_ended = true;
        self.writable_polling = false;
        self.readable_polling = false;
        self.cancel_reason = Some(reason.clone());
        if !self.done_resolved {
            self.done_resolved = true;
            self.rejected = Some(reason);
        }
    }

    fn resolve_if_flushable(&mut self) {
        if self.stream_ended || self.done_resolved || self.pending_ops != 0 {
            return;
        }
        if (self.writable_polling && !self.writable_locked)
            || (self.readable_polling && !self.readable_locked)
        {
            self.resolve_done();
        }
    }

    fn resolve_done(&mut self) {
        if !self.done_resolved {
            self.done_resolved = true;
        }
    }
}
