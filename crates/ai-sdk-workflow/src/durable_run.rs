use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use ai_sdk_provider::json::JsonValue;
use ai_sdk_rust::UiMessageChunk;
use serde::{Deserialize, Serialize};

const DEFAULT_MAX_AGENT_TURNS: usize = 1024;

/// Durable lifecycle state for a remote-agent run.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DurableRunState {
    /// The run has been accepted but not yet driven.
    Queued,

    /// The run is actively producing stream events.
    Running,

    /// The run intentionally paused for tool input from the user.
    WaitingForInput,

    /// The run intentionally paused for tool approval from the user.
    WaitingForApproval,

    /// Cancellation was requested and cleanup is in progress.
    Canceling,

    /// The run was canceled before finishing.
    Canceled,

    /// The run failed and released its active-run claim.
    Failed,

    /// The run finished successfully and released its active-run claim.
    Finished,
}

impl DurableRunState {
    /// Returns true when the run cannot be resumed or canceled further.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Canceled | Self::Failed | Self::Finished)
    }

    /// Returns true when the run is paused for a user/tool boundary.
    pub fn is_waiting(self) -> bool {
        matches!(self, Self::WaitingForInput | Self::WaitingForApproval)
    }
}

/// Reason a durable run paused.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum DurableRunPause {
    /// Waiting for a tool input value.
    ToolInput {
        /// Tool call that needs input.
        tool_call_id: String,
    },

    /// Waiting for an approval decision.
    ToolApproval {
        /// Approval request identifier.
        approval_id: String,

        /// Tool call guarded by the approval.
        tool_call_id: String,
    },
}

/// Resume payload supplied after a durable run pause.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum DurableRunResume {
    /// Continue a run with tool input.
    ToolInput {
        /// Tool call receiving input.
        tool_call_id: String,

        /// Input payload supplied by the caller.
        input: JsonValue,
    },

    /// Continue a run with an approval decision.
    ToolApproval {
        /// Approval request being answered.
        approval_id: String,

        /// Tool call guarded by the approval.
        tool_call_id: String,

        /// Whether the tool execution was approved.
        approved: bool,

        /// Optional user-facing reason for the decision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

impl DurableRunResume {
    fn matches_pause(&self, pause: &DurableRunPause) -> bool {
        match (self, pause) {
            (
                Self::ToolInput { tool_call_id, .. },
                DurableRunPause::ToolInput {
                    tool_call_id: paused_tool_call_id,
                },
            ) => tool_call_id == paused_tool_call_id,
            (
                Self::ToolApproval {
                    approval_id,
                    tool_call_id,
                    ..
                },
                DurableRunPause::ToolApproval {
                    approval_id: paused_approval_id,
                    tool_call_id: paused_tool_call_id,
                },
            ) => approval_id == paused_approval_id && tool_call_id == paused_tool_call_id,
            _ => false,
        }
    }
}

/// Persisted event payload for a durable run.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum DurableRunEventPayload {
    /// Run was created in the queued state.
    RunQueued,

    /// Run entered the running state.
    RunStarted,

    /// A stream chunk was persisted for subscribers.
    StreamChunk {
        /// UI-message stream chunk.
        chunk: UiMessageChunk,
    },

    /// Run paused for tool input.
    WaitingForInput {
        /// Tool call that needs input.
        tool_call_id: String,
    },

    /// Run paused for tool approval.
    WaitingForApproval {
        /// Approval request identifier.
        approval_id: String,

        /// Tool call guarded by the approval.
        tool_call_id: String,
    },

    /// A waiting run was resumed.
    RunResumed {
        /// Resume payload.
        resume: DurableRunResume,
    },

    /// Cancellation was requested.
    CancelRequested {
        /// Optional cancellation reason.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },

    /// Run reached the canceled terminal state.
    RunCanceled {
        /// Optional cancellation reason.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },

    /// Run reached the failed terminal state.
    RunFailed {
        /// Failure message.
        error: String,
    },

    /// Run reached the successful terminal state.
    RunFinished,
}

/// One persisted durable-run event.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableRunEvent {
    /// Monotonic sequence number within the run.
    pub sequence: usize,

    /// Monotonic stream index for stream chunks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_index: Option<usize>,

    /// Event payload.
    pub payload: DurableRunEventPayload,
}

impl DurableRunEvent {
    fn new(sequence: usize, stream_index: Option<usize>, payload: DurableRunEventPayload) -> Self {
        Self {
            sequence,
            stream_index,
            payload,
        }
    }
}

/// Persisted durable-run record.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableRunRecord {
    /// Stable run identifier.
    pub run_id: String,

    /// Chat, Slack thread, or other conversation owner.
    pub conversation_id: String,

    /// Current durable state.
    pub state: DurableRunState,

    /// Pause reason when the run is waiting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause: Option<DurableRunPause>,

    /// Persisted event log.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<DurableRunEvent>,

    #[serde(default)]
    next_stream_index: usize,
}

