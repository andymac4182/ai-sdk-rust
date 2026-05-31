//! World interface crate for the standalone Vercel Workflow SDK Rust port.
//!
//! This crate maps to upstream `packages/world`. It owns cross-world run, step,
//! event, queue, hook, wait, stream, serialization, and World trait contracts.
//! Local, Postgres, and Vercel worlds implement these traits in their own crates.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

pub mod attributes;
pub mod data;
pub mod error;
pub mod events;
pub mod hooks;
pub mod interfaces;
pub mod queue;
pub mod recovery;
pub mod runs;
pub mod serialization;
pub mod spec_version;
pub mod steps;
pub mod ulid;
pub mod waits;

pub use attributes::*;
pub use data::*;
pub use error::*;
pub use events::*;
pub use hooks::*;
pub use interfaces::*;
pub use queue::*;
pub use recovery::*;
pub use runs::*;
pub use serialization::*;
pub use spec_version::*;
pub use steps::*;
pub use ulid::*;
pub use waits::*;

/// Shared string header map used by queue and HTTP contracts.
pub type Headers = BTreeMap<String, String>;

/// Compatibility alias for world implementation pagination helpers.
pub type Pagination = PaginationOptions;

/// Worlds that can clear their backing state for deterministic local tests.
pub trait ClearableWorld {
    /// Error type returned by this world implementation.
    type Error;

    /// Remove persisted workflow data owned by this world.
    fn clear(&self) -> Result<(), Self::Error>;
}

/// Worlds that can recover active runs after a restart.
pub trait RecoverableWorld {
    /// Error type returned by this world implementation.
    type Error;

    /// Re-enqueue persisted pending/running runs.
    fn recover_active_runs(&self) -> Result<usize, Self::Error>;
}

/// Upstream repository used for this crate boundary.
pub const UPSTREAM_REPOSITORY: &str = "github.com/vercel/workflow";

/// Upstream ref fetched into the local source mirror.
pub const UPSTREAM_REF: &str = "main";

/// Remote HEAD verified for the initial crate skeleton.
pub const UPSTREAM_HEAD: &str = "1ee63b870afbf9754eb1022b1bb5f02d0ab042f9";

/// Upstream package owned by this crate.
pub const UPSTREAM_PACKAGE: &str = "@workflow/world";

/// Upstream package version inventoried for this skeleton.
pub const UPSTREAM_VERSION: &str = "5.0.0-beta.5";

/// Rust crate version compiled into the library.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Portable data contracts shared with upstream `packages/web-shared`.
pub mod web_shared_contracts {
    use std::collections::HashMap;

