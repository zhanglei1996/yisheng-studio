use tauri::State;

use crate::{
    commands::export_preflight_for_segments,
    domain::{JobStatus, ProjectReadiness, ProjectSummary},
    error::AppError,
    AppState,
};

#[tauri::command]
pub fn project_readiness(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<ProjectReadiness, AppError> {
    let database = state.database.lock().expect("database mutex poisoned");
    let project = database.get_project(&project_id)?;
    let segments = database.list_segments(&project_id)?;
    let preflight = export_preflight_for_segments(&segments);
    let latest_job = database
        .list_jobs()?
        .into_iter()
        .find(|job| job.project_id == project_id);
    let running = latest_job
        .as_ref()
        .is_some_and(|job| job.status == JobStatus::Running);
    let failed = segments.iter().any(|segment| segment.tts_state == "failed")
        || latest_job
            .as_ref()
            .is_some_and(|job| job.status == JobStatus::Failed);
    let (phase, next_action) = if running {
        ("processing", "等待当前处理完成")
    } else if failed {
        ("failed", "重试失败片段")
    } else if preflight.blocking_count > 0 {
        ("review", "完成尚未生成的配音")
    } else if preflight.warning_count > 0 {
        ("export_warning", "自动修复时长问题或知情导出")
    } else if segments.is_empty() {
        ("processing", "等待识别与翻译完成")
    } else {
        ("ready", "导出中文版本")
    };
    Ok(ProjectReadiness {
        phase: phase.into(),
        blocking_count: preflight.blocking_count,
        warning_count: preflight.warning_count,
        can_export: preflight.can_export,
        next_action: next_action.into(),
        progress: latest_job.map_or(project.progress, |job| job.progress),
    })
}

#[tauri::command]
pub fn project_audio_mode_update(
    state: State<'_, AppState>,
    project_id: String,
    audio_mode: String,
) -> Result<ProjectSummary, AppError> {
    if !matches!(audio_mode.as_str(), "duck" | "mute" | "separate") {
        return Err(AppError::Validation("未知的原声处理模式".into()));
    }
    let database = state.database.lock().expect("database mutex poisoned");
    let project = database.get_project(&project_id)?;
    database.configure_project(
        &project_id,
        &project.workflow_mode,
        &audio_mode,
        project.translation_provider_id.as_deref(),
    )?;
    database.get_project(&project_id)
}
