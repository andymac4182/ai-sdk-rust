use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use time::OffsetDateTime;

use crate::data::{PaginatedResponse, PaginationOptions, ResolveData};
use crate::hooks::Hook;
use crate::runs::WorkflowRun;
use crate::spec_version::SpecVersion;
use crate::steps::Step;
use crate::waits::Wait;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    RunCreated,
    RunStarted,
    RunCompleted,
    RunFailed,
    RunCancelled,
    StepCreated,
    StepCompleted,
    StepFailed,
    StepRetrying,
    StepStarted,
    HookCreated,
    HookReceived,
    HookDisposed,
    HookConflict,
    WaitCreated,
    WaitCompleted,
}

pub type EventData = BTreeMap<String, JsonValue>;

/// Event creation request for all world-creatable event types.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEventRequest {
    pub event_type: EventType,
    pub correlation_id: Option<String>,
    pub spec_version: Option<SpecVersion>,
    pub event_data: Option<EventData>,
}

pub type RunCreatedEventRequest = CreateEventRequest;

/// Event persisted in the world event log.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub run_id: String,
    pub event_id: String,
    pub event_type: EventType,
    pub correlation_id: Option<String>,
    pub spec_version: Option<SpecVersion>,
    pub created_at: OffsetDateTime,
    pub event_data: Option<EventData>,
}

pub type HookReceivedEvent = Event;
pub type HookConflictEvent = Event;

pub const fn event_data_ref_fields(event_type: EventType) -> &'static [&'static str] {
    match event_type {
        EventType::RunCreated => &["input"],
        EventType::RunCompleted => &["output"],
        EventType::RunFailed => &["error"],
        EventType::StepCreated => &["input"],
        EventType::StepCompleted => &["result"],
        EventType::StepFailed | EventType::StepRetrying => &["error"],
        EventType::HookCreated => &["metadata"],
        EventType::HookReceived => &["payload"],
        EventType::RunStarted
        | EventType::RunCancelled
        | EventType::StepStarted
        | EventType::HookDisposed
        | EventType::HookConflict
        | EventType::WaitCreated
        | EventType::WaitCompleted => &[],
    }
}

/// Strip only large ref/payload fields when data resolution is disabled.
pub fn strip_event_data_refs(mut event: Event, resolve_data: ResolveData) -> Event {
    if resolve_data != ResolveData::None {
        return event;
    }

    let fields = event_data_ref_fields(event.event_type);
    if fields.is_empty() {
        return event;
    }

    if let Some(event_data) = &mut event.event_data {
        for field in fields {
            event_data.remove(*field);
        }
        if event_data.is_empty() {
            event.event_data = None;
        }
    }

    event
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEventParams {
    pub v1_compat: Option<bool>,
    pub resolve_data: Option<ResolveData>,
    pub request_id: Option<String>,
}

/// Result of creating an event and applying its materialized entity updates.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventResult {
    pub event: Option<Event>,
    pub run: Option<WorkflowRun>,
    pub step: Option<Step>,
    pub hook: Option<Hook>,
    pub wait: Option<Wait>,
    pub events: Option<Vec<Event>>,
    pub cursor: Option<String>,
    pub has_more: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetEventParams {
    pub resolve_data: Option<ResolveData>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListEventsParams {
    pub run_id: String,
    pub pagination: Option<PaginationOptions>,
    pub resolve_data: Option<ResolveData>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListEventsByCorrelationIdParams {
    pub correlation_id: String,
    pub pagination: Option<PaginationOptions>,
    pub resolve_data: Option<ResolveData>,
}

pub type ListEventsResponse = PaginatedResponse<Event>;