impl DurableRunRecord {
    fn queued(run_id: impl Into<String>, conversation_id: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            conversation_id: conversation_id.into(),
            state: DurableRunState::Queued,
            pause: None,
            events: Vec::new(),
            next_stream_index: 0,
        }
    }

    /// Returns the next stream index that will be assigned to a chunk.
    pub fn next_stream_index(&self) -> usize {
        self.next_stream_index
    }
}

/// Start options for a durable run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableRunStartOptions {
    /// Stable run identifier.
    pub run_id: String,

    /// Chat, Slack thread, or other conversation owner.
    pub conversation_id: String,

    /// Maximum fake-agent turns to drive before failing the run.
    pub max_agent_turns: usize,
}

impl DurableRunStartOptions {
    /// Creates start options with the default turn guard.
    pub fn new(run_id: impl Into<String>, conversation_id: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            conversation_id: conversation_id.into(),
            max_agent_turns: DEFAULT_MAX_AGENT_TURNS,
        }
    }

    /// Sets the maximum fake-agent turns to drive before failing the run.
    pub fn with_max_agent_turns(mut self, max_agent_turns: usize) -> Self {
        self.max_agent_turns = max_agent_turns;
        self
    }
}

/// Context passed to the fake-agent boundary.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableRunAgentContext {
    /// Stable run identifier.
    pub run_id: String,

    /// Chat, Slack thread, or other conversation owner.
    pub conversation_id: String,

    /// Resume payload for this turn, when any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<DurableRunResume>,

    /// Events persisted before this agent turn.
    pub previous_events: Vec<DurableRunEvent>,
}

/// Output from one fake-agent turn.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum DurableRunAgentOutput {
    /// Persist chunks and immediately ask the agent for another turn.
    Continue {
        /// Stream chunks to persist before the next turn.
        chunks: Vec<UiMessageChunk>,
    },

    /// Persist chunks and pause for tool input.
    WaitingForInput {
        /// Tool call that needs input.
        tool_call_id: String,

        /// Stream chunks to persist before pausing.
        chunks: Vec<UiMessageChunk>,
    },

    /// Persist chunks and pause for tool approval.
    WaitingForApproval {
        /// Approval request identifier.
        approval_id: String,

        /// Tool call guarded by the approval.
        tool_call_id: String,

        /// Stream chunks to persist before pausing.
        chunks: Vec<UiMessageChunk>,
    },

    /// Persist chunks and mark the run failed.
    Failed {
        /// Failure message.
        error: String,

        /// Stream chunks to persist before failure.
        chunks: Vec<UiMessageChunk>,
    },

    /// Persist chunks and mark the run finished.
    Finished {
        /// Stream chunks to persist before finish.
        chunks: Vec<UiMessageChunk>,
    },
}

/// Boundary implemented by a deterministic fake agent today and real model
/// streaming later.
pub trait DurableRunAgent {
    /// Produce the next durable-run turn.
    fn next_turn(
        &mut self,
        context: DurableRunAgentContext,
    ) -> Result<DurableRunAgentOutput, DurableRunAgentError>;
}

/// Error returned by the agent boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableRunAgentError {
    message: String,
}

impl DurableRunAgentError {
    /// Creates an agent-boundary error.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for DurableRunAgentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for DurableRunAgentError {}

/// Persistence boundary for durable runs.
pub trait DurableRunStore {
    /// Create a queued run record.
    fn create_run(
        &mut self,
        run_id: String,
        conversation_id: String,
    ) -> Result<(), DurableRunError>;

    /// Load one run record.
    fn load_run(&self, run_id: &str) -> Result<Option<DurableRunRecord>, DurableRunError>;

    /// Save the current state and pause reason.
    fn save_run_state(
        &mut self,
        run_id: &str,
        state: DurableRunState,
        pause: Option<DurableRunPause>,
    ) -> Result<(), DurableRunError>;

    /// Append one event to a run record.
    fn append_event(
        &mut self,
        run_id: &str,
        payload: DurableRunEventPayload,
    ) -> Result<DurableRunEvent, DurableRunError>;

    /// Claim the active run slot for a conversation.
    ///
    /// Returns `Ok(None)` when claimed and `Ok(Some(existing_run_id))` when a
    /// different non-terminal run already owns the slot.
    fn claim_active_run(
        &mut self,
        conversation_id: &str,
        run_id: &str,
    ) -> Result<Option<String>, DurableRunError>;

    /// Release the active run slot if it is still owned by `run_id`.
    fn release_active_run(
        &mut self,
        conversation_id: &str,
        run_id: &str,
    ) -> Result<(), DurableRunError>;

    /// Return the active run id for a conversation, when any.
    fn active_run_id(&self, conversation_id: &str) -> Option<&str>;
}

/// In-memory durable-run store for tests and early integration.
#[derive(Clone, Debug, Default)]
pub struct InMemoryDurableRunStore {
    runs: BTreeMap<String, DurableRunRecord>,
    active_runs: BTreeMap<String, String>,
}

impl InMemoryDurableRunStore {
    /// Creates an empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns all persisted stream chunks from `start_index` onward.
    pub fn stream_chunks_since(
        &self,
        run_id: &str,
        start_index: usize,
    ) -> Result<Vec<UiMessageChunk>, DurableRunError> {
        let record = self.require_run(run_id)?;
        Ok(stream_chunks_since_record(record, start_index))
    }