    /// Workflow run status values shared by the World API and observability data.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum WorkflowRunStatus {
        Pending,
        Running,
        Completed,
        Failed,
        Cancelled,
    }

    /// Workflow event types defined by upstream `packages/world/src/events.ts`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum WorkflowEventType {
        RunCreated,
        RunStarted,
        RunCompleted,
        RunFailed,
        RunCancelled,
        StepCreated,
        StepStarted,
        StepCompleted,
        StepFailed,
        StepRetrying,
        HookCreated,
        HookReceived,
        HookDisposed,
        HookConflict,
        WaitCreated,
        WaitCompleted,
    }

    impl WorkflowEventType {
        /// Return the upstream wire name for this event type.
        pub const fn as_str(self) -> &'static str {
            match self {
                Self::RunCreated => "run_created",
                Self::RunStarted => "run_started",
                Self::RunCompleted => "run_completed",
                Self::RunFailed => "run_failed",
                Self::RunCancelled => "run_cancelled",
                Self::StepCreated => "step_created",
                Self::StepStarted => "step_started",
                Self::StepCompleted => "step_completed",
                Self::StepFailed => "step_failed",
                Self::StepRetrying => "step_retrying",
                Self::HookCreated => "hook_created",
                Self::HookReceived => "hook_received",
                Self::HookDisposed => "hook_disposed",
                Self::HookConflict => "hook_conflict",
                Self::WaitCreated => "wait_created",
                Self::WaitCompleted => "wait_completed",
            }
        }

        pub const fn is_step(self) -> bool {
            matches!(
                self,
                Self::StepCreated
                    | Self::StepStarted
                    | Self::StepCompleted
                    | Self::StepFailed
                    | Self::StepRetrying
            )
        }

        pub const fn is_timer(self) -> bool {
            matches!(self, Self::WaitCreated | Self::WaitCompleted)
        }

        pub const fn is_hook_lifecycle(self) -> bool {
            matches!(
                self,
                Self::HookReceived | Self::HookCreated | Self::HookDisposed
            )
        }
    }

    /// Minimal event shape needed for portable World event data contracts.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct WorkflowEvent {
        pub event_id: String,
        pub run_id: String,
        pub event_type: WorkflowEventType,
        pub correlation_id: Option<String>,
        pub created_at_ms: i64,
        pub spec_version: Option<u32>,
        pub step_name: Option<String>,
        pub resume_at_ms: Option<i64>,
    }

    impl WorkflowEvent {
        pub fn new(
            event_type: WorkflowEventType,
            correlation_id: Option<impl Into<String>>,
            created_at_ms: i64,
        ) -> Self {
            let correlation_id = correlation_id.map(Into::into);
            let event_id = correlation_id
                .as_deref()
                .map(|id| format!("evnt_{id}_{}", event_type.as_str()))
                .unwrap_or_else(|| format!("evnt_{}", event_type.as_str()));
            Self {
                event_id,
                run_id: "wrun_v1test".to_string(),
                event_type,
                correlation_id,
                created_at_ms,
                spec_version: Some(1),
                step_name: None,
                resume_at_ms: None,
            }
        }

        pub fn with_event_id(mut self, event_id: impl Into<String>) -> Self {
            self.event_id = event_id.into();
            self
        }

        pub fn with_run_id(mut self, run_id: impl Into<String>) -> Self {
            self.run_id = run_id.into();
            self
        }

        pub fn with_step_name(mut self, step_name: impl Into<String>) -> Self {
            self.step_name = Some(step_name.into());
            self
        }

        pub fn with_resume_at_ms(mut self, resume_at_ms: i64) -> Self {
            self.resume_at_ms = Some(resume_at_ms);
            self
        }

        pub fn with_spec_version(mut self, spec_version: u32) -> Self {
            self.spec_version = Some(spec_version);
            self
        }
    }

    /// Minimal run shape needed for portable World trace data contracts.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct WorkflowRun {
        pub run_id: String,
        pub deployment_id: String,
        pub workflow_name: String,
        pub spec_version: Option<u32>,
        pub status: WorkflowRunStatus,
        pub created_at_ms: i64,
        pub updated_at_ms: i64,
        pub started_at_ms: Option<i64>,
        pub completed_at_ms: Option<i64>,
    }

    impl WorkflowRun {
        pub fn new(
            run_id: impl Into<String>,
            workflow_name: impl Into<String>,
            status: WorkflowRunStatus,
            created_at_ms: i64,
            updated_at_ms: i64,
        ) -> Self {
            Self {
                run_id: run_id.into(),
                deployment_id: "dep_1".to_string(),
                workflow_name: workflow_name.into(),
                spec_version: Some(1),
                status,
                created_at_ms,
                updated_at_ms,
                started_at_ms: None,
                completed_at_ms: None,
            }
        }

        pub fn with_started_at_ms(mut self, started_at_ms: i64) -> Self {
            self.started_at_ms = Some(started_at_ms);
            self
        }

        pub fn with_completed_at_ms(mut self, completed_at_ms: i64) -> Self {
            self.completed_at_ms = Some(completed_at_ms);
            self
        }

        pub fn without_completed_at(mut self) -> Self {
            self.completed_at_ms = None;
            self
        }

        pub fn with_spec_version(mut self, spec_version: u32) -> Self {
            self.spec_version = Some(spec_version);
            self
        }
    }

    /// Exact workflow ID kinds accepted by the observability event search contract.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ExactWorkflowSearchIdKind {
        Step,
        Wait,
        Hook,
        Event,
    }

    /// Parsed exact workflow ID search input.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ExactWorkflowSearchId {
        pub kind: ExactWorkflowSearchIdKind,
        pub id: String,
    }

    fn parse_prefixed_id(
        query: &str,
        prefix: &str,
        kind: ExactWorkflowSearchIdKind,
    ) -> Option<ExactWorkflowSearchId> {
        let query_prefix = query.get(..prefix.len())?;
        if query.len() != prefix.len() + 26 || !query_prefix.eq_ignore_ascii_case(prefix) {
            return None;
        }
        let body = query.get(prefix.len()..)?;
        let valid = body
        .bytes()
        .all(|byte| matches!(byte.to_ascii_uppercase(), b'0'..=b'9' | b'A'..=b'H' | b'J'..=b'K' | b'M'..=b'N' | b'P'..=b'T' | b'V'..=b'Z'));
        valid.then(|| ExactWorkflowSearchId {
            kind,
            id: format!(
                "{}{}",
                prefix.to_ascii_lowercase(),
                body.to_ascii_uppercase()
            ),
        })
    }

    /// Parse a full step, wait, hook, or event ID, normalizing the ULID body.
    pub fn parse_exact_workflow_search_id(query: &str) -> Option<ExactWorkflowSearchId> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return None;
        }
        parse_prefixed_id(trimmed, "step_", ExactWorkflowSearchIdKind::Step)
            .or_else(|| parse_prefixed_id(trimmed, "wait_", ExactWorkflowSearchIdKind::Wait))
            .or_else(|| parse_prefixed_id(trimmed, "hook_", ExactWorkflowSearchIdKind::Hook))
            .or_else(|| parse_prefixed_id(trimmed, "evnt_", ExactWorkflowSearchIdKind::Event))
    }

    /// Return true when input looks like a workflow ID search, including partial IDs.
    pub fn looks_like_workflow_id_search_input(query: &str) -> bool {
        let trimmed = query.trim();
        let has_workflow_prefix =
            ["step_", "wait_", "hook_", "evnt_", "wrun_"]
                .iter()
                .any(|prefix| {
                    trimmed
                        .get(..prefix.len())
                        .is_some_and(|query_prefix| query_prefix.eq_ignore_ascii_case(prefix))
                });
        has_workflow_prefix && trimmed.bytes().any(|byte| byte.is_ascii_digit())
    }

    /// Events grouped the same way `web-shared/src/lib/trace-builder.ts` groups them.
    #[derive(Debug, Default)]
    pub struct GroupedEvents<'a> {
        pub events_by_step_id: HashMap<String, Vec<&'a WorkflowEvent>>,
        pub run_level_events: Vec<&'a WorkflowEvent>,
        pub timer_events: HashMap<String, Vec<&'a WorkflowEvent>>,
        pub hook_events: HashMap<String, Vec<&'a WorkflowEvent>>,
    }

    fn push_grouped_event<'a>(
        map: &mut HashMap<String, Vec<&'a WorkflowEvent>>,
        correlation_id: &str,
        event: &'a WorkflowEvent,
    ) {
        map.entry(correlation_id.to_string())
            .or_default()
            .push(event);
    }

    /// Group raw events by correlation ID and entity family.
    pub fn group_events_by_correlation(events: &[WorkflowEvent]) -> GroupedEvents<'_> {
        let mut grouped = GroupedEvents::default();
        for event in events {
            let Some(correlation_id) = event.correlation_id.as_deref() else {
                grouped.run_level_events.push(event);
                continue;
            };

            if event.event_type.is_timer() {
                push_grouped_event(&mut grouped.timer_events, correlation_id, event);
            } else if event.event_type.is_hook_lifecycle() {
                push_grouped_event(&mut grouped.hook_events, correlation_id, event);
            } else if event.event_type.is_step() {
                push_grouped_event(&mut grouped.events_by_step_id, correlation_id, event);
            } else {
                grouped.run_level_events.push(event);
            }
        }
        grouped
    }

    /// Queued and execution durations for a correlated event timeline.
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
    pub struct DurationInfo {
        pub queued_ms: Option<i64>,
        pub ran_ms: Option<i64>,
    }

    /// Build per-correlation queued and run durations from workflow events.
    pub fn build_duration_map(events: &[WorkflowEvent]) -> HashMap<String, DurationInfo> {
        let mut chronological = events.iter().collect::<Vec<_>>();
        chronological.sort_by_key(|event| event.created_at_ms);

        let mut created_times = HashMap::<String, i64>::new();
        let mut first_started_times = HashMap::<String, i64>::new();
        let mut started_times = HashMap::<String, i64>::new();
        let mut durations = HashMap::<String, DurationInfo>::new();

        for event in chronological {
            let key = event
                .correlation_id
                .clone()
                .unwrap_or_else(|| "__run__".to_string());
            let ts = event.created_at_ms;

            if matches!(
                event.event_type,
                WorkflowEventType::StepCreated | WorkflowEventType::RunCreated
            ) {
                created_times.entry(key.clone()).or_insert(ts);
            }

            if matches!(
                event.event_type,
                WorkflowEventType::StepStarted
                    | WorkflowEventType::RunStarted
                    | WorkflowEventType::RunCreated
            ) {
                started_times.insert(key.clone(), ts);
                if !first_started_times.contains_key(&key) {
                    first_started_times.insert(key.clone(), ts);
                    created_times.entry(key.clone()).or_insert(ts);
                    if let Some(created_at) = created_times.get(&key) {
                        durations.entry(key.clone()).or_default().queued_ms = Some(ts - created_at);
                    }
                }
            }

            if matches!(
                event.event_type,
                WorkflowEventType::StepCompleted
                    | WorkflowEventType::StepFailed
                    | WorkflowEventType::RunCompleted
                    | WorkflowEventType::RunFailed
                    | WorkflowEventType::RunCancelled
                    | WorkflowEventType::WaitCompleted
                    | WorkflowEventType::HookDisposed
            ) {
                let info = durations.entry(key.clone()).or_default();
                if let Some(started_at) = started_times.get(&key) {
                    info.ran_ms = Some(ts - started_at);
                }
            }
        }

        durations
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ResourceType {
        Run,
        Step,
        Sleep,
        Hook,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum StepStatus {
        Pending,
        Running,
        Completed,
        Failed,
        Cancelled,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Span {
        pub span_id: String,
        pub name: String,
        pub resource: ResourceType,
        pub run_status: Option<WorkflowRunStatus>,
        pub step_status: Option<StepStatus>,
        pub step_name: Option<String>,
        pub start_time_ms: i64,
        pub end_time_ms: i64,
        pub duration_ms: i64,
        pub active_start_time_ms: Option<i64>,
        pub events: Vec<WorkflowEvent>,
        pub parent_span_id: Option<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Trace {
        pub trace_id: String,
        pub root_span_id: String,
        pub spans: Vec<Span>,
        pub known_duration_ms: i64,
    }

    fn compute_latest_known_time(events: &[WorkflowEvent], run: &WorkflowRun) -> i64 {
        events
            .iter()
            .map(|event| event.created_at_ms)
            .fold(run.created_at_ms, i64::max)
    }

    fn step_to_span(step_events: &[&WorkflowEvent], max_end_time_ms: i64) -> Option<Span> {
        let created_event = step_events
            .iter()
            .copied()
            .find(|event| event.event_type == WorkflowEventType::StepCreated);
        let anchor_event = created_event.or_else(|| step_events.first().copied())?;

        let mut status = StepStatus::Pending;
        let mut started_at_ms = None;
        let mut completed_at_ms = None;

        for event in step_events {
            match event.event_type {
                WorkflowEventType::StepStarted => {
                    status = StepStatus::Running;
                    started_at_ms.get_or_insert(event.created_at_ms);
                    completed_at_ms = None;
                }
                WorkflowEventType::StepCompleted => {
                    status = StepStatus::Completed;
                    completed_at_ms = Some(event.created_at_ms);
                }
                WorkflowEventType::StepFailed => {
                    status = StepStatus::Failed;
                    completed_at_ms = Some(event.created_at_ms);
                }
                WorkflowEventType::StepRetrying => {
                    status = StepStatus::Pending;
                    completed_at_ms = None;
                }
                _ => {}
            }
        }

        let step_name = created_event.and_then(|event| event.step_name.clone());
        let start_time_ms = anchor_event.created_at_ms;
        let end_time_ms = completed_at_ms.unwrap_or(max_end_time_ms);
        let active_start_time_ms = step_events
            .iter()
            .find(|event| event.event_type == WorkflowEventType::StepStarted)
            .map(|event| event.created_at_ms)
            .filter(|started_at| *started_at > start_time_ms);

        Some(Span {
            span_id: anchor_event.correlation_id.clone().unwrap_or_default(),
            name: step_name.clone().unwrap_or_default(),
            resource: ResourceType::Step,
            run_status: None,
            step_status: Some(status),
            step_name: Some(step_name.unwrap_or_default()),
            start_time_ms,
            end_time_ms,
            duration_ms: end_time_ms - start_time_ms,
            active_start_time_ms,
            events: step_events.iter().copied().cloned().collect(),
            parent_span_id: None,
        })
    }

    fn wait_to_span(
        wait_events: &[&WorkflowEvent],
        max_end_time_ms: i64,
        fallback_end_time_ms: i64,
    ) -> Option<Span> {
        let start_event = wait_events
            .iter()
            .copied()
            .find(|event| event.event_type == WorkflowEventType::WaitCreated)?;
        let completed_event = wait_events
            .iter()
            .copied()
            .find(|event| event.event_type == WorkflowEventType::WaitCompleted);

        let start_ms = start_event.created_at_ms;
        let end_time_ms = completed_event
            .map(|event| event.created_at_ms)
            .unwrap_or_else(|| {
                let fallback_cap = start_event
                    .resume_at_ms
                    .filter(|resume_at_ms| *resume_at_ms < fallback_end_time_ms)
                    .unwrap_or(fallback_end_time_ms);
                if max_end_time_ms > start_ms && max_end_time_ms < fallback_cap {
                    max_end_time_ms
                } else {
                    fallback_cap
                }
            });

        Some(Span {
            span_id: start_event.correlation_id.clone().unwrap_or_default(),
            name: "sleep".to_string(),
            resource: ResourceType::Sleep,
            run_status: None,
            step_status: None,
            step_name: None,
            start_time_ms: start_ms,
            end_time_ms,
            duration_ms: end_time_ms - start_ms,
            active_start_time_ms: None,
            events: wait_events.iter().copied().cloned().collect(),
            parent_span_id: None,
        })
    }

    fn run_to_span(run: &WorkflowRun, run_events: &[&WorkflowEvent], now_ms: i64) -> Span {
        let start_time_ms = run.created_at_ms;
        let end_time_ms = run.completed_at_ms.unwrap_or(now_ms);
        let active_start_time_ms = run
            .started_at_ms
            .filter(|started_at_ms| *started_at_ms > start_time_ms);

        Span {
            span_id: run.run_id.clone(),
            name: run.workflow_name.clone(),
            resource: ResourceType::Run,
            run_status: Some(run.status),
            step_status: None,
            step_name: None,
            start_time_ms,
            end_time_ms,
            duration_ms: end_time_ms - start_time_ms,
            active_start_time_ms,
            events: run_events.iter().copied().cloned().collect(),
            parent_span_id: None,
        }
    }

    fn cascade_spans(run_span: Span, spans: Vec<Span>) -> Vec<Span> {
        let mut sorted = Vec::with_capacity(spans.len() + 1);
        sorted.push(run_span);
        let mut child_spans = spans;
        child_spans.sort_by_key(|span| span.start_time_ms);
        sorted.extend(child_spans);

        let mut previous_span_id = None;
        sorted
            .into_iter()
            .map(|mut span| {
                span.parent_span_id = previous_span_id.clone();
                previous_span_id = Some(span.span_id.clone());
                span
            })
            .collect()
    }

    /// Build a trace from a WorkflowRun and its events.
    pub fn build_trace(run: &WorkflowRun, events: &[WorkflowEvent], now_ms: i64) -> Trace {
        let grouped_events = group_events_by_correlation(events);
        let latest_known_time_ms = compute_latest_known_time(events, run);
        let child_max_end_ms = latest_known_time_ms;
        let run_max_end_ms = run.completed_at_ms.unwrap_or(now_ms);

        let mut spans = grouped_events
            .events_by_step_id
            .values()
            .filter_map(|events| step_to_span(events, child_max_end_ms))
            .chain(
                grouped_events
                    .timer_events
                    .values()
                    .filter_map(|events| wait_to_span(events, child_max_end_ms, run_max_end_ms)),
            )
            .collect::<Vec<_>>();
        spans.sort_by_key(|span| span.start_time_ms);

        let run_span = run_to_span(run, &grouped_events.run_level_events, now_ms);
        let trace_start_ms = run_span.start_time_ms;
        Trace {
            trace_id: run.run_id.clone(),
            root_span_id: run.run_id.clone(),
            spans: cascade_spans(run_span, spans),
            known_duration_ms: (latest_known_time_ms - trace_start_ms).max(0),
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum SegmentStatus {
        Queued,
        Running,
        Failed,
        Retrying,
        Succeeded,
        Waiting,
        Sleeping,
        Received,
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Segment {
        pub start_fraction: f64,
        pub end_fraction: f64,
        pub status: SegmentStatus,
    }

    #[derive(Debug, Clone, PartialEq)]
    pub struct SegmentResult {
        pub segments: Vec<Segment>,
    }

    fn time_to_fraction(time_ms: i64, span_start_ms: i64, span_duration_ms: i64) -> f64 {
        if span_duration_ms <= 0 {
            return 0.0;
        }
        ((time_ms - span_start_ms) as f64 / span_duration_ms as f64).clamp(0.0, 1.0)
    }

    fn compute_v1_run_segments(
        start_time_ms: i64,
        duration_ms: i64,
        active_start_time_ms: Option<i64>,
        run_status: Option<WorkflowRunStatus>,
    ) -> Vec<Segment> {
        let mut segments = Vec::new();
        let mut cursor = 0.0;
        if let Some(active_start_time_ms) =
            active_start_time_ms.filter(|active| *active > start_time_ms)
        {
            let queued_fraction =
                time_to_fraction(active_start_time_ms, start_time_ms, duration_ms);
            if queued_fraction > 0.001 {
                segments.push(Segment {
                    start_fraction: 0.0,
                    end_fraction: queued_fraction,
                    status: SegmentStatus::Queued,
                });
                cursor = queued_fraction;
            }
        }

        let status = match run_status {
            Some(WorkflowRunStatus::Failed) => SegmentStatus::Failed,
            Some(WorkflowRunStatus::Completed | WorkflowRunStatus::Cancelled) => {
                SegmentStatus::Succeeded
            }
            _ => SegmentStatus::Running,
        };
        segments.push(Segment {
            start_fraction: cursor,
            end_fraction: 1.0,
            status,
        });
        segments
    }

    fn compute_run_segments(span: &Span) -> Vec<Segment> {
        if span.duration_ms <= 0 {
            return Vec::new();
        }

        let has_run_created = span
            .events
            .iter()
            .any(|event| event.event_type == WorkflowEventType::RunCreated);
        if !has_run_created {
            return compute_v1_run_segments(
                span.start_time_ms,
                span.duration_ms,
                span.active_start_time_ms,
                span.run_status,
            );
        }

        let failed_event = span
            .events
            .iter()
            .find(|event| event.event_type == WorkflowEventType::RunFailed);
        let completed_event = span
            .events
            .iter()
            .find(|event| event.event_type == WorkflowEventType::RunCompleted);
        let mut segments = Vec::new();
        let mut cursor = 0.0;

        if let Some(active_start_time_ms) = span
            .active_start_time_ms
            .filter(|active| *active > span.start_time_ms)
        {
            let queued_fraction =
                time_to_fraction(active_start_time_ms, span.start_time_ms, span.duration_ms);
            if queued_fraction > 0.001 {
                segments.push(Segment {
                    start_fraction: 0.0,
                    end_fraction: queued_fraction,
                    status: SegmentStatus::Queued,
                });
                cursor = queued_fraction;
            }
        }

        if let Some(failed_event) = failed_event {
            let failed_fraction = time_to_fraction(
                failed_event.created_at_ms,
                span.start_time_ms,
                span.duration_ms,
            );
            if failed_fraction > cursor + 0.001 {
                segments.push(Segment {
                    start_fraction: cursor,
                    end_fraction: failed_fraction,
                    status: SegmentStatus::Running,
                });
            }
            segments.push(Segment {
                start_fraction: failed_fraction,
                end_fraction: 1.0,
                status: SegmentStatus::Failed,
            });
        } else if let Some(completed_event) = completed_event {
            let completed_fraction = time_to_fraction(
                completed_event.created_at_ms,
                span.start_time_ms,
                span.duration_ms,
            );
            if completed_fraction > cursor + 0.001 {
                segments.push(Segment {
                    start_fraction: cursor,
                    end_fraction: completed_fraction,
                    status: SegmentStatus::Running,
                });
            }
            segments.push(Segment {
                start_fraction: completed_fraction,
                end_fraction: 1.0,
                status: SegmentStatus::Succeeded,
            });
        } else {
            segments.push(Segment {
                start_fraction: cursor,
                end_fraction: 1.0,
                status: SegmentStatus::Running,
            });
        }

        segments
    }

    /// Compute trace-viewer segment states for portable span data.
    pub fn compute_segments(resource_type: ResourceType, span: &Span) -> SegmentResult {
        let segments = match resource_type {
            ResourceType::Run => compute_run_segments(span),
            ResourceType::Step | ResourceType::Sleep | ResourceType::Hook => Vec::new(),
        };
        SegmentResult { segments }
    }
}

#[cfg(test)]
mod tests {
    use super::web_shared_contracts::*;
    use super::{
        ClearableWorld, RecoverableWorld, UPSTREAM_HEAD, UPSTREAM_PACKAGE, UPSTREAM_VERSION,
    };

    const BASE_TIME_MS: i64 = 0;
    const STARTED_TIME_MS: i64 = 1_000;
    const COMPLETED_TIME_MS: i64 = 10_000;
    const DAY_MS: i64 = 86_400_000;

    fn make_v1_run(status: WorkflowRunStatus) -> WorkflowRun {
        let run = WorkflowRun::new(
            "wrun_v1test",
            "v1-workflow",
            status,
            BASE_TIME_MS,
            COMPLETED_TIME_MS,
        )
        .with_started_at_ms(STARTED_TIME_MS);

        match status {
            WorkflowRunStatus::Completed
            | WorkflowRunStatus::Failed
            | WorkflowRunStatus::Cancelled => run.with_completed_at_ms(COMPLETED_TIME_MS),
            WorkflowRunStatus::Pending | WorkflowRunStatus::Running => run.without_completed_at(),
        }
    }

    fn step_events(
        correlation_id: &str,
        step_name: &str,
        start_offset_ms: i64,
        end_offset_ms: i64,
    ) -> Vec<WorkflowEvent> {
        vec![
            WorkflowEvent::new(
                WorkflowEventType::StepCreated,
                Some(correlation_id),
                BASE_TIME_MS + start_offset_ms,
            )
            .with_step_name(step_name),
            WorkflowEvent::new(
                WorkflowEventType::StepStarted,
                Some(correlation_id),
                BASE_TIME_MS + start_offset_ms + 100,
            ),
            WorkflowEvent::new(
                WorkflowEventType::StepCompleted,
                Some(correlation_id),
                BASE_TIME_MS + end_offset_ms,
            ),
        ]
    }

    fn v1_step_events(
        correlation_id: &str,
        start_offset_ms: i64,
        end_offset_ms: i64,
    ) -> Vec<WorkflowEvent> {
        vec![
            WorkflowEvent::new(
                WorkflowEventType::StepStarted,
                Some(correlation_id),
                BASE_TIME_MS + start_offset_ms,
            ),
            WorkflowEvent::new(
                WorkflowEventType::StepCompleted,
                Some(correlation_id),
                BASE_TIME_MS + end_offset_ms,
            ),
        ]
    }

    #[test]
    fn records_world_source_snapshot() {
        assert_eq!(UPSTREAM_PACKAGE, "@workflow/world");
        assert_eq!(UPSTREAM_VERSION, "5.0.0-beta.5");
        assert_eq!(UPSTREAM_HEAD.len(), 40);
    }

    #[test]
    fn exposes_world_local_lifecycle_extension_traits() {
        struct FakeWorld;

        impl ClearableWorld for FakeWorld {
            type Error = core::convert::Infallible;

            fn clear(&self) -> Result<(), Self::Error> {
                Ok(())
            }
        }

        impl RecoverableWorld for FakeWorld {
            type Error = core::convert::Infallible;

            fn recover_active_runs(&self) -> Result<usize, Self::Error> {
                Ok(0)
            }
        }

        let world = FakeWorld;
        assert_eq!(world.recover_active_runs(), Ok(0));
        assert_eq!(world.clear(), Ok(()));
    }

    #[test]
    fn web_shared_exact_id_accepts_full_step_ids() {
        let id = "step_01KSG94DWMWZRQBK04D3GS2CAQ";
        assert_eq!(
            parse_exact_workflow_search_id(id),
            Some(ExactWorkflowSearchId {
                kind: ExactWorkflowSearchIdKind::Step,
                id: id.to_string()
            })
        );
    }

    #[test]
    fn web_shared_exact_id_accepts_full_wait_ids() {
        let id = "wait_01KSG94DWMWZRQBK04D3GS2CAQ";
        assert_eq!(
            parse_exact_workflow_search_id(id),
            Some(ExactWorkflowSearchId {
                kind: ExactWorkflowSearchIdKind::Wait,
                id: id.to_string()
            })
        );
    }

    #[test]
    fn web_shared_exact_id_accepts_full_hook_ids() {
        let id = "hook_01KSG94DWMWZRQBK04D3GS2CAQ";
        assert_eq!(
            parse_exact_workflow_search_id(id),
            Some(ExactWorkflowSearchId {
                kind: ExactWorkflowSearchIdKind::Hook,
                id: id.to_string()
            })
        );
    }

    #[test]
    fn web_shared_exact_id_accepts_full_event_ids() {
        let id = "evnt_01KSG94CMGCPMC3PPACDCJR9AQ";
        assert_eq!(
            parse_exact_workflow_search_id(id),
            Some(ExactWorkflowSearchId {
                kind: ExactWorkflowSearchIdKind::Event,
                id: id.to_string()
            })
        );
    }

    #[test]
    fn web_shared_exact_id_normalizes_lowercase_ulid_bodies() {
        assert_eq!(
            parse_exact_workflow_search_id("step_01ksg94dwmwzrqbk04d3gs2caq"),
            Some(ExactWorkflowSearchId {
                kind: ExactWorkflowSearchIdKind::Step,
                id: "step_01KSG94DWMWZRQBK04D3GS2CAQ".to_string()
            })
        );
    }

    #[test]
    fn web_shared_exact_id_trims_leading_and_trailing_whitespace() {
        let id = "evnt_01KSG94CMGCPMC3PPACDCJR9AQ";
        assert_eq!(
            parse_exact_workflow_search_id(&format!("  {id}  ")),
            Some(ExactWorkflowSearchId {
                kind: ExactWorkflowSearchIdKind::Event,
                id: id.to_string()
            })
        );
    }

    #[test]
    fn web_shared_exact_id_rejects_partial_ids_and_run_ids() {
        for input in [
            "step_01KSG94",
            "wait_01KSG94",
            "hook_01KSG94",
            "evnt_01KSG94",
            "wrun_01KSG94CFWFBPBYWW3PX7SF73W",
        ] {
            assert_eq!(parse_exact_workflow_search_id(input), None);
        }
    }

    #[test]
    fn web_shared_exact_id_rejects_illegal_crockford_characters_or_wrong_length() {
        for input in [
            "step_01ISG94DWMWZRQBK04D3GS2CAQ",
            "step_01KSG94DWMWZRQBK04D3GS2CA",
            "step_01KSG94DWMWZRQBK04D3GS2CAQQ",
        ] {
            assert_eq!(parse_exact_workflow_search_id(input), None);
        }
    }

    #[test]
    fn web_shared_exact_id_looks_like_workflow_id_matches_known_prefixes() {
        assert!(looks_like_workflow_id_search_input("step_01KSG94"));
        assert!(looks_like_workflow_id_search_input("wrun_01KSG94"));
        assert!(looks_like_workflow_id_search_input("EVNT_01KSG94"));
    }

    #[test]
    fn web_shared_exact_id_looks_like_workflow_id_rejects_free_text() {
        assert!(!looks_like_workflow_id_search_input("parseInvoice"));
        assert!(!looks_like_workflow_id_search_input("step_started"));
    }

    #[test]
    fn web_shared_event_duration_uses_first_step_started_not_last_for_retries() {
        let events = vec![
            WorkflowEvent::new(WorkflowEventType::StepCreated, Some("step-1"), 0),
            WorkflowEvent::new(WorkflowEventType::StepStarted, Some("step-1"), 1_000),
            WorkflowEvent::new(WorkflowEventType::StepFailed, Some("step-1"), 2_000),
            WorkflowEvent::new(WorkflowEventType::StepRetrying, Some("step-1"), 3_000),
            WorkflowEvent::new(WorkflowEventType::StepStarted, Some("step-1"), 10_000),
            WorkflowEvent::new(WorkflowEventType::StepCompleted, Some("step-1"), 11_000),
        ];

        assert_eq!(build_duration_map(&events)["step-1"].queued_ms, Some(1_000));
    }

    #[test]
    fn web_shared_event_duration_handles_descending_order() {
        let ascending = vec![
            WorkflowEvent::new(WorkflowEventType::StepCreated, Some("step-1"), 0),
            WorkflowEvent::new(WorkflowEventType::StepStarted, Some("step-1"), 1_000),
            WorkflowEvent::new(WorkflowEventType::StepFailed, Some("step-1"), 2_000),
            WorkflowEvent::new(WorkflowEventType::StepStarted, Some("step-1"), 10_000),
            WorkflowEvent::new(WorkflowEventType::StepCompleted, Some("step-1"), 11_000),
        ];
        let mut descending = ascending.clone();
        descending.reverse();

        assert_eq!(
            build_duration_map(&ascending)["step-1"].queued_ms,
            Some(1_000)
        );
        assert_eq!(
            build_duration_map(&descending)["step-1"].queued_ms,
            Some(1_000)
        );
    }

    #[test]
    fn web_shared_event_duration_handles_single_start_without_retry() {
        let events = vec![
            WorkflowEvent::new(WorkflowEventType::StepCreated, Some("step-2"), 0),
            WorkflowEvent::new(WorkflowEventType::StepStarted, Some("step-2"), 500),
            WorkflowEvent::new(WorkflowEventType::StepCompleted, Some("step-2"), 2_000),
        ];

        assert_eq!(build_duration_map(&events)["step-2"].queued_ms, Some(500));
    }

    #[test]
    fn web_shared_event_duration_falls_back_to_started_time_without_created_event() {
        let events = vec![
            WorkflowEvent::new(WorkflowEventType::StepStarted, Some("step-3"), 5_000),
            WorkflowEvent::new(WorkflowEventType::StepCompleted, Some("step-3"), 6_000),
        ];

        assert_eq!(build_duration_map(&events)["step-3"].queued_ms, Some(0));
    }

    #[test]
    fn web_shared_trace_v1_groups_step_events_without_run_level_events() {
        let events = step_events("step_1", "add", 1_000, 3_000);
        let grouped = group_events_by_correlation(&events);

        assert_eq!(grouped.run_level_events.len(), 0);
        assert_eq!(grouped.events_by_step_id.len(), 1);
        assert_eq!(grouped.events_by_step_id["step_1"].len(), 3);
    }

    #[test]
    fn web_shared_trace_v1_builds_completed_run_with_step_events() {
        let run = make_v1_run(WorkflowRunStatus::Completed);
        let events = step_events("step_1", "add", 1_000, 3_000);
        let trace = build_trace(&run, &events, 60_000);

        assert_eq!(trace.trace_id, "wrun_v1test");
        assert_eq!(trace.root_span_id, "wrun_v1test");
        assert_eq!(trace.spans.len(), 2);
        let run_span = trace
            .spans
            .iter()
            .find(|span| span.span_id == "wrun_v1test")
            .expect("run span");
        assert_eq!(run_span.resource, ResourceType::Run);
        assert_eq!(run_span.run_status, Some(WorkflowRunStatus::Completed));
    }

    #[test]
    fn web_shared_trace_v1_builds_failed_run() {
        let run = make_v1_run(WorkflowRunStatus::Failed);
        let events = step_events("step_1", "add", 1_000, 3_000);
        let trace = build_trace(&run, &events, 60_000);
        let run_span = trace
            .spans
            .iter()
            .find(|span| span.span_id == "wrun_v1test")
            .expect("run span");

        assert_eq!(run_span.run_status, Some(WorkflowRunStatus::Failed));
    }

    #[test]
    fn web_shared_trace_v1_builds_run_with_no_events() {
        let run = make_v1_run(WorkflowRunStatus::Completed);
        let trace = build_trace(&run, &[], 60_000);

        assert_eq!(trace.spans.len(), 1);
        assert_eq!(trace.spans[0].span_id, "wrun_v1test");
        assert_eq!(trace.spans[0].resource, ResourceType::Run);
    }

    #[test]
    fn web_shared_trace_v1_builds_step_spans_without_step_created() {
        let run = make_v1_run(WorkflowRunStatus::Completed);
        let events = [
            v1_step_events("step_1", 1_000, 3_000),
            v1_step_events("step_2", 4_000, 6_000),
        ]
        .concat();
        let trace = build_trace(&run, &events, 60_000);
        let step_spans = trace
            .spans
            .iter()
            .filter(|span| span.resource == ResourceType::Step)
            .collect::<Vec<_>>();

        assert_eq!(trace.spans.len(), 3);
        assert_eq!(step_spans.len(), 2);
        assert_eq!(step_spans[0].span_id, "step_1");
        assert_eq!(step_spans[1].span_id, "step_2");
    }

    #[test]
    fn web_shared_trace_v1_derives_step_status_without_step_created() {
        let run = make_v1_run(WorkflowRunStatus::Completed);
        let events = v1_step_events("step_1", 1_000, 3_000);
        let trace = build_trace(&run, &events, 60_000);
        let step_span = trace
            .spans
            .iter()
            .find(|span| span.span_id == "step_1")
            .expect("step span");

        assert_eq!(step_span.step_status, Some(StepStatus::Completed));
        assert_eq!(step_span.step_name.as_deref(), Some(""));
    }

    #[test]
    fn web_shared_trace_v1_uses_correlation_id_when_step_name_is_unavailable() {
        let run = make_v1_run(WorkflowRunStatus::Completed);
        let events = v1_step_events("step_1", 1_000, 3_000);
        let trace = build_trace(&run, &events, 60_000);
        let step_span = trace
            .spans
            .iter()
            .find(|span| span.span_id == "step_1")
            .expect("step span");

        assert_eq!(step_span.span_id, "step_1");
    }

    #[test]
    fn web_shared_trace_v1_uses_resume_at_for_pending_sleep_duration() {
        let run = make_v1_run(WorkflowRunStatus::Running);
        let wait_created_at = BASE_TIME_MS + 1_000;
        let resume_at = BASE_TIME_MS + 61_000;
        let events = vec![
            WorkflowEvent::new(
                WorkflowEventType::WaitCreated,
                Some("wait_1"),
                wait_created_at,
            )
            .with_event_id("evnt_wait_created")
            .with_resume_at_ms(resume_at),
        ];
        let trace = build_trace(&run, &events, BASE_TIME_MS + 11_000);
        let sleep_span = trace
            .spans
            .iter()
            .find(|span| span.resource == ResourceType::Sleep)
            .expect("sleep span");

        assert_eq!(sleep_span.duration_ms, 10_000);
    }

    #[test]
    fn web_shared_trace_v1_caps_pending_sleep_spans_at_latest_known_event_before_resume_at() {
        let run =
            make_v1_run(WorkflowRunStatus::Completed).with_completed_at_ms(BASE_TIME_MS + DAY_MS);
        let wait_created_at = BASE_TIME_MS + 1_000;
        let latest_known_at = BASE_TIME_MS + DAY_MS + 1_000;
        let resume_at = BASE_TIME_MS + 6 * DAY_MS + 1_000;
        let events = vec![
            WorkflowEvent::new(
                WorkflowEventType::WaitCreated,
                Some("wait_1"),
                wait_created_at,
            )
            .with_event_id("evnt_wait_created")
            .with_resume_at_ms(resume_at),
            WorkflowEvent::new(
                WorkflowEventType::RunCompleted,
                None::<String>,
                latest_known_at,
            )
            .with_event_id("evnt_run_completed"),
        ];
        let trace = build_trace(&run, &events, latest_known_at);
        let sleep_span = trace
            .spans
            .iter()
            .find(|span| span.resource == ResourceType::Sleep)
            .expect("sleep span");

        assert_eq!(sleep_span.duration_ms, 86_399_000);
    }

    #[test]
    fn web_shared_trace_v1_run_segments_show_succeeded_for_completed_v1_run() {
        let run = make_v1_run(WorkflowRunStatus::Completed);
        let events = step_events("step_1", "add", 1_000, 3_000);
        let trace = build_trace(&run, &events, 60_000);
        let run_span = trace
            .spans
            .iter()
            .find(|span| span.span_id == run.run_id)
            .expect("run span");
        let result = compute_segments(ResourceType::Run, run_span);

        assert_eq!(
            result.segments.last().map(|segment| segment.status),
            Some(SegmentStatus::Succeeded)
        );
        assert_eq!(
            result.segments.last().map(|segment| segment.end_fraction),
            Some(1.0)
        );
    }

    #[test]
    fn web_shared_trace_v1_run_segments_show_failed_for_failed_v1_run() {
        let run = make_v1_run(WorkflowRunStatus::Failed);
        let events = step_events("step_1", "add", 1_000, 3_000);
        let trace = build_trace(&run, &events, 60_000);
        let run_span = trace
            .spans
            .iter()
            .find(|span| span.span_id == run.run_id)
            .expect("run span");
        let result = compute_segments(ResourceType::Run, run_span);

        assert_eq!(
            result.segments.last().map(|segment| segment.status),
            Some(SegmentStatus::Failed)
        );
        assert_eq!(
            result.segments.last().map(|segment| segment.end_fraction),
            Some(1.0)
        );
    }

    #[test]
    fn web_shared_trace_v1_run_segments_show_running_for_in_progress_v1_run() {
        let run = make_v1_run(WorkflowRunStatus::Running);
        let events = step_events("step_1", "add", 1_000, 3_000);
        let trace = build_trace(&run, &events, 60_000);
        let run_span = trace
            .spans
            .iter()
            .find(|span| span.span_id == run.run_id)
            .expect("run span");
        let result = compute_segments(ResourceType::Run, run_span);

        assert_eq!(
            result.segments.last().map(|segment| segment.status),
            Some(SegmentStatus::Running)
        );
    }

    #[test]
    fn web_shared_trace_v1_run_segments_show_queued_and_succeeded_with_started_at() {
        let run = make_v1_run(WorkflowRunStatus::Completed);
        let trace = build_trace(&run, &[], 60_000);
        let run_span = trace
            .spans
            .iter()
            .find(|span| span.span_id == run.run_id)
            .expect("run span");
        let result = compute_segments(ResourceType::Run, run_span);

        assert_eq!(result.segments.len(), 2);
        assert_eq!(result.segments[0].status, SegmentStatus::Queued);
        assert_eq!(result.segments[1].status, SegmentStatus::Succeeded);
    }

    #[test]
    fn web_shared_trace_v1_run_segments_v2_baseline_succeeds_from_run_completed_event() {
        let run = make_v1_run(WorkflowRunStatus::Completed).with_spec_version(2);
        let mut events = step_events("step_1", "add", 1_000, 3_000);
        events.insert(
            0,
            WorkflowEvent::new(WorkflowEventType::RunCreated, None::<String>, BASE_TIME_MS)
                .with_spec_version(2),
        );
        events.push(
            WorkflowEvent::new(
                WorkflowEventType::RunCompleted,
                None::<String>,
                COMPLETED_TIME_MS,
            )
            .with_spec_version(2),
        );
        let trace = build_trace(&run, &events, 60_000);
        let run_span = trace
            .spans
            .iter()
            .find(|span| span.span_id == run.run_id)
            .expect("run span");
        let result = compute_segments(ResourceType::Run, run_span);

        assert_eq!(
            result.segments.last().map(|segment| segment.status),
            Some(SegmentStatus::Succeeded)
        );
    }

    #[test]
    fn web_shared_trace_v1_run_segments_v2_mid_pagination_runs_until_completion_event_loaded() {
        let run = make_v1_run(WorkflowRunStatus::Completed).with_spec_version(2);
        let mut events = step_events("step_1", "add", 1_000, 3_000);
        events.insert(
            0,
            WorkflowEvent::new(WorkflowEventType::RunCreated, None::<String>, BASE_TIME_MS)
                .with_spec_version(2),
        );
        let trace = build_trace(&run, &events, 60_000);
        let run_span = trace
            .spans
            .iter()
            .find(|span| span.span_id == run.run_id)
            .expect("run span");
        let result = compute_segments(ResourceType::Run, run_span);

        assert_eq!(
            result.segments.last().map(|segment| segment.status),
            Some(SegmentStatus::Running)
        );
    }
}
