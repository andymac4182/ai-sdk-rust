use crate::Event;

/// Result returned by an [`EventsConsumer`] subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventConsumerResult {
    /// The callback consumed the event and remains subscribed.
    Consumed,
    /// The callback did not consume the event; try the next callback.
    NotConsumed,
    /// The callback consumed the event and should be removed.
    Finished,
    /// Test seam for callback failures. The consumer logs and continues.
    Errored,
}

type EventConsumerCallback = Box<dyn FnMut(Option<&Event>) -> EventConsumerResult>;

/// Deterministic Rust equivalent of upstream `EventsConsumer`.
///
/// JavaScript schedules consumption with `process.nextTick()` and defers the
/// orphan-event check until promise queues drain. The Rust port exposes those
/// queues as explicit, deterministic method calls: `subscribe()` immediately
/// drains consumable events, and `flush_deferred_unconsumed_check()` advances
/// the deferred orphan check.
pub struct EventsConsumer {
    event_index: usize,
    events: Vec<Event>,
    callbacks: Vec<EventConsumerCallback>,
    callback_errors: Vec<String>,
    unconsumed_events: Vec<Event>,
    pending_unconsumed: Option<(u64, Event)>,
    unconsumed_check_version: u64,
}

impl EventsConsumer {
    pub fn new(events: Vec<Event>) -> Self {
        Self {
            event_index: 0,
            events,
            callbacks: Vec::new(),
            callback_errors: Vec::new(),
            unconsumed_events: Vec::new(),
            pending_unconsumed: None,
            unconsumed_check_version: 0,
        }
    }

    pub fn event_index(&self) -> usize {
        self.event_index
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn callback_count(&self) -> usize {
        self.callbacks.len()
    }

    pub fn callback_errors(&self) -> &[String] {
        &self.callback_errors
    }

    pub fn unconsumed_events(&self) -> &[Event] {
        &self.unconsumed_events
    }

    pub fn subscribe<F>(&mut self, callback: F)
    where
        F: FnMut(Option<&Event>) -> EventConsumerResult + 'static,
    {
        self.callbacks.push(Box::new(callback));
        if self.pending_unconsumed.is_some() {
            self.unconsumed_check_version += 1;
            self.pending_unconsumed = None;
        }
        self.consume();
    }

    pub fn flush_deferred_unconsumed_check(&mut self) {
        let Some((version, event)) = self.pending_unconsumed.take() else {
            return;
        };
        if version == self.unconsumed_check_version {
            self.unconsumed_events.push(event);
        }
    }

    fn consume(&mut self) {
        loop {
            let current_event = self.events.get(self.event_index);
            let mut index = 0;
            let mut consumed = false;

            while index < self.callbacks.len() {
                let handled = {
                    let callback = &mut self.callbacks[index];
                    callback(current_event)
                };

                match handled {
                    EventConsumerResult::Consumed => {
                        self.event_index += 1;
                        consumed = true;
                        break;
                    }
                    EventConsumerResult::Finished => {
                        self.event_index += 1;
                        let _ = self.callbacks.remove(index);
                        consumed = true;
                        break;
                    }
                    EventConsumerResult::NotConsumed => {
                        index += 1;
                    }
                    EventConsumerResult::Errored => {
                        self.callback_errors
                            .push("EventConsumer callback threw an error".to_string());
                        index += 1;
                    }
                }
            }

            if consumed {
                continue;
            }

            if let Some(event) = current_event {
                let version = self.unconsumed_check_version + 1;
                self.unconsumed_check_version = version;
                self.pending_unconsumed = Some((version, event.clone()));
            }
            break;
        }
    }
}