    fn require_run(&self, run_id: &str) -> Result<&DurableRunRecord, DurableRunError> {
        self.runs
            .get(run_id)
            .ok_or_else(|| DurableRunError::RunNotFound {
                run_id: run_id.to_string(),
            })
    }

    fn require_run_mut(&mut self, run_id: &str) -> Result<&mut DurableRunRecord, DurableRunError> {
        self.runs
            .get_mut(run_id)
            .ok_or_else(|| DurableRunError::RunNotFound {
                run_id: run_id.to_string(),
            })
    }
}

impl DurableRunStore for InMemoryDurableRunStore {
    fn create_run(
        &mut self,
        run_id: String,
        conversation_id: String,
    ) -> Result<(), DurableRunError> {
        if self.runs.contains_key(&run_id) {
            return Err(DurableRunError::RunAlreadyExists { run_id });
        }

        let record = DurableRunRecord::queued(run_id.clone(), conversation_id);
        self.runs.insert(run_id, record);
        Ok(())
    }

    fn load_run(&self, run_id: &str) -> Result<Option<DurableRunRecord>, DurableRunError> {
        Ok(self.runs.get(run_id).cloned())
    }

    fn save_run_state(
        &mut self,
        run_id: &str,
        state: DurableRunState,
        pause: Option<DurableRunPause>,
    ) -> Result<(), DurableRunError> {
        let record = self.require_run_mut(run_id)?;
        record.state = state;
        record.pause = pause;
        Ok(())
    }

    fn append_event(
        &mut self,
        run_id: &str,
        payload: DurableRunEventPayload,
    ) -> Result<DurableRunEvent, DurableRunError> {
        let record = self.require_run_mut(run_id)?;
        let sequence = record.events.len();
        let stream_index = if matches!(payload, DurableRunEventPayload::StreamChunk { .. }) {
            let stream_index = record.next_stream_index;
            record.next_stream_index += 1;
            Some(stream_index)
        } else {
            None
        };
        let event = DurableRunEvent::new(sequence, stream_index, payload);
        record.events.push(event.clone());
        Ok(event)
    }

    fn claim_active_run(
        &mut self,
        conversation_id: &str,
        run_id: &str,
    ) -> Result<Option<String>, DurableRunError> {
        if let Some(existing_run_id) = self.active_runs.get(conversation_id) {
            if existing_run_id == run_id {
                return Ok(None);
            }

            return Ok(Some(existing_run_id.clone()));
        }

        self.active_runs
            .insert(conversation_id.to_string(), run_id.to_string());
        Ok(None)
    }

    fn release_active_run(
        &mut self,
        conversation_id: &str,
        run_id: &str,
    ) -> Result<(), DurableRunError> {
        if self
            .active_runs
            .get(conversation_id)
            .is_some_and(|id| id == run_id)
        {
            self.active_runs.remove(conversation_id);
        }

        Ok(())
    }

    fn active_run_id(&self, conversation_id: &str) -> Option<&str> {
        self.active_runs.get(conversation_id).map(String::as_str)
    }
}

/// Durable-run execution result.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableRunExecution {
    /// Stable run identifier.
    pub run_id: String,

    /// Current durable state after this operation.
    pub state: DurableRunState,

    /// Stream chunks persisted by this operation.
    pub chunks: Vec<UiMessageChunk>,

    /// Events persisted by this operation.
    pub events: Vec<DurableRunEvent>,
}

impl DurableRunExecution {
    fn new(run_id: impl Into<String>, state: DurableRunState) -> Self {
        Self {
            run_id: run_id.into(),
            state,
            chunks: Vec::new(),
            events: Vec::new(),
        }
    }

    fn push_event(&mut self, event: DurableRunEvent) {
        if let DurableRunEventPayload::StreamChunk { chunk } = &event.payload {
            self.chunks.push(chunk.clone());
        }
        self.events.push(event);
    }
}

/// Durable run state machine and streaming boundary.
#[derive(Clone, Debug)]
pub struct DurableRunEngine<S, A> {
    store: S,
    agent: A,
}

impl<S, A> DurableRunEngine<S, A> {
    /// Creates a durable-run engine.
    pub fn new(store: S, agent: A) -> Self {
        Self { store, agent }
    }

    /// Returns the underlying store.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// Returns the underlying store mutably.
    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    /// Decomposes the engine into its store and agent.
    pub fn into_parts(self) -> (S, A) {
        (self.store, self.agent)
    }
}

