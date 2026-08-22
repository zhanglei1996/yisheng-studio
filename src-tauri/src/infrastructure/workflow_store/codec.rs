use rusqlite::{Connection, OptionalExtension};

use crate::{
    domain::{JobStage, JobStatus},
    error::AppError,
    workflow::{
        FailureClassification, NodeOutcome, NodeRunRecord, NodeRunState, RetryClassification,
        WorkflowRunRecord, WorkflowRunState,
    },
};

pub(super) fn query_run(
    connection: &Connection,
    run_id: &str,
) -> Result<WorkflowRunRecord, AppError> {
    let raw = connection
        .query_row(
            "SELECT id, workflow_id, workflow_version, project_id, legacy_job_id, status,
                    current_node_id, stage, progress, checkpoint, error_message,
                    cancel_requested, created_at, updated_at
             FROM workflow_runs WHERE id=?1",
            [run_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, i64>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, String>(13)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(run_id.into()))?;
    Ok(WorkflowRunRecord {
        id: raw.0,
        workflow_id: raw.1,
        workflow_version: raw.2.max(1) as u32,
        project_id: raw.3,
        legacy_job_id: raw.4,
        state: parse_run_state(&raw.5)?,
        current_node_id: raw.6,
        stage: JobStage::try_from(raw.7.as_str()).map_err(AppError::Validation)?,
        progress: raw.8.clamp(0, 100) as u8,
        checkpoint: raw.9,
        error_message: raw.10,
        cancel_requested: raw.11 != 0,
        created_at: raw.12,
        updated_at: raw.13,
    })
}

pub(super) fn query_node(
    connection: &Connection,
    node_run_id: &str,
) -> Result<NodeRunRecord, AppError> {
    connection
        .query_row(
            "SELECT id, run_id, node_id, node_version, attempt, status,
                    input_artifacts_json, output_artifacts_json, checkpoint,
                    error_class, error_message FROM node_runs WHERE id=?1",
            [node_run_id],
            node_run_from_row,
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(node_run_id.into()))
}

pub(super) fn node_run_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<NodeRunRecord> {
    let inputs = row.get::<_, String>(6)?;
    let outputs = row.get::<_, String>(7)?;
    Ok(NodeRunRecord {
        id: row.get(0)?,
        run_id: row.get(1)?,
        node_id: row.get(2)?,
        node_version: row.get::<_, i64>(3)?.max(1) as u32,
        attempt: row.get::<_, i64>(4)?.max(1) as u32,
        state: parse_node_state(&row.get::<_, String>(5)?).map_err(to_sql_error)?,
        input_artifact_ids: parse_artifact_ids(&inputs).map_err(to_sql_error)?,
        output_artifact_ids: parse_artifact_ids(&outputs).map_err(to_sql_error)?,
        checkpoint: row.get(8)?,
        error_class: row.get(9)?,
        error_message: row.get(10)?,
    })
}

pub(super) type OutcomeFields = (
    NodeRunState,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

pub(super) fn outcome_fields(outcome: &NodeOutcome) -> Result<OutcomeFields, AppError> {
    let empty = "[]".to_string();
    Ok(match outcome {
        NodeOutcome::Completed {
            output_artifact_ids,
            checkpoint,
        } => (
            NodeRunState::Succeeded,
            serde_json::to_string(output_artifact_ids)
                .map_err(|error| AppError::Validation(error.to_string()))?,
            checkpoint.clone(),
            None,
            None,
        ),
        NodeOutcome::WaitingForInput { reason, checkpoint } => (
            NodeRunState::WaitingForInput,
            empty,
            checkpoint.clone(),
            Some("waiting_for_input".into()),
            Some(reason.clone()),
        ),
        NodeOutcome::Retryable {
            classification,
            message,
            checkpoint,
            ..
        } => (
            NodeRunState::Retryable,
            empty,
            checkpoint.clone(),
            Some(retry_class(*classification).into()),
            Some(message.clone()),
        ),
        NodeOutcome::Failed {
            classification,
            message,
            checkpoint,
        } => (
            NodeRunState::Failed,
            empty,
            checkpoint.clone(),
            Some(failure_class(*classification).into()),
            Some(message.clone()),
        ),
        NodeOutcome::Cancelled { checkpoint } => (
            NodeRunState::Cancelled,
            empty,
            checkpoint.clone(),
            None,
            None,
        ),
    })
}

pub(super) fn run_state(value: WorkflowRunState) -> &'static str {
    match value {
        WorkflowRunState::Queued => "queued",
        WorkflowRunState::Running => "running",
        WorkflowRunState::WaitingForInput => "waiting_for_input",
        WorkflowRunState::Retryable => "retryable",
        WorkflowRunState::Succeeded => "succeeded",
        WorkflowRunState::Failed => "failed",
        WorkflowRunState::Cancelled => "cancelled",
    }
}

pub(super) fn valid_run_transition(from: WorkflowRunState, to: WorkflowRunState) -> bool {
    matches!(
        (from, to),
        (WorkflowRunState::Queued, WorkflowRunState::Running)
            | (WorkflowRunState::Queued, WorkflowRunState::Failed)
            | (WorkflowRunState::Queued, WorkflowRunState::Cancelled)
            | (WorkflowRunState::Running, WorkflowRunState::Running)
            | (WorkflowRunState::Running, WorkflowRunState::WaitingForInput)
            | (WorkflowRunState::Running, WorkflowRunState::Retryable)
            | (WorkflowRunState::Running, WorkflowRunState::Succeeded)
            | (WorkflowRunState::Running, WorkflowRunState::Failed)
            | (WorkflowRunState::Running, WorkflowRunState::Cancelled)
            | (WorkflowRunState::WaitingForInput, WorkflowRunState::Queued)
            | (
                WorkflowRunState::WaitingForInput,
                WorkflowRunState::Cancelled
            )
            | (WorkflowRunState::Retryable, WorkflowRunState::Queued)
            | (WorkflowRunState::Retryable, WorkflowRunState::Failed)
            | (WorkflowRunState::Retryable, WorkflowRunState::Cancelled)
            | (WorkflowRunState::Failed, WorkflowRunState::Queued)
    )
}

fn parse_run_state(value: &str) -> Result<WorkflowRunState, AppError> {
    match value {
        "queued" => Ok(WorkflowRunState::Queued),
        "running" => Ok(WorkflowRunState::Running),
        "waiting_for_input" => Ok(WorkflowRunState::WaitingForInput),
        "retryable" => Ok(WorkflowRunState::Retryable),
        "succeeded" => Ok(WorkflowRunState::Succeeded),
        "failed" => Ok(WorkflowRunState::Failed),
        "cancelled" => Ok(WorkflowRunState::Cancelled),
        _ => Err(AppError::Validation(format!("未知工作流运行状态：{value}"))),
    }
}

pub(super) fn node_state(value: NodeRunState) -> &'static str {
    match value {
        NodeRunState::Running => "running",
        NodeRunState::WaitingForInput => "waiting_for_input",
        NodeRunState::Retryable => "retryable",
        NodeRunState::Succeeded => "succeeded",
        NodeRunState::Failed => "failed",
        NodeRunState::Cancelled => "cancelled",
    }
}

