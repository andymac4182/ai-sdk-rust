use std::thread;
use std::time::Duration;

/// Result of a replayed event after any async deserialization work completes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayResolution<T, E> {
    Fulfilled(T),
    Rejected(E),
    Sleep,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayWork<T, E> {
    pub log_index: usize,
    pub delay: Duration,
    pub resolution: ReplayResolution<T, E>,
}

/// Resolve work items concurrently while publishing results in event-log order.
///
/// Upstream uses this guarantee to keep workflow replay deterministic when
/// decryption or other async hydration work finishes out of order.
pub fn resolve_in_event_log_order<T, E>(items: Vec<ReplayWork<T, E>>) -> Vec<ReplayResolution<T, E>>
where
    T: Send + 'static,
    E: Send + 'static,
{
    let mut handles = items
        .into_iter()
        .map(|item| {
            thread::spawn(move || {
                thread::sleep(item.delay);
                (item.log_index, item.resolution)
            })
        })
        .collect::<Vec<_>>();

    let mut resolved = Vec::with_capacity(handles.len());
    for handle in handles.drain(..) {
        resolved.push(handle.join().expect("worker thread panicked"));
    }
    resolved.sort_by_key(|(log_index, _)| *log_index);
    resolved
        .into_iter()
        .map(|(_, resolution)| resolution)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work(
        log_index: usize,
        delay_ms: u64,
        value: impl Into<String>,
    ) -> ReplayWork<String, String> {
        ReplayWork {
            log_index,
            delay: Duration::from_millis(delay_ms),
            resolution: ReplayResolution::Fulfilled(value.into()),
        }
    }

    #[test]
    fn async_deserialization_ordering_test_resolves_step_promises_in_event_log_order() {
        let resolved =
            resolve_in_event_log_order(vec![work(0, 50, "A:result_A"), work(1, 5, "B:result_B")]);
        assert_eq!(
            resolved,
            vec![
                ReplayResolution::Fulfilled("A:result_A".into()),
                ReplayResolution::Fulfilled("B:result_B".into())
            ]
        );
    }

    #[test]
    fn async_deserialization_ordering_test_resolves_sequential_step_promises_in_order() {
        let resolved = resolve_in_event_log_order(vec![
            work(0, 60, "10"),
            work(1, 30, "20"),
            work(2, 5, "30"),
        ]);
        assert_eq!(
            resolved,
            vec![
                ReplayResolution::Fulfilled("10".into()),
                ReplayResolution::Fulfilled("20".into()),
                ReplayResolution::Fulfilled("30".into())
            ]
        );
    }

    #[test]
    fn async_deserialization_ordering_test_resolves_hook_payloads_in_event_log_order() {
        let resolved =
            resolve_in_event_log_order(vec![work(0, 50, "A:first"), work(1, 5, "B:second")]);
        assert_eq!(
            resolved,
            vec![
                ReplayResolution::Fulfilled("A:first".into()),
                ReplayResolution::Fulfilled("B:second".into())
            ]
        );
    }

    #[test]
    fn async_deserialization_ordering_test_resolves_mixed_step_completed_and_failed_in_order() {
        let resolved = resolve_in_event_log_order(vec![
            work(0, 50, "A:success_A"),
            ReplayWork {
                log_index: 1,
                delay: Duration::from_millis(0),
                resolution: ReplayResolution::Rejected("B:step B failed".into()),
            },
            work(2, 5, "C:success_C"),
        ]);
        assert_eq!(
            resolved,
            vec![
                ReplayResolution::Fulfilled("A:success_A".into()),
                ReplayResolution::Rejected("B:step B failed".into()),
                ReplayResolution::Fulfilled("C:success_C".into())
            ]
        );
    }

    #[test]
    fn async_deserialization_ordering_test_handles_many_concurrent_steps_in_correct_order() {
        let items = (0..10)
            .map(|index| work(index, (10 - index) as u64, index.to_string()))
            .collect();
        let resolved = resolve_in_event_log_order(items);
        assert_eq!(
            resolved,
            (0..10)
                .map(|index| ReplayResolution::Fulfilled(index.to_string()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn async_deserialization_ordering_test_resolves_sleep_and_step_promises_in_event_log_order() {
        let resolved = resolve_in_event_log_order(vec![
            work(0, 50, "step:step_result"),
            ReplayWork {
                log_index: 1,
                delay: Duration::from_millis(0),
                resolution: ReplayResolution::Sleep,
            },
            work(2, 5, "step:after_sleep"),
        ]);
        assert_eq!(
            resolved,
            vec![
                ReplayResolution::Fulfilled("step:step_result".into()),
                ReplayResolution::Sleep,
                ReplayResolution::Fulfilled("step:after_sleep".into())
            ]
        );
    }

    #[test]
    fn async_deserialization_ordering_test_resolves_interleaved_step_functions_in_event_log_order()
    {
        let resolved =
            resolve_in_event_log_order(vec![work(2, 50, "A:value_A"), work(3, 5, "B:value_B")]);
        assert_eq!(
            resolved,
            vec![
                ReplayResolution::Fulfilled("A:value_A".into()),
                ReplayResolution::Fulfilled("B:value_B".into())
            ]
        );
    }
}
