use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::{
    application::production_workflow,
    commands,
    domain::{JobStage, JobStatus, JobSummary},
    error::AppError,
    workflow::{CancellationToken, WorkflowRunState, WorkflowRunner, WorkflowStore},
    AppState,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowIntentResult {
    pub job: JobSummary,
    pub workflow_state: WorkflowRunState,
    pub current_node_id: Option<String>,
    pub next_action: WorkflowNextAction,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowNextAction {
    None,
    Continue,
    Retry,
    OpenEditor,
}

#[tauri::command]
pub fn workflow_enqueue(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
) -> Result<WorkflowIntentResult, AppError> {
    let job = commands::job_enqueue(app.clone(), state.clone(), project_id)?;
    let definition = production_workflow::definition(app, job.id.clone())?;
    let run = WorkflowRunner::new(state.workflow_store.as_ref()).create_run(
        &definition,
        job.project_id.clone(),
        Some(job.id.clone()),
    )?;
    Ok(intent_result(job, run.state, None))
}

#[tauri::command]
pub async fn workflow_start(
    app: AppHandle,
    state: State<'_, AppState>,
    job_id: String,
) -> Result<WorkflowIntentResult, AppError> {
    drive(app, state, job_id, false).await
}

#[tauri::command]
pub async fn workflow_continue(
    app: AppHandle,
    state: State<'_, AppState>,
    job_id: String,
) -> Result<WorkflowIntentResult, AppError> {
    drive(app, state, job_id, true).await
}

#[tauri::command]
pub async fn workflow_retry(
    app: AppHandle,
    state: State<'_, AppState>,
    job_id: String,
) -> Result<WorkflowIntentResult, AppError> {
    drive(app, state, job_id, true).await
}

#[tauri::command]
pub fn workflow_pause(
    app: AppHandle,
    state: State<'_, AppState>,
    job_id: String,
) -> Result<JobSummary, AppError> {
    if let Some(run) = state.workflow_store.find_run_by_legacy_job_id(&job_id)? {
        state
            .workflow_store
            .record_event(&run.id, "pause_requested", &serde_json::json!({}))?;
    }
    commands::job_pause(app, state, job_id)
}

#[tauri::command]
pub fn workflow_cancel(
    app: AppHandle,
    state: State<'_, AppState>,
    job_id: String,
) -> Result<JobSummary, AppError> {
    if let Some(run) = state.workflow_store.find_run_by_legacy_job_id(&job_id)? {
        state.workflow_store.request_cancel(&run.id)?;
        if let Some(token) = state
            .workflow_cancellations
            .lock()
            .expect("workflow cancellation registry poisoned")
            .get(&run.id)
        {
            token.cancel();
        }
    }
    commands::job_cancel(app, state, job_id)
}

async fn drive(
    app: AppHandle,
    state: State<'_, AppState>,
    job_id: String,
    resume: bool,
) -> Result<WorkflowIntentResult, AppError> {
    let definition = production_workflow::definition(app.clone(), job_id.clone())?;
    let mut run = match state.workflow_store.find_run_by_legacy_job_id(&job_id)? {
        Some(run) => run,
        None => {
            let job = state
                .database
                .lock()
                .expect("database mutex poisoned")
                .get_job(&job_id)?;
            if let Some(result) = legacy_terminal_or_editor_intent(&job) {
                let _ = app.emit("job://state", &job);
                return Ok(result);
            }
            WorkflowRunner::new(state.workflow_store.as_ref()).create_run(
                &definition,
                job.project_id,
                Some(job_id.clone()),
            )?
        }
    };
    if run.state == WorkflowRunState::WaitingForInput
        && run.current_node_id.as_deref() == Some(production_workflow::EXPORT_NODE_ID)
    {
        let job = current_job(&state, &app, &job_id)?;
        return Ok(intent_result(job, run.state, run.current_node_id.clone()));
    }
    if resume
        && matches!(
            run.state,
            WorkflowRunState::WaitingForInput
                | WorkflowRunState::Retryable
                | WorkflowRunState::Failed
        )
    {
        WorkflowRunner::new(state.workflow_store.as_ref()).resume(&run.id)?;
        run = state.workflow_store.get_run(&run.id)?;
    }
    if matches!(
        run.state,
        WorkflowRunState::Succeeded | WorkflowRunState::Cancelled
    ) {
        let job = current_job(&state, &app, &job_id)?;
        return Ok(intent_result(job, run.state, run.current_node_id));
    }

    let cancellation = CancellationToken::default();
    state
        .workflow_cancellations
        .lock()
        .expect("workflow cancellation registry poisoned")
        .insert(run.id.clone(), cancellation.clone());
    let _workflow_permit = state.workflow_scheduler.acquire_workflow().await?;
    let runner = WorkflowRunner::with_scheduler(
        state.workflow_store.as_ref(),
        state.workflow_scheduler.as_ref(),
    );
    let result = runner
        .run_until_blocked(&definition, &run.id, cancellation)
        .await;
    state
        .workflow_cancellations
        .lock()
        .expect("workflow cancellation registry poisoned")
        .remove(&run.id);
    let result = result?;
    let job = current_job(&state, &app, &job_id)?;
    Ok(intent_result(job, result.state, result.node_id))
}

fn legacy_terminal_or_editor_intent(job: &JobSummary) -> Option<WorkflowIntentResult> {
    let (state, node) = match job.status {
        JobStatus::Succeeded => (WorkflowRunState::Succeeded, Some("export_publish".into())),
        JobStatus::Cancelled => (WorkflowRunState::Cancelled, None),
        JobStatus::WaitingUser => (
            WorkflowRunState::WaitingForInput,
            Some(legacy_editor_node(job.stage).into()),
        ),
        JobStatus::Paused
            if matches!(
                job.stage,
                JobStage::ScriptDirector
                    | JobStage::SemanticNarration
                    | JobStage::Tts
                    | JobStage::Export
            ) =>
        {
            (
                WorkflowRunState::WaitingForInput,
                Some(legacy_editor_node(job.stage).into()),
            )
        }
        _ => return None,
    };
    Some(intent_result(job.clone(), state, node))
}

fn legacy_editor_node(stage: JobStage) -> &'static str {
    match stage {
        JobStage::Export => production_workflow::EXPORT_NODE_ID,
        JobStage::Tts | JobStage::SemanticNarration => "alignment_validation",
        _ => "script_review",
    }
}

fn current_job(
    state: &State<'_, AppState>,
    app: &AppHandle,
    job_id: &str,
) -> Result<JobSummary, AppError> {
    let job = state
        .database
        .lock()
        .expect("database mutex poisoned")
        .get_job(job_id)?;
    let _ = app.emit("job://state", &job);
    Ok(job)
}

fn intent_result(
    job: JobSummary,
    workflow_state: WorkflowRunState,
    current_node_id: Option<String>,
) -> WorkflowIntentResult {
    let next_action = if workflow_state == WorkflowRunState::WaitingForInput {
        WorkflowNextAction::OpenEditor
    } else if workflow_state == WorkflowRunState::Retryable
        || workflow_state == WorkflowRunState::Failed
        || job.status == JobStatus::Failed
    {
        WorkflowNextAction::Retry
    } else if workflow_state == WorkflowRunState::Succeeded
        || workflow_state == WorkflowRunState::Cancelled
    {
        WorkflowNextAction::None
    } else {
        WorkflowNextAction::Continue
    };
    WorkflowIntentResult {
        job,
        workflow_state,
        current_node_id,
        next_action,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_job(stage: JobStage, status: JobStatus) -> JobSummary {
        JobSummary {
            id: "legacy-job".into(),
            project_id: "legacy-project".into(),
            stage,
            progress: 80,
            status,
            checkpoint: None,
            error_message: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn legacy_export_job_opens_the_editor_without_restarting_media() {
        let result =
            legacy_terminal_or_editor_intent(&legacy_job(JobStage::Export, JobStatus::Paused))
                .unwrap();
        assert_eq!(result.workflow_state, WorkflowRunState::WaitingForInput);
        assert_eq!(result.current_node_id.as_deref(), Some("export_publish"));
        assert_eq!(result.next_action, WorkflowNextAction::OpenEditor);
    }

    #[test]
    fn legacy_early_stage_can_be_adopted_by_the_runner() {
        assert!(
            legacy_terminal_or_editor_intent(&legacy_job(JobStage::Asr, JobStatus::Queued,))
                .is_none()
        );
    }
}