impl<S, A> DurableRunEngine<S, A>
where
    S: DurableRunStore,
    A: DurableRunAgent,
{
    /// Start a new durable run and drive it until it finishes, fails, or waits.
    pub fn start(
        &mut self,
        options: DurableRunStartOptions,
    ) -> Result<DurableRunExecution, DurableRunError> {
        if let Some(existing_run_id) = self
            .store
            .claim_active_run(&options.conversation_id, &options.run_id)?
        {
            return Err(DurableRunError::ActiveRunConflict {
                conversation_id: options.conversation_id,
                existing_run_id,
            });
        }

        let create_result = self
            .store
            .create_run(options.run_id.clone(), options.conversation_id.clone());
        if create_result.is_err() {
            self.store
                .release_active_run(&options.conversation_id, &options.run_id)?;
        }
        create_result?;

        let mut execution = DurableRunExecution::new(&options.run_id, DurableRunState::Queued);
        execution.push_event(
            self.store
                .append_event(&options.run_id, DurableRunEventPayload::RunQueued)?,
        );
        self.transition(
            &options.run_id,
            DurableRunState::Running,
            None,
            &mut execution,
        )?;
        execution.push_event(
            self.store
                .append_event(&options.run_id, DurableRunEventPayload::RunStarted)?,
        );

        self.drive_agent_loop(&options.run_id, None, options.max_agent_turns, execution)
    }

    /// Resume a waiting durable run and drive it until it finishes, fails, or
    /// waits again.
    pub fn resume(
        &mut self,
        run_id: &str,
        resume: DurableRunResume,
    ) -> Result<DurableRunExecution, DurableRunError> {
        let record = self.require_run(run_id)?;
        match record.state {
            DurableRunState::Running | DurableRunState::Canceling => {
                return Err(DurableRunError::DuplicateResume {
                    run_id: run_id.to_string(),
                });
            }
            DurableRunState::WaitingForInput | DurableRunState::WaitingForApproval => {}
            actual => {
                return Err(DurableRunError::InvalidRunState {
                    run_id: run_id.to_string(),
                    expected: "waiting run".to_string(),
                    actual,
                });
            }
        }

        let pause = record
            .pause
            .clone()
            .ok_or_else(|| DurableRunError::InvalidRunState {
                run_id: run_id.to_string(),
                expected: "waiting run with pause reason".to_string(),
                actual: record.state,
            })?;

        if !resume.matches_pause(&pause) {
            return Err(DurableRunError::ResumeMismatch {
                run_id: run_id.to_string(),
                expected: Box::new(pause),
                received: Box::new(resume),
            });
        }

        let mut execution = DurableRunExecution::new(run_id, record.state);
        self.transition(run_id, DurableRunState::Running, None, &mut execution)?;
        execution.push_event(self.store.append_event(
            run_id,
            DurableRunEventPayload::RunResumed {
                resume: resume.clone(),
            },
        )?);

        self.drive_agent_loop(run_id, Some(resume), DEFAULT_MAX_AGENT_TURNS, execution)
    }

    /// Cancel a non-terminal durable run.
    pub fn cancel(
        &mut self,
        run_id: &str,
        reason: Option<String>,
    ) -> Result<DurableRunExecution, DurableRunError> {
        let record = self.require_run(run_id)?;
        if record.state.is_terminal() {
            return Err(DurableRunError::InvalidRunState {
                run_id: run_id.to_string(),
                expected: "non-terminal run".to_string(),
                actual: record.state,
            });
        }

        let mut execution = DurableRunExecution::new(run_id, record.state);
        self.transition(run_id, DurableRunState::Canceling, None, &mut execution)?;
        execution.push_event(self.store.append_event(
            run_id,
            DurableRunEventPayload::CancelRequested {
                reason: reason.clone(),
            },
        )?);
        execution.push_event(self.store.append_event(
            run_id,
            DurableRunEventPayload::StreamChunk {
                chunk: UiMessageChunk::abort(),
            },
        )?);
        self.transition(run_id, DurableRunState::Canceled, None, &mut execution)?;
        execution.push_event(
            self.store
                .append_event(run_id, DurableRunEventPayload::RunCanceled { reason })?,
        );
        self.release_active_claim(run_id)?;
        execution.state = DurableRunState::Canceled;
        Ok(execution)
    }

    /// Returns persisted stream chunks from `start_index` onward.
    pub fn stream_chunks_since(
        &self,
        run_id: &str,
        start_index: usize,
    ) -> Result<Vec<UiMessageChunk>, DurableRunError> {
        let record = self.require_run(run_id)?;
        Ok(stream_chunks_since_record(&record, start_index))
    }

    fn drive_agent_loop(
        &mut self,
        run_id: &str,
        resume: Option<DurableRunResume>,
        max_turns: usize,
        mut execution: DurableRunExecution,
    ) -> Result<DurableRunExecution, DurableRunError> {
        let mut next_resume = resume;

        for _ in 0..max_turns {
            let record = self.require_run(run_id)?;
            let context = DurableRunAgentContext {
                run_id: record.run_id.clone(),
                conversation_id: record.conversation_id,
                resume: next_resume.take(),
                previous_events: record.events,
            };

            let output = match self.agent.next_turn(context) {
                Ok(output) => output,
                Err(error) => {
                    self.fail_run(run_id, error.message().to_string(), &mut execution)?;
                    return Err(DurableRunError::AgentFailed {
                        run_id: run_id.to_string(),
                        message: error.message().to_string(),
                    });
                }
            };

            match output {
                DurableRunAgentOutput::Continue { chunks } => {
                    self.append_chunks(run_id, chunks, &mut execution)?;
                }
                DurableRunAgentOutput::WaitingForInput {
                    tool_call_id,
                    chunks,
                } => {
                    self.append_chunks(run_id, chunks, &mut execution)?;
                    let pause = DurableRunPause::ToolInput {
                        tool_call_id: tool_call_id.clone(),
                    };
                    self.transition(
                        run_id,
                        DurableRunState::WaitingForInput,
                        Some(pause),
                        &mut execution,
                    )?;
                    execution.push_event(self.store.append_event(
                        run_id,
                        DurableRunEventPayload::WaitingForInput { tool_call_id },
                    )?);
                    execution.state = DurableRunState::WaitingForInput;
                    return Ok(execution);
                }
                DurableRunAgentOutput::WaitingForApproval {
                    approval_id,
                    tool_call_id,
                    chunks,
                } => {
                    self.append_chunks(run_id, chunks, &mut execution)?;
                    let pause = DurableRunPause::ToolApproval {
                        approval_id: approval_id.clone(),
                        tool_call_id: tool_call_id.clone(),
                    };
                    self.transition(
                        run_id,
                        DurableRunState::WaitingForApproval,
                        Some(pause),
                        &mut execution,
                    )?;
                    execution.push_event(self.store.append_event(
                        run_id,
                        DurableRunEventPayload::WaitingForApproval {
                            approval_id,
                            tool_call_id,
                        },
                    )?);
                    execution.state = DurableRunState::WaitingForApproval;
                    return Ok(execution);
                }
                DurableRunAgentOutput::Failed { error, chunks } => {
                    self.append_chunks(run_id, chunks, &mut execution)?;
                    self.fail_run(run_id, error, &mut execution)?;
                    execution.state = DurableRunState::Failed;
                    return Ok(execution);
                }
                DurableRunAgentOutput::Finished { chunks } => {
                    self.append_chunks(run_id, chunks, &mut execution)?;
                    self.transition(run_id, DurableRunState::Finished, None, &mut execution)?;
                    execution.push_event(
                        self.store
                            .append_event(run_id, DurableRunEventPayload::RunFinished)?,
                    );
                    self.release_active_claim(run_id)?;
                    execution.state = DurableRunState::Finished;
                    return Ok(execution);
                }
            }
        }

        let message = format!("durable run exceeded {max_turns} agent turns");
        self.fail_run(run_id, message.clone(), &mut execution)?;
        Err(DurableRunError::MaxAgentTurnsExceeded {
            run_id: run_id.to_string(),
            max_turns,
        })
    }

    fn append_chunks(
        &mut self,
        run_id: &str,
        chunks: Vec<UiMessageChunk>,
        execution: &mut DurableRunExecution,
    ) -> Result<(), DurableRunError> {
        for chunk in chunks {
            execution.push_event(
                self.store
                    .append_event(run_id, DurableRunEventPayload::StreamChunk { chunk })?,
            );
        }
        Ok(())
    }

    fn transition(
        &mut self,
        run_id: &str,
        state: DurableRunState,
        pause: Option<DurableRunPause>,
        execution: &mut DurableRunExecution,
    ) -> Result<(), DurableRunError> {
        self.store.save_run_state(run_id, state, pause)?;
        execution.state = state;
        Ok(())
    }

    fn fail_run(
        &mut self,
        run_id: &str,
        error: String,
        execution: &mut DurableRunExecution,
    ) -> Result<(), DurableRunError> {
        self.transition(run_id, DurableRunState::Failed, None, execution)?;
        execution.push_event(
            self.store
                .append_event(run_id, DurableRunEventPayload::RunFailed { error })?,
        );
        self.release_active_claim(run_id)?;
        Ok(())
    }

    fn release_active_claim(&mut self, run_id: &str) -> Result<(), DurableRunError> {
        let record = self.require_run(run_id)?;
        self.store
            .release_active_run(&record.conversation_id, &record.run_id)
    }

    fn require_run(&self, run_id: &str) -> Result<DurableRunRecord, DurableRunError> {
        self.store
            .load_run(run_id)?
            .ok_or_else(|| DurableRunError::RunNotFound {
                run_id: run_id.to_string(),
            })
    }
}

