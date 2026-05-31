use async_trait::async_trait;

use crate::attributes::{AttributeChange, ExperimentalSetAttributesResult};
use crate::data::{GetChunksOptions, PaginatedResponse, StreamChunksResponse, StreamInfoResponse};
use crate::error::WorldError;
use crate::events::{
    CreateEventParams, CreateEventRequest, Event, EventResult, GetEventParams,
    ListEventsByCorrelationIdParams, ListEventsParams, RunCreatedEventRequest,
};
use crate::hooks::{GetHookParams, Hook, ListHooksParams};
use crate::queue::{QueueOptions, QueuePayload, QueueResult, ValidQueueName};
use crate::runs::{GetWorkflowRunParams, ListWorkflowRunsParams, WorkflowRun};
use crate::spec_version::SpecVersion;
use crate::steps::{GetStepParams, ListWorkflowRunStepsParams, Step};

#[async_trait]
pub trait Queue: Send + Sync {
    async fn deployment_id(&self) -> Result<String, WorldError>;

    async fn queue(
        &self,
        queue_name: &ValidQueueName,
        message: QueuePayload,
        options: Option<QueueOptions>,
    ) -> Result<QueueResult, WorldError>;
}

#[async_trait]
pub trait Streams: Send + Sync {
    async fn write_stream(
        &self,
        run_id: &str,
        name: &str,
        chunk: StreamWriteChunk,
    ) -> Result<(), WorldError>;

    async fn write_stream_multi(
        &self,
        run_id: &str,
        name: &str,
        chunks: Vec<StreamWriteChunk>,
    ) -> Result<(), WorldError> {
        for chunk in chunks {
            self.write_stream(run_id, name, chunk).await?;
        }
        Ok(())
    }

    async fn close_stream(&self, run_id: &str, name: &str) -> Result<(), WorldError>;

    async fn list_streams(&self, run_id: &str) -> Result<Vec<String>, WorldError>;

    async fn get_stream_chunks(
        &self,
        run_id: &str,
        name: &str,
        options: Option<GetChunksOptions>,
    ) -> Result<StreamChunksResponse, WorldError>;

    async fn get_stream_info(
        &self,
        run_id: &str,
        name: &str,
    ) -> Result<StreamInfoResponse, WorldError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StreamWriteChunk {
    Text(String),
    Bytes(Vec<u8>),
}

#[async_trait]
pub trait Runs: Send + Sync {
    async fn get_run(
        &self,
        run_id: &str,
        params: Option<GetWorkflowRunParams>,
    ) -> Result<WorkflowRun, WorldError>;

    async fn list_runs(
        &self,
        params: Option<ListWorkflowRunsParams>,
    ) -> Result<PaginatedResponse<WorkflowRun>, WorldError>;

    async fn experimental_set_attributes(
        &self,
        _run_id: &str,
        _changes: &[AttributeChange],
        _allow_reserved_attributes: bool,
    ) -> Result<ExperimentalSetAttributesResult, WorldError> {
        Err(WorldError::unsupported("runs.experimental_set_attributes"))
    }
}

#[async_trait]
pub trait Steps: Send + Sync {
    async fn get_step(
        &self,
        run_id: &str,
        step_id: &str,
        params: Option<GetStepParams>,
    ) -> Result<Step, WorldError>;

    async fn list_steps(
        &self,
        params: ListWorkflowRunStepsParams,
    ) -> Result<PaginatedResponse<Step>, WorldError>;
}

#[async_trait]
pub trait Events: Send + Sync {
    async fn create_run_event(
        &self,
        run_id: Option<&str>,
        data: RunCreatedEventRequest,
        params: Option<CreateEventParams>,
    ) -> Result<EventResult, WorldError>;

    async fn create_event(
        &self,
        run_id: &str,
        data: CreateEventRequest,
        params: Option<CreateEventParams>,
    ) -> Result<EventResult, WorldError>;

    async fn get_event(
        &self,
        run_id: &str,
        event_id: &str,
        params: Option<GetEventParams>,
    ) -> Result<Event, WorldError>;

    async fn list_events(
        &self,
        params: ListEventsParams,
    ) -> Result<PaginatedResponse<Event>, WorldError>;

    async fn list_events_by_correlation_id(
        &self,
        params: ListEventsByCorrelationIdParams,
    ) -> Result<PaginatedResponse<Event>, WorldError>;
}

#[async_trait]
pub trait Hooks: Send + Sync {
    async fn get_hook(
        &self,
        hook_id: &str,
        params: Option<GetHookParams>,
    ) -> Result<Hook, WorldError>;

    async fn get_hook_by_token(
        &self,
        token: &str,
        params: Option<GetHookParams>,
    ) -> Result<Hook, WorldError>;

    async fn list_hooks(
        &self,
        params: ListHooksParams,
    ) -> Result<PaginatedResponse<Hook>, WorldError>;
}

pub trait Storage: Runs + Steps + Events + Hooks {}

impl<T> Storage for T where T: Runs + Steps + Events + Hooks {}

#[async_trait]
pub trait World: Queue + Storage + Streams {
    fn spec_version(&self) -> Option<SpecVersion> {
        None
    }

    fn process_exit_triggers_queue_redelivery(&self) -> bool {
        false
    }

    fn stream_flush_interval_ms(&self) -> Option<u64> {
        None
    }

    async fn start(&self) -> Result<(), WorldError> {
        Ok(())
    }

    async fn close(&self) -> Result<(), WorldError> {
        Ok(())
    }

    async fn resolve_latest_deployment_id(&self) -> Result<String, WorldError> {
        Err(WorldError::unsupported("resolve_latest_deployment_id"))
    }

    async fn encryption_key_for_run(
        &self,
        _run: &WorkflowRun,
    ) -> Result<Option<Vec<u8>>, WorldError> {
        Ok(None)
    }

    async fn encryption_key_for_run_id(
        &self,
        _run_id: &str,
        _context: Option<&serde_json::Map<String, serde_json::Value>>,
    ) -> Result<Option<Vec<u8>>, WorldError> {
        Ok(None)
    }
}
