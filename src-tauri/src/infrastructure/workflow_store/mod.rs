mod codec;
mod schema;
mod shared;

pub(crate) use shared::SharedWorkflowStore;

use codec::*;

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::{JobStage, JobSummary},
    error::AppError,
    workflow::{
        CreateWorkflowRun, NodeOutcome, NodeRunRecord, NodeRunState, RunEventRecord, RunUpdate,
        WorkflowNodeSpec, WorkflowRunRecord, WorkflowRunState, WorkflowStore,
    },
};

#[allow(dead_code)]
pub(crate) struct WorkflowSqliteStore {
    connection: Connection,
}

#[allow(dead_code)]
impl WorkflowSqliteStore {
    pub(crate) fn open(path: &Path) -> Result<Self, AppError> {
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             BEGIN IMMEDIATE;",
        )?;
        let migration = schema::migrate(&connection);
        match migration {
            Ok(()) => connection.execute_batch("COMMIT")?,
            Err(error) => {
                let _ = connection.execute_batch("ROLLBACK");
                return Err(error);
            }
        }
        recover_interrupted(&connection)?;
        Ok(Self { connection })
    }

    fn append_event(
        connection: &Connection,
        run_id: &str,
        node_run_id: Option<&str>,
        kind: &str,
        payload: &serde_json::Value,
    ) -> Result<(), AppError> {
        connection.execute(
            "INSERT INTO run_events(run_id, node_run_id, kind, payload_json)
             VALUES (?1, ?2, ?3, ?4)",
            params![run_id, node_run_id, kind, payload.to_string()],
        )?;
        Ok(())
    }

    fn append_projection(connection: &Connection, run: &WorkflowRunRecord) -> Result<(), AppError> {
        let projection = JobSummary {
            id: run.legacy_job_id.clone().unwrap_or_else(|| run.id.clone()),
            project_id: run.project_id.clone(),
            stage: run.stage,
            progress: run.progress,
            status: legacy_job_status(run.state),
            checkpoint: run.checkpoint.clone(),
            error_message: run.error_message.clone(),
            created_at: run.created_at.clone(),
            updated_at: run.updated_at.clone(),
        };
        let payload = serde_json::to_value(projection)
            .map_err(|error| AppError::Validation(error.to_string()))?;
        Self::append_event(connection, &run.id, None, "job_projection", &payload)
    }
}