fn stream_chunks_since_record(
    record: &DurableRunRecord,
    start_index: usize,
) -> Vec<UiMessageChunk> {
    record
        .events
        .iter()
        .filter(|event| event.stream_index.is_some_and(|index| index >= start_index))
        .filter_map(|event| match &event.payload {
            DurableRunEventPayload::StreamChunk { chunk } => Some(chunk.clone()),
            _ => None,
        })
        .collect()
}

/// Error returned by durable-run state machine operations.
#[derive(Clone, Debug, PartialEq)]
pub enum DurableRunError {
    /// Another active run owns the conversation.
    ActiveRunConflict {
        /// Conversation with an active run.
        conversation_id: String,

        /// Existing active run id.
        existing_run_id: String,
    },

    /// A run with the requested id already exists.
    RunAlreadyExists {
        /// Requested run id.
        run_id: String,
    },

    /// A run was not found.
    RunNotFound {
        /// Missing run id.
        run_id: String,
    },

    /// A resume request tried to drive an already-running run.
    DuplicateResume {
        /// Running run id.
        run_id: String,
    },

    /// Operation was not valid for the current state.
    InvalidRunState {
        /// Run id.
        run_id: String,

        /// Expected state description.
        expected: String,

        /// Actual durable state.
        actual: DurableRunState,
    },

