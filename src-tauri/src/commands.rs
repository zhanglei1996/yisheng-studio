use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::{
    credentials,
    domain::{
        ArtifactKind, JobStatus, JobSummary, MediaArtifacts, MediaProbe, ProjectSummary,
        ProviderProfile, ProviderTestResult, RuntimeComponent, RuntimeStatus, SegmentChange,
        SegmentRecord,
    },
    error::AppError,
    AppState,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenAiCompatibleConfig {
    base_url: String,
    #[allow(dead_code)]
    model: String,
}

/// Milestone-one command contract. Persistent SQLite storage replaces this empty
/// response in milestone two; keeping the boundary now prevents UI shell access.
#[tauri::command]
pub fn project_list(state: State<'_, AppState>) -> Result<Vec<ProjectSummary>, AppError> {
    state
        .database
        .lock()
        .expect("database mutex poisoned")
        .list_projects()
}

#[tauri::command]
pub fn project_create(
    state: State<'_, AppState>,
    name: String,
) -> Result<ProjectSummary, AppError> {
    state
        .database
        .lock()
        .expect("database mutex poisoned")
        .create_project(&Uuid::new_v4().to_string(), &name)
}

#[tauri::command]
pub async fn media_probe(path: String) -> Result<MediaProbe, AppError> {
    tauri::async_runtime::spawn_blocking(move || crate::media::probe(&PathBuf::from(path)))
        .await
        .map_err(|error| AppError::Media(format!("媒体检查任务异常：{error}")))?
}

#[tauri::command]
pub fn project_create_from_media(
    state: State<'_, AppState>,
    probe: MediaProbe,
    workflow_mode: String,
    audio_mode: String,
    translation_provider_id: Option<String>,
) -> Result<ProjectSummary, AppError> {
    let id = Uuid::new_v4().to_string();
    let database = state.database.lock().expect("database mutex poisoned");
    database.create_project(&id, &probe.file_name)?;
    database.attach_media(&id, &probe)?;
    database.configure_project(
        &id,
        &workflow_mode,
        &audio_mode,
        translation_provider_id.as_deref(),
    )?;
    database.get_project(&id)
}

#[tauri::command]
pub async fn media_prepare(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    job_id: String,
) -> Result<MediaArtifacts, AppError> {
    let source_path = {
        let database = state.database.lock().expect("database mutex poisoned");
        let project = database.get_project(&project_id)?;
        let source = project
            .source_path
            .ok_or_else(|| AppError::Validation("项目尚未关联视频".into()))?;
        let current = database.get_job(&job_id)?;
        if matches!(
            current.status,
            JobStatus::Paused | JobStatus::Failed | JobStatus::WaitingUser
        ) {
            database.transition_job(&job_id, JobStatus::Queued)?;
        }
        database.start_job(&job_id)?;
        database.checkpoint_job(&job_id, "audio_extract", 3, "media:started")?;
        emit_job_state(&app, &database.get_job(&job_id)?);
        source
    };
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::Media(error.to_string()))?;
    let worker_project_id = project_id.clone();
    let worker_job_id = job_id.clone();
    let worker_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        crate::media::prepare(
            &worker_project_id,
            &PathBuf::from(source_path),
            &root,
            |stage, progress, checkpoint| {
                let state = worker_app.state::<AppState>();
                let database = state.database.lock().expect("database mutex poisoned");
                database.checkpoint_job(&worker_job_id, stage, progress, checkpoint)?;
                emit_job_state(&worker_app, &database.get_job(&worker_job_id)?);
                Ok(())
            },
        )
    })
    .await
    .map_err(|error| AppError::Media(format!("媒体准备任务异常：{error}")))?;

    match result {
        Ok(artifacts) => {
            let database = state.database.lock().expect("database mutex poisoned");
            database.set_artifact_dir(&project_id, &artifacts.artifact_dir)?;
            database.checkpoint_job(
                &job_id,
                "asr",
                15,
                &format!("media:{}", artifacts.artifact_dir),
            )?;
            let job = database.transition_job(&job_id, JobStatus::Paused)?;
            emit_job_state(&app, &job);
            Ok(artifacts)
        }
        Err(error) => {
            let database = state.database.lock().expect("database mutex poisoned");
            if let Ok(job) = database.fail_job(&job_id, &error.to_string()) {
                emit_job_state(&app, &job);
            }
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn preview_media(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<crate::domain::PreviewMedia, AppError> {
    let project = state
        .database
        .lock()
        .expect("database mutex poisoned")
        .get_project(&project_id)?;
    let artifact_dir = PathBuf::from(
        project
            .artifact_dir
            .ok_or_else(|| AppError::Media("请先完成媒体准备".into()))?,
    );
    tauri::async_runtime::spawn_blocking(move || {
        crate::media::resolve_preview(&artifact_dir, &project.audio_mode)
    })
    .await
    .map_err(|error| AppError::Media(error.to_string()))?
}

#[tauri::command]
pub fn job_enqueue(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
) -> Result<JobSummary, AppError> {
    let job = JobSummary {
        id: Uuid::new_v4().to_string(),
        project_id,
        stage: "media_check".into(),
        progress: 0,
        status: JobStatus::Queued,
        checkpoint: None,
        error_message: None,
        created_at: String::new(),
        updated_at: String::new(),
    };
    state
        .database
        .lock()
        .expect("database mutex poisoned")
        .enqueue_job(&job)?;
    let persisted = state
        .database
        .lock()
        .expect("database mutex poisoned")
        .get_job(&job.id)?;
    emit_job_state(&app, &persisted);
    Ok(persisted)
}

#[tauri::command]
pub fn job_list(state: State<'_, AppState>) -> Result<Vec<JobSummary>, AppError> {
    state
        .database
        .lock()
        .expect("database mutex poisoned")
        .list_jobs()
}

#[tauri::command]
pub fn job_start(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    let database = state.database.lock().expect("database mutex poisoned");
    database.start_job(&id)?;
    emit_job_state(&app, &database.get_job(&id)?);
    Ok(())
}

#[tauri::command]
pub fn job_pause(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<JobSummary, AppError> {
    let job = state
        .database
        .lock()
        .expect("database mutex poisoned")
        .transition_job(&id, JobStatus::Paused)?;
    emit_job_state(&app, &job);
    Ok(job)
}

#[tauri::command]
pub fn job_resume(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<JobSummary, AppError> {
    let database = state.database.lock().expect("database mutex poisoned");
    database.start_job(&id)?;
    let job = database.get_job(&id)?;
    emit_job_state(&app, &job);
    Ok(job)
}

#[tauri::command]
pub fn job_cancel(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<JobSummary, AppError> {
    let job = state
        .database
        .lock()
        .expect("database mutex poisoned")
        .transition_job(&id, JobStatus::Cancelled)?;
    emit_job_state(&app, &job);
    Ok(job)
}

#[tauri::command]
pub fn job_retry(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<JobSummary, AppError> {
    let database = state.database.lock().expect("database mutex poisoned");
    database.transition_job(&id, JobStatus::Queued)?;
    let job = database.get_job(&id)?;
    emit_job_state(&app, &job);
    Ok(job)
}

#[tauri::command]
pub fn job_checkpoint(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    stage: String,
    progress: u8,
    checkpoint: String,
) -> Result<(), AppError> {
    let database = state.database.lock().expect("database mutex poisoned");
    database.checkpoint_job(&id, &stage, progress, &checkpoint)?;
    emit_job_state(&app, &database.get_job(&id)?);
    Ok(())
}

fn emit_job_state(app: &AppHandle, job: &JobSummary) {
    let _ = app.emit("job://state", job);
}

#[tauri::command]
pub fn segment_upsert(state: State<'_, AppState>, segment: SegmentRecord) -> Result<(), AppError> {
    state
        .database
        .lock()
        .expect("database mutex poisoned")
        .upsert_segment(&segment)
}

#[tauri::command]
pub fn segment_list(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<SegmentRecord>, AppError> {
    let database = state.database.lock().expect("database mutex poisoned");
    let project = database.get_project(&project_id)?;
    let segments = database.list_segments(&project_id)?;
    if project.progress >= 80 {
        if let Some(directory) = project.artifact_dir {
            let warning_ids = crate::tts::audit_warnings(&segments, &PathBuf::from(directory));
            database.set_project_segments_status(&project_id, "warning", "ready")?;
            for id in warning_ids {
                database.set_segment_status(&id, "warning")?;
            }
            return database.list_segments(&project_id);
        }
    }
    Ok(segments)
}

#[tauri::command]
pub fn segment_replace_project(
    state: State<'_, AppState>,
    project_id: String,
    segments: Vec<SegmentRecord>,
) -> Result<(), AppError> {
    state
        .database
        .lock()
        .expect("database mutex poisoned")
        .replace_project_segments(&project_id, &segments)
}

#[tauri::command]
pub async fn asr_run(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    job_id: String,
) -> Result<Vec<SegmentRecord>, AppError> {
    let project = state
        .database
        .lock()
        .expect("database mutex poisoned")
        .get_project(&project_id)?;
    let artifact_dir = PathBuf::from(
        project
            .artifact_dir
            .ok_or_else(|| AppError::Media("请先完成媒体准备".into()))?,
    );
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Media(e.to_string()))?;
    let audio = artifact_dir.join("source-16k-mono.wav");
    let model = app_dir.join("models/ggml-small.en.bin");
    let runtimes = app_dir.join("runtimes");
    {
        let db = state.database.lock().expect("database mutex poisoned");
        let current = db.get_job(&job_id)?;
        if matches!(
            current.status,
            JobStatus::Paused | JobStatus::Failed | JobStatus::WaitingUser
        ) {
            db.transition_job(&job_id, JobStatus::Queued)?;
        }
        db.start_job(&job_id)?;
        db.checkpoint_job(&job_id, "asr", 16, "asr:started")?;
        emit_job_state(&app, &db.get_job(&job_id)?);
    }
    let worker_project = project_id.clone();
    let worker_dir = artifact_dir.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        crate::asr::transcribe(&worker_project, &audio, &model, &worker_dir, &runtimes)
    })
    .await
    .map_err(|e| AppError::Media(e.to_string()))?;
    match result {
        Ok(segments) => {
            let mut db = state.database.lock().expect("database mutex poisoned");
            db.replace_asr_segments(&project_id, &segments)?;
            db.checkpoint_job(
                &job_id,
                "glossary",
                35,
                &format!("asr:{}-segments", segments.len()),
            )?;
            let job = db.transition_job(&job_id, JobStatus::WaitingUser)?;
            emit_job_state(&app, &job);
            Ok(segments)
        }
        Err(error) => {
            let db = state.database.lock().expect("database mutex poisoned");
            if let Ok(job) = db.fail_job(&job_id, &error.to_string()) {
                emit_job_state(&app, &job)
            }
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn translation_run(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    job_id: String,
) -> Result<Vec<SegmentRecord>, AppError> {
    let (project, segments, profile) = {
        let database = state.database.lock().expect("database mutex poisoned");
        let project = database.get_project(&project_id)?;
        let provider_id = match project.translation_provider_id.clone() {
            Some(id) => id,
            None => {
                let fallback = database
                    .list_providers()?
                    .into_iter()
                    .find(|provider| provider.kind == "translation" || provider.kind == "翻译模型")
                    .ok_or_else(|| {
                        AppError::Provider("请先到“服务商”添加 DeepSeek 或阿里百炼".into())
                    })?;
                database.set_project_translation_provider(&project_id, &fallback.id)?;
                fallback.id
            }
        };
        let segments = database.list_segments(&project_id)?;
        let profile = database.get_provider(&provider_id)?;
        (project, segments, profile)
    };
    if segments.is_empty() {
        return Err(AppError::Validation("项目还没有识别片段".into()));
    }
    let reference = profile
        .credential_ref
        .ok_or_else(|| AppError::Provider("请先在“服务商”页面填写 API Key".into()))?;
    let secret = credentials::get(&reference)?;
    let config: crate::translation::ProviderConfig =
        serde_json::from_str(&profile.public_config_json)
            .map_err(|_| AppError::Provider("翻译服务配置无法解析".into()))?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|_| AppError::Provider("无法创建翻译连接".into()))?;
    {
        let database = state.database.lock().expect("database mutex poisoned");
        if matches!(
            database.get_job(&job_id)?.status,
            JobStatus::WaitingUser | JobStatus::Paused | JobStatus::Failed
        ) {
            database.transition_job(&job_id, JobStatus::Queued)?;
        }
        database.start_job(&job_id)?;
        database.checkpoint_job(&job_id, "translation", 36, "translation:started")?;
        emit_job_state(&app, &database.get_job(&job_id)?);
    }
    const BATCH_SIZE: usize = 8;
    let pending = segments
        .iter()
        .filter(|segment| {
            segment
                .subtitle_zh
                .trim_matches(['。', '，', '！', '？', ' '])
                .is_empty()
                || segment.spoken_zh.trim().is_empty()
        })
        .cloned()
        .collect::<Vec<_>>();
    if pending.is_empty() {
        let database = state.database.lock().expect("database mutex poisoned");
        database.checkpoint_job(&job_id, "tts", 56, "translation:complete")?;
        let job = database.transition_job(&job_id, JobStatus::Paused)?;
        emit_job_state(&app, &job);
        return database.list_segments(&project_id);
    }
    let total_batches = pending.len().div_ceil(BATCH_SIZE);
    for (index, batch) in pending.chunks(BATCH_SIZE).enumerate() {
        let values = match translate_with_format_retry(&client, &config, &secret, batch).await {
            Ok(values) => values,
            Err(error) => {
                let database = state.database.lock().expect("database mutex poisoned");
                if let Ok(job) = database.fail_job(&job_id, &error.to_string()) {
                    emit_job_state(&app, &job);
                }
                return Err(error);
            }
        };
        let database = state.database.lock().expect("database mutex poisoned");
        for (id, subtitle, spoken) in values {
            database.update_segment_translation(&id, &subtitle, &spoken)?;
        }
        let progress = 36 + (((index + 1) * 20 / total_batches.max(1)) as u8);
        database.checkpoint_job(
            &job_id,
            "translation",
            progress,
            &format!("translation:batch-{}/{}", index + 1, total_batches),
        )?;
        emit_job_state(&app, &database.get_job(&job_id)?);
    }
    let database = state.database.lock().expect("database mutex poisoned");
    database.checkpoint_job(&job_id, "tts", 56, "translation:complete")?;
    let target = if project.workflow_mode == "review" {
        JobStatus::WaitingUser
    } else {
        JobStatus::Paused
    };
    let job = database.transition_job(&job_id, target)?;
    emit_job_state(&app, &job);
    database.list_segments(&project_id)
}

#[tauri::command]
pub async fn translation_rebuild(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    job_id: String,
) -> Result<Vec<SegmentRecord>, AppError> {
    {
        let database = state.database.lock().expect("database mutex poisoned");
        let job = database.get_job(&job_id)?;
        if job.status == JobStatus::Succeeded {
            database.reopen_job(&job_id, "translation", 36, "translation:rebuild")?;
        } else if matches!(job.status, JobStatus::WaitingUser | JobStatus::Paused) {
            database.transition_job(&job_id, JobStatus::Queued)?;
        }
        database.clear_project_translations(&project_id)?;
    }
    translation_run(app, state, project_id, job_id).await
}

async fn translate_with_format_retry(
    client: &reqwest::Client,
    config: &crate::translation::ProviderConfig,
    secret: &str,
    batch: &[SegmentRecord],
) -> Result<Vec<(String, String, String)>, AppError> {
    match crate::translation::translate_batch(client, config, secret, batch).await {
        Ok(values) => Ok(values),
        Err(error) if crate::translation::is_retryable_format_error(&error) => {
            let mut recovered = Vec::with_capacity(batch.len());
            for segment in batch {
                let mut last_error = None;
                for _ in 0..2 {
                    match crate::translation::translate_batch(
                        client,
                        config,
                        secret,
                        std::slice::from_ref(segment),
                    )
                    .await
                    {
                        Ok(mut values) => {
                            recovered.append(&mut values);
                            last_error = None;
                            break;
                        }
                        Err(retry_error) => last_error = Some(retry_error),
                    }
                }
                if let Some(retry_error) = last_error {
                    match crate::translation::translate_one_fallback(
                        client, config, secret, segment,
                    )
                    .await
                    {
                        Ok(value) => recovered.push(value),
                        Err(_) => return Err(retry_error),
                    }
                }
            }
            Ok(recovered)
        }
        Err(error) => Err(error),
    }
}

#[tauri::command]
pub async fn tts_run(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    job_id: String,
) -> Result<Vec<String>, AppError> {
    let (project, segments) = {
        let database = state.database.lock().expect("database mutex poisoned");
        (
            database.get_project(&project_id)?,
            database.list_segments(&project_id)?,
        )
    };
    if segments
        .iter()
        .any(|segment| segment.spoken_zh.trim().is_empty())
    {
        return Err(AppError::Validation(
            "仍有片段没有中文配音文案，请先完成翻译".into(),
        ));
    }
    let audio_mode = project.audio_mode.clone();
    let artifact_dir = PathBuf::from(
        project
            .artifact_dir
            .ok_or_else(|| AppError::Media("请先完成媒体准备".into()))?,
    );
    {
        let database = state.database.lock().expect("database mutex poisoned");
        let current = database.get_job(&job_id)?;
        if matches!(
            current.status,
            JobStatus::Paused | JobStatus::Failed | JobStatus::WaitingUser
        ) {
            database.transition_job(&job_id, JobStatus::Queued)?;
        }
        database.start_job(&job_id)?;
        database.checkpoint_job(&job_id, "tts", 57, "tts:started")?;
        emit_job_state(&app, &database.get_job(&job_id)?);
    }
    let duration_ms = project
        .duration_ms
        .unwrap_or_else(|| segments.last().map_or(0, |segment| segment.end_ms));
    let progress_app = app.clone();
    let progress_job_id = job_id.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let output = crate::tts::synthesize(&segments, &artifact_dir, duration_ms, |done, total| {
            if done == total || done % 4 == 0 {
                let managed_state = progress_app.state::<AppState>();
                let database = managed_state
                    .database
                    .lock()
                    .expect("database mutex poisoned");
                let percent = 57 + (done * 22 / total.max(1)) as u8;
                if database
                    .checkpoint_job(
                        &progress_job_id,
                        "tts",
                        percent,
                        &format!("tts:segment-{done}/{total}"),
                    )
                    .is_ok()
                {
                    if let Ok(job) = database.get_job(&progress_job_id) {
                        emit_job_state(&progress_app, &job);
                    }
                }
            }
        })?;
        crate::media::render_dubbed_preview(&artifact_dir, &audio_mode)?;
        Ok::<_, AppError>(output)
    })
    .await
    .map_err(|error| AppError::Media(error.to_string()))?;
    match result {
        Ok(output) => {
            let database = state.database.lock().expect("database mutex poisoned");
            database.set_project_segments_status(&project_id, "warning", "ready")?;
            database.set_project_segments_status(&project_id, "stale", "ready")?;
            database.set_project_segments_status(&project_id, "processing", "ready")?;
            for id in &output.warning_ids {
                database.set_segment_status(id, "warning")?;
            }
            database.checkpoint_job(
                &job_id,
                "export",
                80,
                &format!("tts:{}", output.track_path.display()),
            )?;
            let status = if output.warning_ids.is_empty() {
                JobStatus::Paused
            } else {
                JobStatus::WaitingUser
            };
            let job = database.transition_job(&job_id, status)?;
            emit_job_state(&app, &job);
            Ok(output.warning_ids)
        }
        Err(error) => {
            let database = state.database.lock().expect("database mutex poisoned");
            if let Ok(job) = database.fail_job(&job_id, &error.to_string()) {
                emit_job_state(&app, &job)
            }
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn tts_fit_warnings(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    job_id: String,
) -> Result<Vec<String>, AppError> {
    let (project, mut warnings, profile) = {
        let database = state.database.lock().expect("database mutex poisoned");
        let project = database.get_project(&project_id)?;
        let warnings = database
            .list_segments(&project_id)?
            .into_iter()
            .filter(|segment| segment.status == "warning")
            .collect::<Vec<_>>();
        let provider_id = project
            .translation_provider_id
            .clone()
            .ok_or_else(|| AppError::Provider("项目没有翻译服务配置".into()))?;
        let profile = database.get_provider(&provider_id)?;
        (project, warnings, profile)
    };
    if warnings.is_empty() {
        return Ok(Vec::new());
    }
    let reference = profile
        .credential_ref
        .ok_or_else(|| AppError::Provider("请先在“服务商”页面填写 API Key".into()))?;
    let secret = credentials::get(&reference)?;
    let config: crate::translation::ProviderConfig =
        serde_json::from_str(&profile.public_config_json)
            .map_err(|_| AppError::Provider("翻译服务配置无法解析".into()))?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|_| AppError::Provider("无法创建配音文案压缩连接".into()))?;
    {
        let database = state.database.lock().expect("database mutex poisoned");
        let current = database.get_job(&job_id)?;
        if current.status == JobStatus::Succeeded {
            database.reopen_job(&job_id, "tts", 57, "tts:fit-started")?;
        } else if matches!(
            current.status,
            JobStatus::Paused | JobStatus::Failed | JobStatus::WaitingUser
        ) {
            database.transition_job(&job_id, JobStatus::Queued)?;
        }
        database.start_job(&job_id)?;
        database.checkpoint_job(&job_id, "tts", 57, "tts:fit-started")?;
        emit_job_state(&app, &database.get_job(&job_id)?);
    }
    for attempt in 0..2 {
        if warnings.is_empty() {
            break;
        }
        let warning_count = warnings.len();
        for (index, segment) in warnings.iter().enumerate() {
            let original_chars = segment.spoken_zh.chars().count();
            let factor = if attempt == 0 { 0.76 } else { 0.62 };
            // At the default macOS voice speed, a conservative Mandarin budget is
            // roughly one spoken character per 220–270 ms. Keep tightening on the
            // second pass so repeated fitting also helps very short source windows.
            let duration_ms = (segment.end_ms - segment.start_ms).max(300) as usize;
            let duration_budget = duration_ms / if attempt == 0 { 220 } else { 270 };
            let target_chars = ((original_chars as f64 * factor).round() as usize)
                .min(duration_budget)
                .max(2);
            let compressed = crate::translation::compress_spoken(
                &client,
                &config,
                &secret,
                segment,
                target_chars,
            )
            .await?;
            let database = state.database.lock().expect("database mutex poisoned");
            database.update_segment_spoken(&segment.id, &compressed)?;
            let done = attempt * warning_count + index + 1;
            let total = warning_count * 2;
            let progress = 57 + (done * 16 / total.max(1)) as u8;
            database.checkpoint_job(
                &job_id,
                "tts",
                progress,
                &format!("tts:compress-{}/{total}", done),
            )?;
            emit_job_state(&app, &database.get_job(&job_id)?);
        }
        let refreshed = {
            let database = state.database.lock().expect("database mutex poisoned");
            database.list_segments(&project_id)?
        };
        let artifact_dir = PathBuf::from(
            project
                .artifact_dir
                .as_ref()
                .ok_or_else(|| AppError::Media("项目产物目录丢失".into()))?,
        );
        let ids = warnings
            .iter()
            .map(|segment| segment.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        let candidates = refreshed
            .into_iter()
            .filter(|segment| ids.contains(segment.id.as_str()))
            .collect::<Vec<_>>();
        let duration_ms = project
            .duration_ms
            .unwrap_or_else(|| candidates.last().map_or(0, |segment| segment.end_ms));
        let artifact_for_worker = artifact_dir.clone();
        let output = tauri::async_runtime::spawn_blocking(move || {
            crate::tts::synthesize(&candidates, &artifact_for_worker, duration_ms, |_, _| {})
        })
        .await
        .map_err(|error| AppError::Media(error.to_string()))??;
        let remaining = output
            .warning_ids
            .iter()
            .map(String::as_str)
            .collect::<std::collections::HashSet<_>>();
        let database = state.database.lock().expect("database mutex poisoned");
        for segment in &warnings {
            database.set_segment_status(
                &segment.id,
                if remaining.contains(segment.id.as_str()) {
                    "warning"
                } else {
                    "ready"
                },
            )?;
        }
        warnings = database
            .list_segments(&project_id)?
            .into_iter()
            .filter(|segment| segment.status == "warning")
            .collect();
    }
    {
        let database = state.database.lock().expect("database mutex poisoned");
        database.transition_job(&job_id, JobStatus::Paused)?;
    }
    tts_run(app, state, project_id, job_id).await
}

#[tauri::command]
pub async fn export_start(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    job_id: String,
    output_directory: String,
    subtitle_mode: String,
) -> Result<crate::exporter::ExportOutput, AppError> {
    if !matches!(subtitle_mode.as_str(), "none" | "chinese" | "bilingual") {
        return Err(AppError::Validation("未知的字幕导出模式".into()));
    }
    let (project, segments) = {
        let database = state.database.lock().expect("database mutex poisoned");
        (
            database.get_project(&project_id)?,
            database.list_segments(&project_id)?,
        )
    };
    {
        let database = state.database.lock().expect("database mutex poisoned");
        let current = database.get_job(&job_id)?;
        if matches!(
            current.status,
            JobStatus::Paused | JobStatus::Failed | JobStatus::WaitingUser
        ) {
            database.transition_job(&job_id, JobStatus::Queued)?;
        }
        database.start_job(&job_id)?;
        database.checkpoint_job(&job_id, "export", 82, "export:started")?;
        emit_job_state(&app, &database.get_job(&job_id)?);
    }
    let output = PathBuf::from(output_directory);
    let result = tauri::async_runtime::spawn_blocking(move || {
        crate::exporter::export(&project, &segments, &output, &subtitle_mode)
    })
    .await
    .map_err(|e| AppError::Media(e.to_string()))?;
    match result {
        Ok(value) => {
            let database = state.database.lock().expect("database mutex poisoned");
            database.checkpoint_job(
                &job_id,
                "export",
                100,
                &format!("export:{}", value.directory),
            )?;
            let job = database.transition_job(&job_id, JobStatus::Succeeded)?;
            emit_job_state(&app, &job);
            Ok(value)
        }
        Err(error) => {
            let database = state.database.lock().expect("database mutex poisoned");
            if let Ok(job) = database.fail_job(&job_id, &error.to_string()) {
                emit_job_state(&app, &job)
            }
            Err(error)
        }
    }
}

#[tauri::command]
pub fn path_reveal(path: String) -> Result<(), AppError> {
    let requested = PathBuf::from(path);
    if !requested.exists() {
        return Err(AppError::NotFound("导出目录不存在".into()));
    }
    let target = requested
        .canonicalize()
        .map_err(|error| AppError::Media(error.to_string()))?;
    let status = std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(&target)
        .status()
        .map_err(|error| AppError::Media(format!("无法打开 Finder：{error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(AppError::Media("Finder 无法定位导出结果".into()))
    }
}

#[tauri::command]
pub fn segment_invalidation(change: SegmentChange) -> Vec<ArtifactKind> {
    change.invalidates().to_vec()
}

#[tauri::command]
pub fn credential_save(provider_id: String, secret: String) -> Result<String, AppError> {
    credentials::save(&provider_id, &secret)
}

#[tauri::command]
pub fn credential_delete(credential_ref: String) -> Result<(), AppError> {
    credentials::delete(&credential_ref)
}

#[tauri::command]
pub fn provider_list(state: State<'_, AppState>) -> Result<Vec<ProviderProfile>, AppError> {
    state
        .database
        .lock()
        .expect("database mutex poisoned")
        .list_providers()
}

#[tauri::command]
pub fn provider_save(
    state: State<'_, AppState>,
    id: String,
    kind: String,
    name: String,
    public_config_json: String,
    secret: Option<String>,
) -> Result<ProviderProfile, AppError> {
    serde_json::from_str::<serde_json::Value>(&public_config_json)
        .map_err(|error| AppError::Validation(format!("invalid provider config: {error}")))?;
    let existing_credential = state
        .database
        .lock()
        .expect("database mutex poisoned")
        .get_provider(&id)
        .ok()
        .and_then(|profile| profile.credential_ref);
    let credential_ref = match secret {
        Some(value) if !value.trim().is_empty() => Some(credentials::save(&id, &value)?),
        _ => existing_credential,
    };
    let profile = ProviderProfile {
        id,
        kind,
        name,
        public_config_json,
        credential_ref,
    };
    state
        .database
        .lock()
        .expect("database mutex poisoned")
        .save_provider(&profile)?;
    Ok(profile)
}

#[tauri::command]
pub async fn provider_test(
    state: State<'_, AppState>,
    id: String,
) -> Result<ProviderTestResult, AppError> {
    let profile = state
        .database
        .lock()
        .expect("database mutex poisoned")
        .get_provider(&id)?;
    if profile.id == "system" || profile.kind == "system_tts" {
        return Ok(ProviderTestResult {
            ok: true,
            latency_ms: 0,
            message: "macOS 系统语音可用".into(),
            available_models: 1,
        });
    }
    let reference = profile
        .credential_ref
        .ok_or_else(|| AppError::Provider("请先保存 API Key".into()))?;
    let secret = credentials::get(&reference)?;
    let config: OpenAiCompatibleConfig = serde_json::from_str(&profile.public_config_json)
        .map_err(|_| AppError::Provider("服务商配置无法解析".into()))?;
    let base = reqwest::Url::parse(config.base_url.trim_end_matches('/'))
        .map_err(|_| AppError::Provider("Base URL 格式不正确".into()))?;
    let is_local = matches!(base.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
    if base.scheme() != "https" && !is_local {
        return Err(AppError::Provider("远程服务必须使用 HTTPS".into()));
    }
    let url = format!("{}/models", base.as_str().trim_end_matches('/'));
    let started = Instant::now();
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| AppError::Provider("无法创建安全连接".into()))?
        .get(url)
        .bearer_auth(secret)
        .header("User-Agent", "YishengStudio/0.1")
        .send()
        .await
        .map_err(|error| {
            AppError::Provider(if error.is_timeout() {
                "连接超时，请检查网络和 Base URL".into()
            } else {
                "无法连接服务商".into()
            })
        })?;
    let latency_ms = started.elapsed().as_millis();
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(AppError::Provider("API Key 无效或没有访问权限".into()));
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(AppError::Provider("服务商正在限流，请稍后重试".into()));
    }
    if !status.is_success() {
        return Err(AppError::Provider(format!(
            "服务商返回 HTTP {}",
            status.as_u16()
        )));
    }
    let payload = response
        .json::<serde_json::Value>()
        .await
        .map_err(|_| AppError::Provider("服务商响应格式不正确".into()))?;
    let available_models = payload
        .get("data")
        .and_then(|value| value.as_array())
        .map_or(0, Vec::len);
    Ok(ProviderTestResult {
        ok: true,
        latency_ms,
        message: "连接成功，凭据有效".into(),
        available_models,
    })
}

#[tauri::command]
pub fn provider_delete(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    if let Some(reference) = state
        .database
        .lock()
        .expect("database mutex poisoned")
        .remove_provider(&id)?
    {
        credentials::delete(&reference)?;
    }
    Ok(())
}

#[tauri::command]
pub fn runtime_catalog(app: AppHandle) -> Vec<RuntimeComponent> {
    let architecture = std::env::consts::ARCH.to_string();
    let app_data = app.path().app_data_dir().ok();
    let whisper_installed = app_data.as_ref().is_some_and(|root| {
        root.join("runtimes/whisper-cpp-v1.9.2/whisper-cli")
            .is_file()
    });
    let model_installed = app_data.as_ref().is_some_and(|root| {
        root.join("models/ggml-small.en.bin")
            .metadata()
            .is_ok_and(|value| value.len() > 400 * 1024 * 1024)
    });
    let ffmpeg_installed = [
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/usr/bin/ffmpeg",
    ]
    .iter()
    .any(|path| std::path::Path::new(path).is_file());
    vec![
        RuntimeComponent {
            id: "system-tts".into(),
            name: "macOS 系统语音".into(),
            architecture: architecture.clone(),
            version: "system".into(),
            installed: cfg!(target_os = "macos"),
            sha256: None,
            license: "Apple system component".into(),
            size_bytes: None,
            status: RuntimeStatus::Installed,
        },
        RuntimeComponent {
            id: "ffmpeg".into(),
            name: "FFmpeg 媒体组件".into(),
            architecture: architecture.clone(),
            version: "system/dev".into(),
            installed: ffmpeg_installed,
            sha256: Some("manifest-pending".into()),
            license: "LGPL-2.1-or-later".into(),
            size_bytes: Some(92 * 1024 * 1024),
            status: if ffmpeg_installed {
                RuntimeStatus::Installed
            } else {
                RuntimeStatus::Available
            },
        },
        RuntimeComponent {
            id: "whisper-cpp".into(),
            name: "whisper.cpp".into(),
            architecture: architecture.clone(),
            version: "1.9.2".into(),
            installed: whisper_installed,
            sha256: Some("manifest-pending".into()),
            license: "MIT".into(),
            size_bytes: Some(18 * 1024 * 1024),
            status: if whisper_installed {
                RuntimeStatus::Installed
            } else {
                RuntimeStatus::Available
            },
        },
        RuntimeComponent {
            id: "whisper-small-en".into(),
            name: "Whisper small.en".into(),
            architecture,
            version: "ggml-small.en".into(),
            installed: model_installed,
            sha256: Some("manifest-pending".into()),
            license: "MIT model distribution notice required".into(),
            size_bytes: Some(466 * 1024 * 1024),
            status: if model_installed {
                RuntimeStatus::Installed
            } else {
                RuntimeStatus::Available
            },
        },
    ]
}

#[tauri::command]
pub fn diagnostics_create() -> Result<String, String> {
    Ok("Diagnostics contract ready; content and credentials are excluded.".into())
}