impl WorkflowStore for WorkflowSqliteStore {
    fn create_run(
        &self,
        request: &CreateWorkflowRun,
        initial_stage: JobStage,
    ) -> Result<WorkflowRunRecord, AppError> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO workflow_runs
             (id, workflow_id, workflow_version, project_id, legacy_job_id, status, stage)
             VALUES (?1, ?2, ?3, ?4, ?5, 'queued', ?6)",
            params![
                request.id,
                request.workflow_id,
                request.workflow_version,
                request.project_id,
                request.legacy_job_id,
                initial_stage.as_str()
            ],
        )?;
        let run = query_run(&transaction, &request.id)?;
        Self::append_event(
            &transaction,
            &run.id,
            None,
            "run_created",
            &json!({"workflowId": run.workflow_id, "workflowVersion": run.workflow_version}),
        )?;
        Self::append_projection(&transaction, &run)?;
        transaction.commit()?;
        Ok(run)
    }

    fn get_run(&self, run_id: &str) -> Result<WorkflowRunRecord, AppError> {
        query_run(&self.connection, run_id)
    }

    fn find_run_by_legacy_job_id(
        &self,
        legacy_job_id: &str,
    ) -> Result<Option<WorkflowRunRecord>, AppError> {
        let run_id = self
            .connection
            .query_row(
                "SELECT id FROM workflow_runs WHERE legacy_job_id=?1",
                [legacy_job_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        run_id
            .map(|run_id| query_run(&self.connection, &run_id))
            .transpose()
    }

    fn latest_node_run(
        &self,
        run_id: &str,
        node_id: &str,
    ) -> Result<Option<NodeRunRecord>, AppError> {
        self.connection
            .query_row(
                "SELECT id, run_id, node_id, node_version, attempt, status,
                        input_artifacts_json, output_artifacts_json, checkpoint,
                        error_class, error_message
                 FROM node_runs WHERE run_id=?1 AND node_id=?2 ORDER BY attempt DESC LIMIT 1",
                params![run_id, node_id],
                node_run_from_row,
            )
            .optional()
            .map_err(AppError::from)
    }

    fn begin_node(
        &self,
        run_id: &str,
        spec: &WorkflowNodeSpec,
        attempt: u32,
        input_artifact_ids: &[String],
    ) -> Result<NodeRunRecord, AppError> {
        if self
            .latest_node_run(run_id, &spec.id)?
            .is_some_and(|record| record.state == NodeRunState::Succeeded)
        {
            return Err(AppError::Validation(
                "已完成节点不能创建重复执行记录".into(),
            ));
        }
        let id = Uuid::new_v4().to_string();
        let inputs = serde_json::to_string(input_artifact_ids)
            .map_err(|error| AppError::Validation(error.to_string()))?;
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO node_runs
             (id, run_id, node_id, node_version, attempt, stage, resource_class, status,
              input_artifacts_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'running', ?8)",
            params![
                id,
                run_id,
                spec.id,
                spec.version,
                attempt,
                spec.stage.as_str(),
                spec.resource_class.as_str(),
                inputs
            ],
        )?;
        Self::append_event(
            &transaction,
            run_id,
            Some(&id),
            "node_started",
            &json!({"nodeId": spec.id, "nodeVersion": spec.version, "attempt": attempt}),
        )?;
        let record = query_node(&transaction, &id)?;
        transaction.commit()?;
        Ok(record)
    }

    fn finish_node(&self, node_run_id: &str, outcome: &NodeOutcome) -> Result<(), AppError> {
        let (state, outputs, checkpoint, error_class, error_message) = outcome_fields(outcome)?;
        let transaction = self.connection.unchecked_transaction()?;
        let run_id: String = transaction.query_row(
            "SELECT run_id FROM node_runs WHERE id=?1",
            [node_run_id],
            |row| row.get(0),
        )?;
        let changed = transaction.execute(
            "UPDATE node_runs SET status=?2, output_artifacts_json=?3, checkpoint=?4,
             error_class=?5, error_message=?6, finished_at=CURRENT_TIMESTAMP
             WHERE id=?1 AND status='running'",
            params![
                node_run_id,
                node_state(state),
                outputs,
                checkpoint,
                error_class,
                error_message
            ],
        )?;
        if changed != 1 {
            return Err(AppError::Validation(
                "只有运行中的节点可以写入执行结果".into(),
            ));
        }
        Self::append_event(
            &transaction,
            &run_id,
            Some(node_run_id),
            "node_finished",
            &serde_json::to_value(outcome)
                .map_err(|error| AppError::Validation(error.to_string()))?,
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn update_run(&self, run_id: &str, update: &RunUpdate) -> Result<(), AppError> {
        let transaction = self.connection.unchecked_transaction()?;
        let current = query_run(&transaction, run_id)?;
        if !valid_run_transition(current.state, update.state) {
            return Err(AppError::Validation(format!(
                "工作流不能从 {} 转为 {}",
                run_state(current.state),
                run_state(update.state)
            )));
        }
        let changed = transaction.execute(
            "UPDATE workflow_runs SET status=?2, current_node_id=?3, stage=?4,
             progress=?5, checkpoint=?6, error_message=?7, updated_at=CURRENT_TIMESTAMP
             WHERE id=?1",
            params![
                run_id,
                run_state(update.state),
                update.current_node_id,
                update.stage.as_str(),
                update.progress.min(100),
                update.checkpoint,
                update.error_message
            ],
        )?;
        if changed != 1 {
            return Err(AppError::NotFound(run_id.into()));
        }
        let run = query_run(&transaction, run_id)?;
        Self::append_event(
            &transaction,
            run_id,
            None,
            "run_state_changed",
            &json!({"status": run_state(run.state), "currentNodeId": run.current_node_id}),
        )?;
        Self::append_projection(&transaction, &run)?;
        transaction.commit()?;
        Ok(())
    }

    fn resume_run(&self, run_id: &str) -> Result<(), AppError> {
        let run = self.get_run(run_id)?;
        if !matches!(
            run.state,
            WorkflowRunState::WaitingForInput
                | WorkflowRunState::Retryable
                | WorkflowRunState::Failed
        ) {
            return Err(AppError::Validation(
                "只有等待输入、失败或可重试的工作流可以继续".into(),
            ));
        }
        self.update_run(
            run_id,
            &RunUpdate {
                state: WorkflowRunState::Queued,
                current_node_id: run.current_node_id,
                stage: run.stage,
                progress: run.progress,
                checkpoint: run.checkpoint,
                error_message: None,
            },
        )
    }

    fn request_cancel(&self, run_id: &str) -> Result<(), AppError> {
        let changed = self.connection.execute(
            "UPDATE workflow_runs SET cancel_requested=1, updated_at=CURRENT_TIMESTAMP
             WHERE id=?1 AND status NOT IN ('succeeded','failed','cancelled')",
            [run_id],
        )?;
        if changed != 1 {
            return Err(AppError::Validation(
                "工作流不存在或已经结束，不能取消".into(),
            ));
        }
        Self::append_event(
            &self.connection,
            run_id,
            None,
            "cancel_requested",
            &json!({}),
        )
    }

    fn record_event(
        &self,
        run_id: &str,
        kind: &str,
        payload: &serde_json::Value,
    ) -> Result<(), AppError> {
        Self::append_event(&self.connection, run_id, None, kind, payload)
    }

    fn complete_external_node(
        &self,
        legacy_job_id: &str,
        node_id: &str,
        node_version: u32,
        output_artifact_ids: &[String],
        checkpoint: &str,
    ) -> Result<(), AppError> {
        let transaction = self.connection.unchecked_transaction()?;
        let run_id: String = transaction
            .query_row(
                "SELECT id FROM workflow_runs WHERE legacy_job_id=?1",
                [legacy_job_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound(format!("workflow for job {legacy_job_id}")))?;
        let run = query_run(&transaction, &run_id)?;
        if run.state == WorkflowRunState::Succeeded {
            return Ok(());
        }
        if run.state != WorkflowRunState::WaitingForInput
            || run.current_node_id.as_deref() != Some(node_id)
        {
            return Err(AppError::Validation(
                "当前工作流未等待该外部发布步骤".into(),
            ));
        }
        let node_run_id: String = transaction.query_row(
            "SELECT id FROM node_runs WHERE run_id=?1 AND node_id=?2 AND node_version=?3
             ORDER BY attempt DESC LIMIT 1",
            params![run_id, node_id, node_version],
            |row| row.get(0),
        )?;
        let outputs = serde_json::to_string(output_artifact_ids)
            .map_err(|error| AppError::Validation(error.to_string()))?;
        let changed = transaction.execute(
            "UPDATE node_runs SET status='succeeded', output_artifacts_json=?2,
             checkpoint=?3, error_class=NULL, error_message=NULL,
             finished_at=CURRENT_TIMESTAMP WHERE id=?1 AND status='waiting_for_input'",
            params![node_run_id, outputs, checkpoint],
        )?;
        if changed != 1 {
            return Err(AppError::Validation("外部发布节点不处于等待状态".into()));
        }
        transaction.execute(
            "UPDATE workflow_runs SET status='succeeded', progress=100, checkpoint=?2,
             error_message=NULL, updated_at=CURRENT_TIMESTAMP WHERE id=?1",
            params![run_id, checkpoint],
        )?;
        Self::append_event(
            &transaction,
            &run_id,
            Some(&node_run_id),
            "external_node_completed",
            &json!({"nodeId": node_id, "nodeVersion": node_version, "outputs": output_artifact_ids}),
        )?;
        let completed = query_run(&transaction, &run_id)?;
        Self::append_projection(&transaction, &completed)?;
        transaction.commit()?;
        Ok(())
    }

    fn project_job_summary(&self, run_id: &str) -> Result<JobSummary, AppError> {
        let payload: String = self
            .connection
            .query_row(
                "SELECT payload_json FROM run_events
                 WHERE run_id=?1 AND kind='job_projection' ORDER BY id DESC LIMIT 1",
                [run_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound(format!("job projection for {run_id}")))?;
        serde_json::from_str(&payload).map_err(|error| AppError::Validation(error.to_string()))
    }

    fn list_node_runs(&self, run_id: &str) -> Result<Vec<NodeRunRecord>, AppError> {
        let mut statement = self.connection.prepare(
            "SELECT id, run_id, node_id, node_version, attempt, status,
                    input_artifacts_json, output_artifacts_json, checkpoint,
                    error_class, error_message
             FROM node_runs WHERE run_id=?1 ORDER BY rowid",
        )?;
        let records = statement
            .query_map([run_id], node_run_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from)?;
        Ok(records)
    }

    fn list_events(&self, run_id: &str) -> Result<Vec<RunEventRecord>, AppError> {
        let mut statement = self.connection.prepare(
            "SELECT id, run_id, node_run_id, kind, payload_json, created_at
             FROM run_events WHERE run_id=?1 ORDER BY id",
        )?;
        let events = statement
            .query_map([run_id], |row| {
                Ok(RunEventRecord {
                    id: row.get(0)?,
                    run_id: row.get(1)?,
                    node_run_id: row.get(2)?,
                    kind: row.get(3)?,
                    payload_json: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from)?;
        Ok(events)
    }
}

pub(crate) fn migrate(connection: &Connection) -> Result<(), AppError> {
    schema::migrate(connection)
}

pub(crate) fn recover_interrupted(connection: &Connection) -> Result<(), AppError> {
    let run_ids = {
        let mut statement =
            connection.prepare("SELECT id FROM workflow_runs WHERE status='running'")?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids
    };
    if run_ids.is_empty() {
        return Ok(());
    }
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(
        "UPDATE node_runs SET status='retryable', error_class='interrupted',
         error_message='应用中断，等待恢复', finished_at=CURRENT_TIMESTAMP
         WHERE status='running'",
        [],
    )?;
    transaction.execute(
        "UPDATE workflow_runs SET status='retryable', error_message='应用中断，等待恢复',
         updated_at=CURRENT_TIMESTAMP WHERE status='running'",
        [],
    )?;
    for run_id in run_ids {
        let run = query_run(&transaction, &run_id)?;
        WorkflowSqliteStore::append_event(
            &transaction,
            &run_id,
            None,
            "run_recovered",
            &json!({"reason": "interrupted"}),
        )?;
        WorkflowSqliteStore::append_projection(&transaction, &run)?;
    }
    transaction.commit()?;
    Ok(())
}
