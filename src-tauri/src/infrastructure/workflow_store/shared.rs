use std::{path::Path, sync::Mutex};

use serde_json::Value;

use crate::{
    domain::{JobStage, JobSummary},
    error::AppError,
    workflow::{
        CreateWorkflowRun, NodeOutcome, NodeRunRecord, RunEventRecord, RunUpdate, WorkflowNodeSpec,
        WorkflowRunRecord, WorkflowStore,
    },
};

use super::WorkflowSqliteStore;

/// Makes the single-connection SQLite store safe to share with async workflow
/// commands while ensuring no connection guard is held across a node await.
pub(crate) struct SharedWorkflowStore(Mutex<WorkflowSqliteStore>);

impl SharedWorkflowStore {
    pub(crate) fn open(path: &Path) -> Result<Self, AppError> {
        Ok(Self(Mutex::new(WorkflowSqliteStore::open(path)?)))
    }

    fn with<T>(
        &self,
        operation: impl FnOnce(&WorkflowSqliteStore) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        operation(&self.0.lock().expect("workflow store mutex poisoned"))
    }
}

impl WorkflowStore for SharedWorkflowStore {
    fn create_run(
        &self,
        request: &CreateWorkflowRun,
        initial_stage: JobStage,
    ) -> Result<WorkflowRunRecord, AppError> {
        self.with(|store| store.create_run(request, initial_stage))
    }

    fn get_run(&self, run_id: &str) -> Result<WorkflowRunRecord, AppError> {
        self.with(|store| store.get_run(run_id))
    }

    fn find_run_by_legacy_job_id(
        &self,
        legacy_job_id: &str,
    ) -> Result<Option<WorkflowRunRecord>, AppError> {
        self.with(|store| store.find_run_by_legacy_job_id(legacy_job_id))
    }

    fn latest_node_run(
        &self,
        run_id: &str,
        node_id: &str,
    ) -> Result<Option<NodeRunRecord>, AppError> {
        self.with(|store| store.latest_node_run(run_id, node_id))
    }

    fn begin_node(
        &self,
        run_id: &str,
        spec: &WorkflowNodeSpec,
        attempt: u32,
        input_artifact_ids: &[String],
    ) -> Result<NodeRunRecord, AppError> {
        self.with(|store| store.begin_node(run_id, spec, attempt, input_artifact_ids))
    }

    fn finish_node(&self, node_run_id: &str, outcome: &NodeOutcome) -> Result<(), AppError> {
        self.with(|store| store.finish_node(node_run_id, outcome))
    }

    fn update_run(&self, run_id: &str, update: &RunUpdate) -> Result<(), AppError> {
        self.with(|store| store.update_run(run_id, update))
    }

    fn resume_run(&self, run_id: &str) -> Result<(), AppError> {
        self.with(|store| store.resume_run(run_id))
    }

    fn request_cancel(&self, run_id: &str) -> Result<(), AppError> {
        self.with(|store| store.request_cancel(run_id))
    }

    fn record_event(&self, run_id: &str, kind: &str, payload: &Value) -> Result<(), AppError> {
        self.with(|store| store.record_event(run_id, kind, payload))
    }

    fn complete_external_node(
        &self,
        legacy_job_id: &str,
        node_id: &str,
        node_version: u32,
        output_artifact_ids: &[String],
        checkpoint: &str,
    ) -> Result<(), AppError> {
        self.with(|store| {
            store.complete_external_node(
                legacy_job_id,
                node_id,
                node_version,
                output_artifact_ids,
                checkpoint,
            )
        })
    }

    fn project_job_summary(&self, run_id: &str) -> Result<JobSummary, AppError> {
        self.with(|store| store.project_job_summary(run_id))
    }

    fn list_node_runs(&self, run_id: &str) -> Result<Vec<NodeRunRecord>, AppError> {
        self.with(|store| store.list_node_runs(run_id))
    }

    fn list_events(&self, run_id: &str) -> Result<Vec<RunEventRecord>, AppError> {
        self.with(|store| store.list_events(run_id))
    }
}