    /// Resume payload did not match the recorded pause reason.
    ResumeMismatch {
        /// Run id.
        run_id: String,

        /// Expected pause reason.
        expected: Box<DurableRunPause>,

        /// Received resume payload.
        received: Box<DurableRunResume>,
    },

    /// Agent boundary failed after the failure event was persisted.
    AgentFailed {
        /// Run id.
        run_id: String,

        /// Agent error message.
        message: String,
    },

    /// Agent produced too many continuation turns without reaching a boundary.
    MaxAgentTurnsExceeded {
        /// Run id.
        run_id: String,

        /// Configured turn guard.
        max_turns: usize,
    },
}

impl fmt::Display for DurableRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ActiveRunConflict {
                conversation_id,
                existing_run_id,
            } => write!(
                formatter,
                "conversation '{conversation_id}' already has active run '{existing_run_id}'"
            ),
            Self::RunAlreadyExists { run_id } => {
                write!(formatter, "durable run '{run_id}' already exists")
            }
            Self::RunNotFound { run_id } => write!(formatter, "durable run '{run_id}' not found"),
            Self::DuplicateResume { run_id } => {
                write!(formatter, "durable run '{run_id}' is already running")
            }
            Self::InvalidRunState {
                run_id,
                expected,
                actual,
            } => write!(
                formatter,
                "durable run '{run_id}' expected {expected}, got {actual:?}"
            ),
            Self::ResumeMismatch { run_id, .. } => {
                write!(
                    formatter,
                    "resume payload does not match durable run '{run_id}'"
                )
            }
            Self::AgentFailed { run_id, message } => {
                write!(formatter, "durable run '{run_id}' agent failed: {message}")
            }
            Self::MaxAgentTurnsExceeded { run_id, max_turns } => write!(
                formatter,
                "durable run '{run_id}' exceeded {max_turns} agent turns"
            ),
        }
    }
}

impl Error for DurableRunError {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::VecDeque;

    #[derive(Debug)]
    struct ScriptedDurableAgent {
        outputs: VecDeque<Result<DurableRunAgentOutput, DurableRunAgentError>>,
        contexts: Vec<DurableRunAgentContext>,
    }

    impl ScriptedDurableAgent {
        fn new(
            outputs: impl IntoIterator<Item = Result<DurableRunAgentOutput, DurableRunAgentError>>,
        ) -> Self {
            Self {
                outputs: outputs.into_iter().collect(),
                contexts: Vec::new(),
            }
        }

        fn output(outputs: impl IntoIterator<Item = DurableRunAgentOutput>) -> Self {
            Self::new(outputs.into_iter().map(Ok))
        }
    }

    impl DurableRunAgent for ScriptedDurableAgent {
        fn next_turn(
            &mut self,
            context: DurableRunAgentContext,
        ) -> Result<DurableRunAgentOutput, DurableRunAgentError> {
            self.contexts.push(context);
            self.outputs
                .pop_front()
                .unwrap_or_else(|| Err(DurableRunAgentError::new("script exhausted")))
        }
    }

    fn engine(
        agent: ScriptedDurableAgent,
    ) -> DurableRunEngine<InMemoryDurableRunStore, ScriptedDurableAgent> {
        DurableRunEngine::new(InMemoryDurableRunStore::new(), agent)
    }

    fn start_options() -> DurableRunStartOptions {
        DurableRunStartOptions::new("run-1", "slack-thread-1")
    }

    fn persisted_event_payloads(
        engine: &DurableRunEngine<InMemoryDurableRunStore, ScriptedDurableAgent>,
    ) -> Vec<DurableRunEventPayload> {
        engine
            .store()
            .load_run("run-1")
            .expect("store loads")
            .expect("run exists")
            .events
            .into_iter()
            .map(|event| event.payload)
            .collect()
    }

    #[test]
    fn durable_run_finishes_after_persisting_stream_chunks() {
        let mut engine = engine(ScriptedDurableAgent::output([
            DurableRunAgentOutput::Finished {
                chunks: vec![
                    UiMessageChunk::start_with_message_id("assistant-1"),
                    UiMessageChunk::text_delta("text-1", "done"),
                    UiMessageChunk::finish(),
                ],
            },
        ]));

        let execution = engine.start(start_options()).expect("run starts");

        assert_eq!(execution.state, DurableRunState::Finished);
        assert_eq!(
            execution.chunks,
            vec![
                UiMessageChunk::start_with_message_id("assistant-1"),
                UiMessageChunk::text_delta("text-1", "done"),
                UiMessageChunk::finish(),
            ]
        );
        assert_eq!(engine.store().active_run_id("slack-thread-1"), None);

        let record = engine
            .store()
            .load_run("run-1")
            .expect("store loads")
            .expect("run exists");
        assert_eq!(record.state, DurableRunState::Finished);
        assert_eq!(record.next_stream_index(), 3);
        assert_eq!(
            record.events.last().map(|event| &event.payload),
            Some(&DurableRunEventPayload::RunFinished)
        );
    }

