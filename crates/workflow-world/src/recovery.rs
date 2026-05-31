use crate::data::ResolveData;
use crate::error::WorldError;
use crate::interfaces::{Queue, Runs};
use crate::queue::{QueuePayload, ValidQueueName, WorkflowInvokePayload};
use crate::runs::{ListWorkflowRunsParams, WorkflowRunStatus};

/// Summary from re-enqueueing active workflow runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReenqueueSummary {
    pub reenqueued: usize,
    pub failed: Vec<String>,
}

/// Re-enqueue all pending and running workflow runs after a world restart.
pub async fn reenqueue_active_runs<R, Q>(
    runs: &R,
    queue: &Q,
) -> Result<ReenqueueSummary, WorldError>
where
    R: Runs + Sync,
    Q: Queue + Sync,
{
    let mut summary = ReenqueueSummary::default();

    for status in [WorkflowRunStatus::Pending, WorkflowRunStatus::Running] {
        let mut cursor = None;
        let mut has_more = true;
        while has_more {
            let page = runs
                .list_runs(Some(ListWorkflowRunsParams {
                    workflow_name: None,
                    status: Some(status),
                    pagination: Some(crate::data::PaginationOptions {
                        limit: None,
                        cursor: cursor.clone(),
                        sort_order: None,
                    }),
                    resolve_data: Some(ResolveData::None),
                }))
                .await?;

            for run in page.data {
                let queue_name = ValidQueueName::workflow(&run.workflow_name);
                let payload = QueuePayload::Workflow(WorkflowInvokePayload {
                    run_id: run.run_id.clone(),
                    trace_carrier: None,
                    requested_at: None,
                    server_error_retry_count: None,
                    step_id: None,
                    step_name: None,
                    run_input: None,
                });
                match queue.queue(&queue_name, payload, None).await {
                    Ok(_) => summary.reenqueued += 1,
                    Err(error) => {
                        summary
                            .failed
                            .push(format!("{}: {}", run.run_id, error.message()))
                    }
                }
            }

            has_more = page.has_more;
            cursor = page.cursor;
        }
    }

    Ok(summary)
}