fn parse_node_state(value: &str) -> Result<NodeRunState, AppError> {
    match value {
        "running" => Ok(NodeRunState::Running),
        "waiting_for_input" => Ok(NodeRunState::WaitingForInput),
        "retryable" => Ok(NodeRunState::Retryable),
        "succeeded" => Ok(NodeRunState::Succeeded),
        "failed" => Ok(NodeRunState::Failed),
        "cancelled" => Ok(NodeRunState::Cancelled),
        _ => Err(AppError::Validation(format!("未知节点运行状态：{value}"))),
    }
}

pub(super) fn legacy_job_status(state: WorkflowRunState) -> JobStatus {
    match state {
        WorkflowRunState::Queued => JobStatus::Queued,
        WorkflowRunState::Running => JobStatus::Running,
        WorkflowRunState::WaitingForInput => JobStatus::WaitingUser,
        WorkflowRunState::Retryable => JobStatus::Paused,
        WorkflowRunState::Succeeded => JobStatus::Succeeded,
        WorkflowRunState::Failed => JobStatus::Failed,
        WorkflowRunState::Cancelled => JobStatus::Cancelled,
    }
}

fn retry_class(value: RetryClassification) -> &'static str {
    match value {
        RetryClassification::Transient => "transient",
        RetryClassification::RateLimited => "rate_limited",
        RetryClassification::Dependency => "dependency",
    }
}

fn failure_class(value: FailureClassification) -> &'static str {
    match value {
        FailureClassification::Validation => "validation",
        FailureClassification::Dependency => "dependency",
        FailureClassification::ProviderTerminal => "provider_terminal",
        FailureClassification::Media => "media",
        FailureClassification::Internal => "internal",
    }
}

fn parse_artifact_ids(value: &str) -> Result<Vec<String>, AppError> {
    serde_json::from_str(value).map_err(|error| AppError::Validation(error.to_string()))
}

fn to_sql_error(error: AppError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            error.to_string(),
        )),
    )
}