    #[test]
    fn durable_run_continues_through_local_tool_events_before_finish() {
        let mut engine = engine(ScriptedDurableAgent::output([
            DurableRunAgentOutput::Continue {
                chunks: vec![UiMessageChunk::tool_input_available(
                    "call-1",
                    "readFile",
                    json!({ "path": "README.md" }),
                )],
            },
            DurableRunAgentOutput::Continue {
                chunks: vec![UiMessageChunk::tool_output_available(
                    "call-1",
                    json!({ "contents": "hello" }),
                )],
            },
            DurableRunAgentOutput::Finished {
                chunks: vec![UiMessageChunk::finish()],
            },
        ]));

        let execution = engine.start(start_options()).expect("run starts");

        assert_eq!(execution.state, DurableRunState::Finished);
        assert_eq!(
            engine
                .stream_chunks_since("run-1", 0)
                .expect("stream chunks load"),
            vec![
                UiMessageChunk::tool_input_available(
                    "call-1",
                    "readFile",
                    json!({ "path": "README.md" }),
                ),
                UiMessageChunk::tool_output_available("call-1", json!({ "contents": "hello" })),
                UiMessageChunk::finish(),
            ]
        );
    }

    #[test]
    fn durable_run_pauses_for_tool_input_and_resumes() {
        let mut engine = engine(ScriptedDurableAgent::output([
            DurableRunAgentOutput::WaitingForInput {
                tool_call_id: "call-1".to_string(),
                chunks: vec![UiMessageChunk::tool_input_available(
                    "call-1",
                    "askUser",
                    json!({ "question": "Proceed?" }),
                )],
            },
            DurableRunAgentOutput::Finished {
                chunks: vec![
                    UiMessageChunk::tool_output_available("call-1", json!({ "answer": "yes" })),
                    UiMessageChunk::finish(),
                ],
            },
        ]));

        let first = engine.start(start_options()).expect("run starts");

        assert_eq!(first.state, DurableRunState::WaitingForInput);
        assert_eq!(
            engine.store().active_run_id("slack-thread-1"),
            Some("run-1")
        );

        let record = engine
            .store()
            .load_run("run-1")
            .expect("store loads")
            .expect("run exists");
        assert_eq!(
            record.pause,
            Some(DurableRunPause::ToolInput {
                tool_call_id: "call-1".to_string()
            })
        );

        let resumed = engine
            .resume(
                "run-1",
                DurableRunResume::ToolInput {
                    tool_call_id: "call-1".to_string(),
                    input: json!({ "answer": "yes" }),
                },
            )
            .expect("run resumes");

        assert_eq!(resumed.state, DurableRunState::Finished);
        assert_eq!(engine.store().active_run_id("slack-thread-1"), None);
        assert!(persisted_event_payloads(&engine).iter().any(|payload| {
            matches!(
                payload,
                DurableRunEventPayload::RunResumed {
                    resume: DurableRunResume::ToolInput { .. }
                }
            )
        }));
    }

    #[test]
    fn durable_run_pauses_for_approval_and_resumes() {
        let mut engine = engine(ScriptedDurableAgent::output([
            DurableRunAgentOutput::WaitingForApproval {
                approval_id: "approval-1".to_string(),
                tool_call_id: "call-1".to_string(),
                chunks: vec![UiMessageChunk::tool_approval_request(
                    "approval-1",
                    "call-1",
                )],
            },
            DurableRunAgentOutput::Finished {
                chunks: vec![
                    UiMessageChunk::tool_approval_response("approval-1", true),
                    UiMessageChunk::finish(),
                ],
            },
        ]));

        let first = engine.start(start_options()).expect("run starts");

        assert_eq!(first.state, DurableRunState::WaitingForApproval);

        let resumed = engine
            .resume(
                "run-1",
                DurableRunResume::ToolApproval {
                    approval_id: "approval-1".to_string(),
                    tool_call_id: "call-1".to_string(),
                    approved: true,
                    reason: Some("trusted".to_string()),
                },
            )
            .expect("approval resumes");

        assert_eq!(resumed.state, DurableRunState::Finished);
        assert_eq!(
            engine
                .stream_chunks_since("run-1", 1)
                .expect("tail stream chunks load"),
            vec![
                UiMessageChunk::tool_approval_response("approval-1", true),
                UiMessageChunk::finish(),
            ]
        );
    }

    #[test]
    fn durable_run_cancellation_transitions_through_canceling_and_releases_active_run() {
        let mut engine = engine(ScriptedDurableAgent::output([
            DurableRunAgentOutput::WaitingForInput {
                tool_call_id: "call-1".to_string(),
                chunks: vec![UiMessageChunk::tool_input_available(
                    "call-1",
                    "askUser",
                    json!({ "question": "Proceed?" }),
                )],
            },
        ]));

        engine.start(start_options()).expect("run starts");
        let canceled = engine
            .cancel("run-1", Some("slack stop action".to_string()))
            .expect("run cancels");

        assert_eq!(canceled.state, DurableRunState::Canceled);
        assert_eq!(engine.store().active_run_id("slack-thread-1"), None);

        let payloads = persisted_event_payloads(&engine);
        assert!(payloads.iter().any(|payload| {
            matches!(
                payload,
                DurableRunEventPayload::CancelRequested {
                    reason: Some(reason)
                } if reason == "slack stop action"
            )
        }));
        assert!(matches!(
            payloads.last(),
            Some(DurableRunEventPayload::RunCanceled { .. })
        ));
    }

    #[test]
    fn durable_run_failure_is_persisted_and_cleans_active_run() {
        let mut engine = engine(ScriptedDurableAgent::output([
            DurableRunAgentOutput::Failed {
                error: "model stream failed".to_string(),
                chunks: vec![UiMessageChunk::error("model stream failed")],
            },
        ]));

        let execution = engine.start(start_options()).expect("run records failure");

        assert_eq!(execution.state, DurableRunState::Failed);
        assert_eq!(engine.store().active_run_id("slack-thread-1"), None);
        assert!(persisted_event_payloads(&engine).iter().any(|payload| {
            matches!(
                payload,
                DurableRunEventPayload::RunFailed { error } if error == "model stream failed"
            )
        }));
    }

    #[test]
    fn durable_run_agent_error_is_persisted_before_error_return() {
        let mut engine = engine(ScriptedDurableAgent::new([Err(DurableRunAgentError::new(
            "executor crashed",
        ))]));

        let error = engine
            .start(start_options())
            .expect_err("agent error returns");

        assert_eq!(
            error,
            DurableRunError::AgentFailed {
                run_id: "run-1".to_string(),
                message: "executor crashed".to_string(),
            }
        );
        assert_eq!(engine.store().active_run_id("slack-thread-1"), None);
        assert!(persisted_event_payloads(&engine).iter().any(|payload| {
            matches!(
                payload,
                DurableRunEventPayload::RunFailed { error } if error == "executor crashed"
            )
        }));
    }

    #[test]
    fn durable_run_rejects_duplicate_active_start_for_conversation() {
        let mut engine = engine(ScriptedDurableAgent::output([
            DurableRunAgentOutput::WaitingForInput {
                tool_call_id: "call-1".to_string(),
                chunks: Vec::new(),
            },
        ]));

        engine.start(start_options()).expect("first run waits");

        let error = engine
            .start(DurableRunStartOptions::new("run-2", "slack-thread-1"))
            .expect_err("duplicate active run conflicts");

        assert_eq!(
            error,
            DurableRunError::ActiveRunConflict {
                conversation_id: "slack-thread-1".to_string(),
                existing_run_id: "run-1".to_string(),
            }
        );
    }

    #[test]
    fn durable_run_rejects_duplicate_resume_while_running() {
        let mut store = InMemoryDurableRunStore::new();
        store
            .claim_active_run("slack-thread-1", "run-1")
            .expect("claim succeeds");
        store
            .create_run("run-1".to_string(), "slack-thread-1".to_string())
            .expect("create succeeds");
        store
            .save_run_state("run-1", DurableRunState::Running, None)
            .expect("state saves");

        let mut engine = DurableRunEngine::new(
            store,
            ScriptedDurableAgent::output([DurableRunAgentOutput::Finished { chunks: Vec::new() }]),
        );

        let error = engine
            .resume(
                "run-1",
                DurableRunResume::ToolInput {
                    tool_call_id: "call-1".to_string(),
                    input: json!({}),
                },
            )
            .expect_err("running run cannot resume");

        assert_eq!(
            error,
            DurableRunError::DuplicateResume {
                run_id: "run-1".to_string(),
            }
        );
    }

    #[test]
    fn durable_run_crash_resume_uses_persisted_waiting_state_and_stream_tail() {
        let mut first_engine = engine(ScriptedDurableAgent::output([
            DurableRunAgentOutput::WaitingForInput {
                tool_call_id: "call-1".to_string(),
                chunks: vec![UiMessageChunk::text_delta("text-1", "before pause")],
            },
        ]));
        first_engine.start(start_options()).expect("run pauses");
        let (store_after_crash, _) = first_engine.into_parts();

        let mut resumed_engine = DurableRunEngine::new(
            store_after_crash,
            ScriptedDurableAgent::output([DurableRunAgentOutput::Finished {
                chunks: vec![
                    UiMessageChunk::text_delta("text-1", " after resume"),
                    UiMessageChunk::finish(),
                ],
            }]),
        );

        assert_eq!(
            resumed_engine
                .stream_chunks_since("run-1", 0)
                .expect("previous chunks survive"),
            vec![UiMessageChunk::text_delta("text-1", "before pause")]
        );

        let resumed = resumed_engine
            .resume(
                "run-1",
                DurableRunResume::ToolInput {
                    tool_call_id: "call-1".to_string(),
                    input: json!({ "answer": "continue" }),
                },
            )
            .expect("run resumes after crash");

        assert_eq!(resumed.state, DurableRunState::Finished);
        assert_eq!(
            resumed_engine
                .stream_chunks_since("run-1", 1)
                .expect("tail chunks survive"),
            vec![
                UiMessageChunk::text_delta("text-1", " after resume"),
                UiMessageChunk::finish(),
            ]
        );
    }
}
