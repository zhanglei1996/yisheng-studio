use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::{
    credentials,
    db::{TtsPublishSnapshot, TtsSegmentPublication},
    director::{self, ProtectedTerm},
    domain::{
        ArtifactKind, ArtifactRecord, ExportPreflight, GlossaryTerm, JobStage, JobStatus,
        JobSummary, LocalizationAnalysis, MediaArtifacts, MediaProbe, PreviewMedia, ProjectSummary,
        ProviderProfile, ProviderTestResult, PublishCheck, RuntimeComponent, RuntimeStatus,
        SegmentChange, SegmentRecord, TtsCatalog, TtsFitProgress, TtsFitResult, TtsPreviewAudio,
        TtsRunResult, TtsSegmentFailure, TtsStyleDescriptor, TtsVoiceDescriptor,
    },
    error::AppError,
    script::{ProtectedKind, ScriptDocumentV1},
    tts_provider::{
        AliyunTtsAdapter, AliyunTtsConfig, AudioEncoding, IflytekSuperTtsAdapter,
        IflytekSuperTtsConfig, SynthesisRequest, SystemTtsAdapter, TtsProviderAdapter,
        TtsSecretBundle,
    },
    workflow::WorkflowStore,
    AppState,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenAiCompatibleConfig {
    base_url: String,
    #[allow(dead_code)]
    model: String,
}

fn normalize_translation_config(
    value: serde_json::Value,
) -> Result<crate::translation::ProviderConfig, AppError> {
    let base_url = value
        .get("baseUrl")
        .or_else(|| value.get("base_url"))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| AppError::Provider("翻译服务缺少 baseUrl".into()))?;
    let model = value
        .get("model")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| AppError::Provider("翻译服务缺少 model".into()))?;
    Ok(crate::translation::ProviderConfig {
        base_url: base_url.to_string(),
        model: model.to_string(),
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentScriptUpdateRequest {
    pub segment_id: String,
    pub expected_revision: u64,
    pub document: ScriptDocumentV1,
    #[serde(default = "empty_json_object")]
    pub tts_overrides_json: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsAuditionRequest {
    pub segment_id: String,
    pub script_revision: u64,
    pub document: ScriptDocumentV1,
    pub provider_id: Option<String>,
    pub voice_id: Option<String>,
    pub style: Option<String>,
    pub speed: Option<f32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectorPlanRequest {
    pub segment_id: String,
    pub style: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TtsOverrides {
    voice_id: Option<String>,
    style: Option<String>,
    speed: Option<f32>,
    director_enabled: Option<bool>,
}

struct TtsPipelineOutcome {
    warning_ids: Vec<String>,
    failed_segments: Vec<TtsSegmentFailure>,
    affected_segment_ids: Vec<String>,
    synthesis_unit_count: usize,
    cache_hit_unit_count: usize,
}

fn non_retryable_tts_error(error: &AppError) -> Option<AppError> {
    let message = match error {
        AppError::Provider(message) => message,
        _ => return None,
    };
    if message.contains("错误码：10163")
        || message.contains("must be one of")
        || message.contains("错误码：11200")
        || message.contains("LiccCheck failed")
    {
        return Some(AppError::Provider(format!(
            "当前讯飞音色未获得此账号授权或可用额度。请到“服务商”填写控制台显示的已授权 VCN，并先执行连接测试；本次整片合成已停止，未继续重复请求。原始信息：{message}"
        )));
    }
    None
}

fn empty_json_object() -> String {
    "{}".into()
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
pub async fn project_thumbnail(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Option<String>, AppError> {
    let project = state
        .database
        .lock()
        .expect("database mutex poisoned")
        .get_project(&project_id)?;
    let Some(source_path) = project.source_path else {
        return Ok(None);
    };
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::Media(error.to_string()))?;
    let thumbnail_lock = project_preview_lock(&state, &project_id);
    let path = tauri::async_runtime::spawn_blocking(move || {
        let _guard = thumbnail_lock
            .lock()
            .expect("project thumbnail mutex poisoned");
        crate::media::ensure_thumbnail(&project_id, &PathBuf::from(source_path), &root)
    })
    .await
    .map_err(|error| AppError::Media(format!("首帧封面生成任务异常：{error}")))??;
    Ok(Some(path.to_string_lossy().into_owned()))
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
pub fn project_rename(
    state: State<'_, AppState>,
    project_id: String,
    name: String,
) -> Result<ProjectSummary, AppError> {
    state
        .database
        .lock()
        .expect("database mutex poisoned")
        .rename_project(&project_id, &name)
}

#[tauri::command]
pub async fn project_delete(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
) -> Result<(), AppError> {
    let project_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::Media(error.to_string()))?
        .join("projects")
        .join(&project_id);
    {
        let database = state.database.lock().expect("database mutex poisoned");
        let _ = database.get_project(&project_id)?;
        if database
            .list_jobs()?
            .iter()
            .any(|job| job.project_id == project_id && job.status == JobStatus::Running)
        {
            return Err(AppError::Validation(
                "项目仍有运行中的任务，请先取消任务再删除项目".into(),
            ));
        }
        database.delete_project(&project_id)?;
    }
    if project_dir.exists() {
        tauri::async_runtime::spawn_blocking(move || std::fs::remove_dir_all(project_dir))
            .await
            .map_err(|error| AppError::Media(format!("删除项目文件任务异常：{error}")))?
            .map_err(|error| AppError::Media(format!("无法删除项目生成文件：{error}")))?;
    }
    state
        .preview_locks
        .lock()
        .expect("preview lock map poisoned")
        .remove(&project_id);
    state
        .tts_fit_snapshots
        .lock()
        .expect("TTS fit snapshot map poisoned")
        .remove(&project_id);
    Ok(())
}

#[tauri::command]
pub async fn media_probe(path: String) -> Result<MediaProbe, AppError> {
    tauri::async_runtime::spawn_blocking(move || crate::media::probe(&PathBuf::from(path)))
        .await
        .map_err(|error| AppError::Media(format!("媒体检查任务异常：{error}")))?
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn project_create_from_media(
    state: State<'_, AppState>,
    probe: MediaProbe,
    workflow_mode: String,
    audio_mode: String,
    translation_provider_id: Option<String>,
    tts_provider_id: Option<String>,
    tts_voice_id: Option<String>,
    project_name: Option<String>,
) -> Result<ProjectSummary, AppError> {
    let id = Uuid::new_v4().to_string();
    let database = state.database.lock().expect("database mutex poisoned");
    database.create_project(&id, &probe.file_name)?;
    database.attach_media(&id, &probe)?;
    if let Some(name) = project_name {
        database.rename_project(&id, &name)?;
    }
    database.configure_project(
        &id,
        &workflow_mode,
        &audio_mode,
        translation_provider_id.as_deref(),
    )?;
    let tts_provider_id = tts_provider_id.unwrap_or_else(|| "system".into());
    if tts_provider_id != "system" {
        let provider = database.get_provider(&tts_provider_id)?;
        if provider.kind != "cloud_tts"
            || provider
                .secret_bundle_ref
                .as_ref()
                .or(provider.credential_ref.as_ref())
                .is_none()
        {
            return Err(AppError::Provider(
                "所选高级语音服务尚未完成凭据配置".into(),
            ));
        }
    }
    database.set_project_tts_defaults(
        &id,
        &tts_provider_id,
        tts_voice_id.as_deref(),
        "auto",
        "{}",
        true,
        if tts_provider_id == "system" {
            "strict"
        } else {
            "balanced"
        },
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
    let (source_path, audio_mode, source_fingerprint) = {
        let database = state.database.lock().expect("database mutex poisoned");
        let project = database.get_project(&project_id)?;
        let source = project
            .source_path
            .ok_or_else(|| AppError::Validation("项目尚未关联视频".into()))?;
        let source_fingerprint = project
            .source_fingerprint
            .ok_or_else(|| AppError::Validation("项目源视频指纹缺失，请重新导入".into()))?;
        let current = database.get_job(&job_id)?;
        if matches!(
            current.status,
            JobStatus::Paused | JobStatus::Failed | JobStatus::WaitingUser
        ) {
            database.transition_job(&job_id, JobStatus::Queued)?;
        } else if current.status == JobStatus::Succeeded {
            database.requeue_completed_job(&job_id)?;
        }
        database.start_job(&job_id)?;
        database.checkpoint_job(&job_id, JobStage::AudioExtract, 3, "media:started")?;
        emit_job_state(&app, &database.get_job(&job_id)?);
        (source, project.audio_mode, source_fingerprint)
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
            &audio_mode,
            &source_fingerprint,
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
                JobStage::Asr,
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
            .clone()
            .ok_or_else(|| AppError::Media("请先完成媒体准备".into()))?,
    );
    tauri::async_runtime::spawn_blocking(move || {
        crate::media::resolve_preview(&artifact_dir, &project.audio_mode)
    })
    .await
    .map_err(|error| AppError::Media(error.to_string()))?
}

#[tauri::command]
pub async fn preview_prepare(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<crate::domain::PreviewMedia, AppError> {
    let preview_lock = project_preview_lock(&state, &project_id);
    let project = state
        .database
        .lock()
        .expect("database mutex poisoned")
        .get_project(&project_id)?;
    let artifact_dir = PathBuf::from(
        project
            .artifact_dir
            .clone()
            .ok_or_else(|| AppError::Media("请先完成媒体准备".into()))?,
    );
    let timeline_edits = state
        .database
        .lock()
        .expect("database mutex poisoned")
        .list_timeline_edits(&project_id)?;
    let source_duration_ms = project.duration_ms.unwrap_or(0);
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = preview_lock.lock().expect("project preview mutex poisoned");
        crate::media::prepare_timeline_preview(
            &artifact_dir,
            &project.audio_mode,
            source_duration_ms,
            &timeline_edits,
        )
    })
    .await
    .map_err(|error| AppError::Media(error.to_string()))?
}

fn project_preview_lock(
    state: &State<'_, AppState>,
    project_id: &str,
) -> std::sync::Arc<std::sync::Mutex<()>> {
    state
        .preview_locks
        .lock()
        .expect("preview lock registry mutex poisoned")
        .entry(project_id.to_string())
        .or_insert_with(|| std::sync::Arc::new(std::sync::Mutex::new(())))
        .clone()
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
        stage: JobStage::MediaCheck,
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
pub fn job_delete(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    state
        .database
        .lock()
        .expect("database mutex poisoned")
        .delete_job(&id)
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

fn emit_job_state(app: &AppHandle, job: &JobSummary) {
    let _ = app.emit("job://state", job);
}

fn emit_tts_fit_progress(app: &AppHandle, progress: TtsFitProgress) {
    let _ = app.emit("tts-fit://progress", progress);
}

pub(crate) fn export_preflight_for_segments(segments: &[SegmentRecord]) -> ExportPreflight {
    let blocking_segments = segments
        .iter()
        .filter(|segment| !crate::localization::is_non_speech_text(&segment.source_text))
        .filter(|segment| segment.tts_state != "ready" && segment.status != "warning")
        .collect::<Vec<_>>();
    let blocking_segment_ids = blocking_segments
        .iter()
        .map(|segment| segment.id.clone())
        .collect::<Vec<_>>();
    let warning_segment_ids = segments
        .iter()
        .filter(|segment| !crate::localization::is_non_speech_text(&segment.source_text))
        .filter(|segment| segment.status == "warning")
        .map(|segment| segment.id.clone())
        .collect::<Vec<_>>();
    let mut checks = publish_checks_for_segments(segments);
    for segment in blocking_segments {
        checks.push(PublishCheck {
            code: "tts_not_ready".into(),
            severity: "blocking".into(),
            source_range: Some([segment.start_ms, segment.end_ms]),
            output_range: None,
            scene_id: Some(segment.id.clone()),
            message: format!("第 {} 段尚未成功生成中文配音", segment.ordinal + 1),
            suggested_action: Some("定位到这一段并重新生成所在语音块".into()),
        });
    }
    let blocking_check_count = checks
        .iter()
        .filter(|check| check.severity == "blocking")
        .count();
    let warning_check_count = checks
        .iter()
        .filter(|check| check.severity == "warning")
        .count();
    let can_export = blocking_segment_ids.is_empty() && blocking_check_count == 0;
    let message = if !can_export {
        format!("有 {blocking_check_count} 个发布问题，处理后才能导出")
    } else if !warning_segment_ids.is_empty() {
        format!(
            "可导出，但仍有 {} 个片段的配音可能超出原视频时间窗",
            warning_segment_ids.len()
        )
    } else {
        "项目已通过配音与时长检查，可以导出".into()
    };
    ExportPreflight {
        can_export,
        blocking_count: blocking_check_count,
        warning_count: warning_check_count + warning_segment_ids.len(),
        blocking_segment_ids,
        warning_segment_ids,
        checks,
        message,
    }
}

fn apply_safe_background_preflight(project: &ProjectSummary, preflight: &mut ExportPreflight) {
    if project.audio_mode != "separate" {
        return;
    }
    let ready = project
        .artifact_dir
        .as_deref()
        .zip(project.source_fingerprint.as_deref())
        .is_some_and(|(artifact_dir, fingerprint)| {
            crate::media::safe_background_is_ready(
                PathBuf::from(artifact_dir).as_path(),
                fingerprint,
            )
        });
    if ready {
        return;
    }
    preflight.checks.push(PublishCheck {
        code: "safe_background_not_ready".into(),
        severity: "blocking".into(),
        source_range: None,
        output_range: None,
        scene_id: None,
        message: "安全背景轨缺失或已过期；为避免残留英文，不能导出".into(),
        suggested_action: Some("重新运行媒体准备，完成本地人声分离".into()),
    });
    preflight.blocking_count += 1;
    preflight.can_export = false;
    preflight.message = "安全模式尚未生成有效的背景与音效轨，处理后才能导出".into();
}

fn publish_checks_for_segments(segments: &[SegmentRecord]) -> Vec<PublishCheck> {
    let mut checks = Vec::new();
    for segment in segments {
        let duration = segment.end_ms - segment.start_ms;
        if crate::localization::is_non_speech_text(&segment.source_text)
            && !segment.spoken_zh.trim().is_empty()
        {
            checks.push(PublishCheck {
                code: "non_speech_spoken".into(),
                severity: "info".into(),
                source_range: Some([segment.start_ms, segment.end_ms]),
                output_range: None,
                scene_id: None,
                message: format!(
                    "“{}”已识别为非语言事件，不会送入 TTS；对应声音按所选原声模式处理",
                    segment.source_text.trim()
                ),
                suggested_action: Some("如识别有误，可在片段文本中改回口播内容".into()),
            });
        }
        if duration < 800 && !segment.subtitle_zh.trim().is_empty() {
            checks.push(PublishCheck {
                code: "caption_flash".into(),
                severity: "warning".into(),
                source_range: Some([segment.start_ms, segment.end_ms]),
                output_range: None,
                scene_id: None,
                message: format!("中文字幕仅显示 {duration}ms，可能闪烁"),
                suggested_action: Some("与相邻字幕合并或延长显示时间".into()),
            });
        }
        let readable_units = segment
            .subtitle_zh
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .count();
        if duration > 0 && readable_units as i64 * 1_000 > duration * 10 {
            checks.push(PublishCheck {
                code: "caption_reading_speed".into(),
                severity: "warning".into(),
                source_range: Some([segment.start_ms, segment.end_ms]),
                output_range: None,
                scene_id: None,
                message: "中文字幕阅读速度超过每秒 10 个字符".into(),
                suggested_action: Some("精简字幕或延长画面".into()),
            });
        }
    }
    for pair in segments.windows(2) {
        let gap_ms = pair[1].start_ms - pair[0].end_ms;
        if gap_ms > 4_000
            && !crate::localization::is_non_speech_text(&pair[0].source_text)
            && !crate::localization::is_non_speech_text(&pair[1].source_text)
        {
            checks.push(PublishCheck {
                code: "unexpected_silence".into(),
                severity: "warning".into(),
                source_range: Some([pair[0].end_ms, pair[1].start_ms]),
                output_range: None,
                scene_id: None,
                message: format!("相邻口播之间存在 {:.1} 秒空窗", gap_ms as f64 / 1_000.0),
                suggested_action: Some("确认是否为有效等待；必要时采用时间线加速建议".into()),
            });
        }
    }
    checks
}

#[tauri::command]
pub fn export_preflight(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<ExportPreflight, AppError> {
    let database = state.database.lock().expect("database mutex poisoned");
    let project = database.get_project(&project_id)?;
    let mut preflight = export_preflight_for_segments(&database.list_segments(&project_id)?);
    apply_safe_background_preflight(&project, &mut preflight);
    Ok(preflight)
}

#[tauri::command]
pub async fn localization_analyze(
    state: State<'_, AppState>,
    project_id: String,
    refresh: Option<bool>,
) -> Result<LocalizationAnalysis, AppError> {
    let (project, segments, existing_edits) = {
        let database = state.database.lock().expect("database mutex poisoned");
        let project = database.get_project(&project_id)?;
        if !refresh.unwrap_or(false) {
            let scenes = database.list_narration_scenes(&project_id)?;
            if !scenes.is_empty() {
                let anchors = database.list_sync_anchors(&project_id)?;
                let timeline_edits = database.list_timeline_edits(&project_id)?;
                let non_speech_events = database.list_non_speech_events(&project_id)?;
                let source_duration_ms = project.duration_ms.unwrap_or(0);
                let map = crate::timeline_map::TimelineMap::from_edits(
                    source_duration_ms,
                    &timeline_edits,
                )?;
                return Ok(LocalizationAnalysis {
                    scenes,
                    anchors,
                    timeline_edits,
                    non_speech_events,
                    source_duration_ms,
                    output_duration_ms: map.output_duration_ms(),
                    estimated_savings_ms: source_duration_ms - map.output_duration_ms(),
                });
            }
        }
        (
            project,
            database.list_segments(&project_id)?,
            database.list_timeline_edits(&project_id)?,
        )
    };
    let source_duration_ms = project
        .duration_ms
        .unwrap_or_else(|| segments.last().map_or(0, |segment| segment.end_ms));
    // Video analysis may scan the full source. Do not hold the shared SQLite
    // mutex while FFmpeg is running.
    let source_path = project
        .source_path
        .as_deref()
        .map(std::path::Path::new)
        .map(std::path::Path::to_path_buf);
    let static_intervals = tauri::async_runtime::spawn_blocking(move || {
        source_path
            .as_deref()
            .map(crate::visual_analysis::detect_static_intervals)
            .unwrap_or_default()
    })
    .await
    .map_err(|error| AppError::Media(format!("画面节奏分析任务失败：{error}")))?;
    let mut analysis = crate::localization::analyze(
        &project_id,
        &segments,
        source_duration_ms,
        &static_intervals,
    )?;
    for edit in &mut analysis.timeline_edits {
        if let Some(existing) = existing_edits.iter().find(|existing| {
            existing.operation == edit.operation
                && (existing.source_start_ms - edit.source_start_ms).abs() <= 250
                && (existing.source_end_ms - edit.source_end_ms).abs() <= 250
        }) {
            edit.accepted = existing.accepted;
        } else if project.workflow_mode == "quick" && edit.confidence >= 0.9 {
            edit.accepted = true;
        }
    }
    state
        .database
        .lock()
        .expect("database mutex poisoned")
        .replace_localization_analysis(
            &project_id,
            &analysis.scenes,
            &analysis.anchors,
            &analysis.timeline_edits,
            &analysis.non_speech_events,
        )?;
    Ok(analysis)
}

#[tauri::command]
pub fn timeline_edit_accept(
    state: State<'_, AppState>,
    project_id: String,
    edit_id: String,
    accepted: bool,
) -> Result<LocalizationAnalysis, AppError> {
    let database = state.database.lock().expect("database mutex poisoned");
    let project = database.get_project(&project_id)?;
    let _ = database.set_timeline_edit_accepted(&project_id, &edit_id, accepted)?;
    let timeline_edits = database.list_timeline_edits(&project_id)?;
    let source_duration_ms = project.duration_ms.unwrap_or(0);
    let map = crate::timeline_map::TimelineMap::from_edits(source_duration_ms, &timeline_edits)?;
    Ok(LocalizationAnalysis {
        scenes: database.list_narration_scenes(&project_id)?,
        anchors: database.list_sync_anchors(&project_id)?,
        timeline_edits,
        non_speech_events: database.list_non_speech_events(&project_id)?,
        source_duration_ms,
        output_duration_ms: map.output_duration_ms(),
        estimated_savings_ms: source_duration_ms - map.output_duration_ms(),
    })
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
    state
        .database
        .lock()
        .expect("database mutex poisoned")
        .list_segments(&project_id)
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
#[allow(clippy::too_many_arguments)]
pub fn project_tts_settings_update(
    state: State<'_, AppState>,
    project_id: String,
    provider_id: String,
    voice_id: Option<String>,
    style: String,
    settings_json: Option<String>,
    director_enabled: bool,
    sync_mode: String,
) -> Result<ProjectSummary, AppError> {
    if !matches!(
        style.as_str(),
        "auto" | "professional" | "conversational" | "documentary" | "upbeat" | "emphasis"
    ) {
        return Err(AppError::Validation("未知的中文配音风格".into()));
    }
    let database = state.database.lock().expect("database mutex poisoned");
    if provider_id != "system" {
        let provider = database.get_provider(&provider_id)?;
        if provider.kind != "cloud_tts" {
            return Err(AppError::Validation("所选服务商不是中文语音服务".into()));
        }
        if provider
            .secret_bundle_ref
            .as_ref()
            .or(provider.credential_ref.as_ref())
            .is_none()
        {
            return Err(AppError::Provider(
                "所选语音服务尚未保存凭据，请先在“服务商”页面完成配置".into(),
            ));
        }
        if sync_mode == "narration"
            && !matches!(provider.driver.as_str(), "aliyun_tts" | "bailian_tts")
        {
            return Err(AppError::Validation(
                "连续旁白当前仅支持阿里百炼 Qwen3-TTS Realtime".into(),
            ));
        }
        if matches!(sync_mode.as_str(), "narration" | "semantic")
            && matches!(provider.driver.as_str(), "aliyun_tts" | "bailian_tts")
        {
            let config = serde_json::from_str::<serde_json::Value>(&provider.public_config_json)
                .map_err(|_| AppError::Provider("阿里语音配置无法解析".into()))?;
            let model = config
                .get("model")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if !matches!(model, "qwen3-tts-instruct-flash" | "qwen3-tts-flash") {
                return Err(AppError::Validation(
                    "连续旁白需要 Qwen3-TTS Instruct Flash 或 Flash 模型".into(),
                ));
            }
        }
    } else if matches!(sync_mode.as_str(), "narration" | "semantic") {
        return Err(AppError::Validation(
            "系统语音不支持连续旁白，请先选择阿里百炼音色".into(),
        ));
    }
    database.set_project_tts_defaults(
        &project_id,
        &provider_id,
        voice_id.as_deref(),
        &style,
        settings_json.as_deref().unwrap_or("{}"),
        director_enabled,
        &sync_mode,
    )
}

#[tauri::command]
pub fn segment_script_update(
    state: State<'_, AppState>,
    input: SegmentScriptUpdateRequest,
) -> Result<SegmentRecord, AppError> {
    input.document.validate()?;
    let database = state.database.lock().expect("database mutex poisoned");
    let document = serde_json::to_string(&input.document)
        .map_err(|error| AppError::Validation(error.to_string()))?;
    database.update_segment_script(
        &input.segment_id,
        &document,
        input.expected_revision,
        &input.tts_overrides_json,
    )
}

#[tauri::command]
pub fn director_plan(
    state: State<'_, AppState>,
    request: DirectorPlanRequest,
) -> Result<ScriptDocumentV1, AppError> {
    let database = state.database.lock().expect("database mutex poisoned");
    let segment = database.get_segment(&request.segment_id)?;
    let project = database.get_project(&segment.project_id)?;
    let terms = database
        .list_glossary_terms(Some(&segment.project_id))?
        .into_iter()
        .filter(|term| term.enabled && term.policy != "disabled")
        .map(|term| ProtectedTerm {
            surface: if term.target.trim().is_empty() {
                term.source.clone()
            } else {
                term.target.clone()
            },
            canonical: if term.source.trim().is_empty() {
                term.target.clone()
            } else {
                term.source.clone()
            },
            pronunciation: None,
            kind: ProtectedKind::Term,
        })
        .collect::<Vec<_>>();
    let style = request.style.as_deref().unwrap_or(&project.tts_style);
    let current =
        ScriptDocumentV1::parse_or_fallback(Some(&segment.script_doc_json), &segment.spoken_zh);
    if current.has_manual_nodes() {
        return Err(AppError::Validation(
            "当前口播稿包含人工调整；为避免覆盖，请先移除人工标记后再重新编排".into(),
        ));
    }
    let document = director::direct_plain_text(&segment.spoken_zh, style, &terms);
    if let Err(missing) = director::canonical_coverage(&current, &document) {
        return Err(AppError::Validation(format!(
            "自动导演不能移除受保护内容：{}",
            missing.join("、")
        )));
    }
    document.validate()?;
    Ok(document)
}

#[tauri::command]
pub fn glossary_list(
    state: State<'_, AppState>,
    project_id: Option<String>,
) -> Result<Vec<GlossaryTerm>, AppError> {
    state
        .database
        .lock()
        .expect("database mutex poisoned")
        .list_glossary_terms(project_id.as_deref())
}

#[tauri::command]
pub fn glossary_save(
    state: State<'_, AppState>,
    term: GlossaryTerm,
) -> Result<GlossaryTerm, AppError> {
    state
        .database
        .lock()
        .expect("database mutex poisoned")
        .save_glossary_term(&term)?;
    Ok(term)
}

#[tauri::command]
pub fn glossary_delete(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    state
        .database
        .lock()
        .expect("database mutex poisoned")
        .delete_glossary_term(&id)
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
        db.checkpoint_job(&job_id, JobStage::Asr, 16, "asr:started")?;
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
                JobStage::Glossary,
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
        database.checkpoint_job(&job_id, JobStage::Translation, 36, "translation:started")?;
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
        let result = {
            let database = state.database.lock().expect("database mutex poisoned");
            apply_project_director(
                &database,
                &project_id,
                &project.tts_style,
                project.tts_director_enabled,
            )?;
            database.checkpoint_job(&job_id, JobStage::Tts, 62, "director:complete")?;
            let target = if project.workflow_mode == "review" {
                JobStatus::WaitingUser
            } else {
                JobStatus::Paused
            };
            let job = database.transition_job(&job_id, target)?;
            emit_job_state(&app, &job);
            database.list_segments(&project_id)?
        };
        return Ok(result);
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
            JobStage::Translation,
            progress,
            &format!("translation:batch-{}/{}", index + 1, total_batches),
        )?;
        emit_job_state(&app, &database.get_job(&job_id)?);
    }
    let result = {
        let database = state.database.lock().expect("database mutex poisoned");
        database.checkpoint_job(&job_id, JobStage::ScriptDirector, 58, "director:started")?;
        // The scene rewriter has already produced the final spoken copy. Running
        // the per-segment director here would rewrite every script a second time,
        // serialize 136 database updates, and reintroduce sentence-level delivery
        // boundaries. Semantic mode instead supplies one continuity instruction
        // to each anchored synthesis block below.
        database.checkpoint_job(&job_id, JobStage::Tts, 62, "director:complete")?;
        let target = if project.workflow_mode == "review" {
            JobStatus::WaitingUser
        } else {
            JobStatus::Paused
        };
        let job = database.transition_job(&job_id, target)?;
        emit_job_state(&app, &job);
        database.list_segments(&project_id)?
    };
    Ok(result)
}

fn apply_project_director(
    database: &crate::db::Database,
    project_id: &str,
    style: &str,
    enabled: bool,
) -> Result<(), AppError> {
    if !enabled {
        return Ok(());
    }
    let terms = database
        .list_glossary_terms(Some(project_id))?
        .into_iter()
        .filter(|term| term.enabled && term.policy != "disabled")
        .map(|term| ProtectedTerm {
            surface: if term.target.trim().is_empty() {
                term.source.clone()
            } else {
                term.target.clone()
            },
            canonical: if term.source.trim().is_empty() {
                term.target.clone()
            } else {
                term.source.clone()
            },
            pronunciation: None,
            kind: ProtectedKind::Term,
        })
        .collect::<Vec<_>>();
    for segment in database.list_segments(project_id)? {
        let current =
            ScriptDocumentV1::parse_or_fallback(Some(&segment.script_doc_json), &segment.spoken_zh);
        if current.has_manual_nodes() || segment.spoken_zh.trim().is_empty() {
            continue;
        }
        let directed = director::direct_plain_text(&segment.spoken_zh, style, &terms);
        if let Err(missing) = director::canonical_coverage(&current, &directed) {
            return Err(AppError::Validation(format!(
                "自动导演不能移除受保护内容：{}",
                missing.join("、")
            )));
        }
        let serialized = serde_json::to_string(&directed)
            .map_err(|error| AppError::Validation(error.to_string()))?;
        database.update_segment_script(
            &segment.id,
            &serialized,
            segment.script_revision,
            &segment.tts_overrides_json,
        )?;
    }
    Ok(())
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
            database.reopen_job(&job_id, JobStage::Translation, 36, "translation:rebuild")?;
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
pub async fn semantic_narration_run(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    job_id: String,
) -> Result<TtsRunResult, AppError> {
    let (segments, profile, resume_from_rewrite) = {
        let database = state.database.lock().expect("database mutex poisoned");
        let project = database.get_project(&project_id)?;
        if project.tts_sync_mode != "semantic" {
            return Err(AppError::Validation("请先切换到语义旁白模式".into()));
        }
        let provider_id = project
            .translation_provider_id
            .clone()
            .ok_or_else(|| AppError::Provider("语义旁白需要先配置阿里百炼翻译服务".into()))?;
        let resume_from_rewrite = database.get_job(&job_id).ok().is_some_and(|job| {
            job.checkpoint.as_deref() == Some("semantic:rewrite-complete")
                || (job.stage == JobStage::Export
                    && job.progress >= 80
                    && job
                        .checkpoint
                        .as_deref()
                        .is_some_and(|checkpoint| checkpoint.starts_with("tts:")))
        });
        (
            database.list_segments(&project_id)?,
            database.get_provider(&provider_id)?,
            resume_from_rewrite,
        )
    };
    if segments.is_empty() {
        return Err(AppError::Validation("项目还没有可改写的识别片段".into()));
    }
    if resume_from_rewrite {
        return tts_run(app, state, project_id, job_id, None).await;
    }
    let reference = profile
        .credential_ref
        .as_ref()
        .ok_or_else(|| AppError::Provider("请先在“服务商”页面填写阿里百炼 API Key".into()))?;
    let secret = credentials::get(reference)?;
    let config = normalize_translation_config(
        serde_json::from_str(&profile.public_config_json)
            .map_err(|_| AppError::Provider("语义旁白翻译配置无法解析".into()))?,
    )?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(|_| AppError::Provider("无法创建语义旁白连接".into()))?;
    let scenes = semantic_scenes(&segments);
    let mut rewritten = Vec::<(String, String)>::new();
    let scene_count = scenes.len();
    {
        let database = state.database.lock().expect("database mutex poisoned");
        database.prepare_tts_job(&job_id)?;
        database.start_job(&job_id)?;
        database.checkpoint_job(
            &job_id,
            JobStage::SemanticNarration,
            57,
            &format!("semantic:scene-0/{scene_count}"),
        )?;
        emit_job_state(&app, &database.get_job(&job_id)?);
    }
    for (scene_index, scene) in scenes.iter().enumerate() {
        let beats = scene
            .beats
            .iter()
            .map(|beat| crate::translation::SemanticBeatInput {
                id: &beat.segments[0].id,
                start_ms: beat.start_ms,
                end_ms: beat.end_ms,
                segments: beat
                    .segments
                    .iter()
                    .map(|segment| crate::translation::SemanticSourceSegment {
                        id: &segment.id,
                        source: &segment.source_text,
                        subtitle_zh: &segment.subtitle_zh,
                        spoken_zh: &segment.spoken_zh,
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        let values =
            match crate::translation::rewrite_semantic_scene(&client, &config, &secret, &beats)
                .await
            {
                Ok(values) => values,
                Err(error) => {
                    // A model timeout must not stall an otherwise usable project.
                    // Reuse the existing natural Chinese script for this scene;
                    // every later TTS beat still receives same-session continuity
                    // and visual-anchor fitting.
                    let fallback = scene
                        .beats
                        .iter()
                        .flat_map(|beat| {
                            beat.segments
                                .iter()
                                .map(|segment| (segment.id.clone(), segment.spoken_zh.clone()))
                        })
                        .collect::<Vec<_>>();
                    if fallback.iter().any(|(_, spoken)| spoken.trim().is_empty()) {
                        return Err(error);
                    }
                    fallback
                }
            };
        rewritten.extend(values);
        let database = state.database.lock().expect("database mutex poisoned");
        let progress = 57 + (((scene_index + 1) * 5 / scenes.len().max(1)) as u8);
        database.checkpoint_job(
            &job_id,
            JobStage::SemanticNarration,
            progress,
            &format!("semantic:scene-{}/{}", scene_index + 1, scenes.len()),
        )?;
        emit_job_state(&app, &database.get_job(&job_id)?);
    }
    {
        let database = state.database.lock().expect("database mutex poisoned");
        for (id, spoken) in rewritten {
            database.update_segment_spoken(&id, &spoken)?;
        }
        // Semantic blocks already carry one block-level delivery instruction.
        // Rebuilding 136 per-row director documents here adds no audible value
        // and can stall the UI before the first TTS progress event.
        database.checkpoint_job(&job_id, JobStage::Tts, 62, "semantic:rewrite-complete")?;
        emit_job_state(&app, &database.get_job(&job_id)?);
    }
    {
        let database = state.database.lock().expect("database mutex poisoned");
        database.handoff_running_job(&job_id, JobStage::Tts, 62, "semantic:rewrite-complete")?;
    }
    tts_run(app, state, project_id, job_id, None).await
}

#[tauri::command]
pub async fn tts_run(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    job_id: String,
    segment_ids: Option<Vec<String>>,
) -> Result<TtsRunResult, AppError> {
    let (project, all_segments, profile, raw_secret, publish_snapshot) = {
        let database = state.database.lock().expect("database mutex poisoned");
        let project = database.get_project(&project_id)?;
        let segments = database.list_segments(&project_id)?;
        if project.tts_provider_id == "system" {
            let snapshot = database.capture_tts_publish_snapshot(&project_id, 1)?;
            (project, segments, None, None, Some(snapshot))
        } else {
            let profile = database.get_provider(&project.tts_provider_id)?;
            let reference = profile
                .secret_bundle_ref
                .as_ref()
                .or(profile.credential_ref.as_ref())
                .ok_or_else(|| AppError::Provider("请先在“服务商”配置中文语音 API Key".into()))?;
            let secret = credentials::get(reference)?;
            let snapshot = database.capture_tts_publish_snapshot(&project_id, profile.revision)?;
            (
                project,
                segments,
                Some(profile),
                Some(secret),
                Some(snapshot),
            )
        }
    };
    let requested = segment_ids.unwrap_or_default();
    let scope = requested
        .iter()
        .map(String::as_str)
        .collect::<std::collections::HashSet<_>>();
    let segments = if scope.is_empty() {
        all_segments.clone()
    } else {
        let selected = all_segments
            .iter()
            .filter(|segment| scope.contains(segment.id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if selected.len() != scope.len() {
            return Err(AppError::Validation("局部生成包含未知片段".into()));
        }
        selected
    }
    .into_iter()
    .filter(|segment| !crate::localization::is_non_speech_text(&segment.source_text))
    .collect::<Vec<_>>();
    if segments.is_empty() {
        return Err(AppError::Validation(
            "所选范围只有音乐或环境声事件，不需要生成口播".into(),
        ));
    }
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
            .clone()
            .ok_or_else(|| AppError::Media("请先完成媒体准备".into()))?,
    );
    let published_track = artifact_dir.join("chinese-voice.wav");
    let track_revision_before = file_revision(&published_track);
    {
        let database = state.database.lock().expect("database mutex poisoned");
        database.prepare_tts_job(&job_id)?;
        database.start_job(&job_id)?;
        database.checkpoint_job(&job_id, JobStage::Tts, 63, "tts:started")?;
        emit_job_state(&app, &database.get_job(&job_id)?);
    }
    let duration_ms = project
        .duration_ms
        .unwrap_or_else(|| all_segments.last().map_or(0, |segment| segment.end_ms));
    let synthesis = if project.tts_provider_id == "system" {
        synthesize_system_segments(
            &app,
            &state,
            &job_id,
            &project,
            &all_segments,
            &segments,
            publish_snapshot.as_ref().expect("system publish snapshot"),
            &artifact_dir,
            duration_ms,
        )
        .await
    } else if matches!(project.tts_sync_mode.as_str(), "semantic" | "narration") {
        synthesize_cloud_narration(
            &app,
            &state,
            &job_id,
            &project,
            &all_segments,
            &segments,
            profile.as_ref().expect("cloud profile"),
            raw_secret.as_deref().expect("cloud secret"),
            publish_snapshot.as_ref().expect("cloud publish snapshot"),
            &artifact_dir,
            duration_ms,
        )
        .await
    } else if project.tts_sync_mode == "balanced" {
        synthesize_cloud_blocks(
            &app,
            &state,
            &job_id,
            &project,
            &all_segments,
            &segments,
            profile.as_ref().expect("cloud profile"),
            raw_secret.as_deref().expect("cloud secret"),
            publish_snapshot.as_ref().expect("cloud publish snapshot"),
            &artifact_dir,
            duration_ms,
        )
        .await
    } else {
        synthesize_cloud_segments(
            &app,
            &state,
            &job_id,
            &project,
            &all_segments,
            &segments,
            profile.as_ref().expect("cloud profile"),
            raw_secret.as_deref().expect("cloud secret"),
            publish_snapshot.as_ref().expect("cloud publish snapshot"),
            &artifact_dir,
            duration_ms,
        )
        .await
    };
    let finalization = match synthesis {
        Ok(output) => {
            async {
                {
                    let database = state.database.lock().expect("database mutex poisoned");
                    if database.get_job(&job_id)?.status != JobStatus::Running {
                        return Err(AppError::Validation(
                            "配音任务已取消，未发布后续预览".into(),
                        ));
                    }
                }
                let revision = file_revision(&published_track);
                let track_published = revision > 0 && revision != track_revision_before;
                let preview_media = if track_published {
                    let preview_lock = project_preview_lock(&state, &project_id);
                    match tauri::async_runtime::spawn_blocking({
                        let artifact_dir = artifact_dir.clone();
                        let audio_mode = audio_mode.clone();
                        move || {
                            let _guard =
                                preview_lock.lock().expect("project preview mutex poisoned");
                            crate::media::render_dubbed_preview(&artifact_dir, &audio_mode)
                        }
                    })
                    .await
                    {
                        Ok(Ok(preview_path)) => Some(PreviewMedia {
                            path: preview_path.to_string_lossy().into_owned(),
                            dubbed: true,
                            revision: file_revision(&preview_path),
                        }),
                        _ => None,
                    }
                } else {
                    None
                };
                let database = state.database.lock().expect("database mutex poisoned");
                let checkpoint = format!(
                    "tts:{}|cache:{}/{}",
                    if track_published {
                        published_track.to_string_lossy().into_owned()
                    } else {
                        "partial".into()
                    },
                    output.cache_hit_unit_count,
                    output.synthesis_unit_count
                );
                if track_published {
                    database.checkpoint_job(&job_id, JobStage::Export, 80, &checkpoint)?;
                } else {
                    let progress = database.get_job(&job_id)?.progress;
                    database.checkpoint_job(&job_id, JobStage::Tts, progress, &checkpoint)?;
                }
                let needs_attention = !track_published
                    || !output.warning_ids.is_empty()
                    || !output.failed_segments.is_empty();
                let status = if needs_attention {
                    JobStatus::WaitingUser
                } else {
                    JobStatus::Paused
                };
                let job = database.transition_job(&job_id, status)?;
                emit_job_state(&app, &job);
                Ok(TtsRunResult {
                    warning_ids: output.warning_ids,
                    failed_segments: output.failed_segments,
                    affected_segment_ids: output.affected_segment_ids,
                    synthesis_unit_count: output.synthesis_unit_count,
                    cache_hit_unit_count: output.cache_hit_unit_count,
                    track_revision: revision,
                    preview_media,
                })
            }
            .await
        }
        Err(error) => Err(error),
    };
    match finalization {
        Ok(result) => Ok(result),
        Err(error) => {
            let database = state.database.lock().expect("database mutex poisoned");
            if let Ok(job) = database.fail_job(&job_id, &error.to_string()) {
                emit_job_state(&app, &job)
            }
            Err(error)
        }
    }
}

fn update_tts_progress(app: &AppHandle, job_id: &str, done: usize, total: usize) {
    update_tts_progress_kind(app, job_id, done, total, "segment");
}

fn update_tts_progress_kind(app: &AppHandle, job_id: &str, done: usize, total: usize, unit: &str) {
    if unit == "segment" && done != total && !done.is_multiple_of(4) {
        return;
    }
    let managed_state = app.state::<AppState>();
    let database = managed_state
        .database
        .lock()
        .expect("database mutex poisoned");
    // The preceding director stage reaches 80%. Keep TTS checkpoints above that
    // persisted watermark so the queue shows real per-segment progress.
    let percent = 80 + (done * 14 / total.max(1)) as u8;
    if database
        .checkpoint_job(
            job_id,
            JobStage::Tts,
            percent,
            &format!("tts:{unit}-{done}/{total}"),
        )
        .is_ok()
    {
        if let Ok(job) = database.get_job(job_id) {
            emit_job_state(app, &job);
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn synthesize_system_segments(
    app: &AppHandle,
    state: &State<'_, AppState>,
    job_id: &str,
    project: &ProjectSummary,
    all_segments: &[SegmentRecord],
    selected_segments: &[SegmentRecord],
    publish_snapshot: &TtsPublishSnapshot,
    artifact_dir: &std::path::Path,
    duration_ms: i64,
) -> Result<TtsPipelineOutcome, AppError> {
    let tts_dir = artifact_dir.join("tts-system-v3");
    std::fs::create_dir_all(&tts_dir).map_err(|error| AppError::Media(error.to_string()))?;
    let selected_ids = selected_segments
        .iter()
        .map(|segment| segment.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let full_run = selected_ids.len() == all_segments.len();
    if !full_run {
        validate_system_partial_cache(project, all_segments, &selected_ids, &tts_dir)?;
    }

    let adapter = SystemTtsAdapter;
    let secret = TtsSecretBundle::local();
    let run_id = Uuid::new_v4().to_string();
    let mut selected_clips = std::collections::HashMap::<String, PathBuf>::new();
    let mut segment_updates = Vec::<TtsSegmentPublication>::new();
    let mut segment_artifacts = Vec::<ArtifactRecord>::new();
    let mut warning_ids = Vec::new();
    let mut cache_hit_unit_count = 0;
    for (index, segment) in selected_segments.iter().enumerate() {
        let settings_hash = system_tts_settings_hash(project, segment);
        let target = system_segment_cache_path(&tts_dir, segment, &settings_hash);
        let ready = reusable_system_clip(&tts_dir, project, segment).is_some();
        cache_hit_unit_count += usize::from(ready);
        let (actual_ms, content_hash, new_artifact) = if ready {
            (
                crate::tts::duration_ms_of(&target)?,
                file_sha256(&target)?,
                false,
            )
        } else {
            let overrides: TtsOverrides = serde_json::from_str(&segment.tts_overrides_json)
                .unwrap_or(TtsOverrides {
                    voice_id: None,
                    style: None,
                    speed: None,
                    director_enabled: None,
                });
            let request = SynthesisRequest {
                text: segment.spoken_zh.clone(),
                voice_id: overrides
                    .voice_id
                    .or_else(|| project.tts_voice_id.clone())
                    .unwrap_or_else(|| "Tingting".into()),
                style: overrides.style.unwrap_or_else(|| project.tts_style.clone()),
                instructions: None,
                speed: overrides.speed.unwrap_or(1.0).clamp(0.8, 1.08),
                pitch: 1.0,
                volume: 1.0,
                sample_rate: 24_000,
                target_duration_ms: Some(segment.end_ms - segment.start_ms),
            };
            let audio = adapter.synthesize(&request, &secret).await?;
            let temporary = tts_dir.join(format!("{}.{}.raw.pending", segment.id, run_id));
            let normalized = tts_dir.join(format!("{}.{}.pending.wav", segment.id, run_id));
            std::fs::write(&temporary, &audio.bytes)
                .map_err(|error| AppError::Media(error.to_string()))?;
            let normalized_result = normalize_provider_audio(
                &temporary,
                &normalized,
                audio.encoding,
                audio.sample_rate,
            );
            let _ = std::fs::remove_file(&temporary);
            if let Err(error) = normalized_result {
                let _ = std::fs::remove_file(&normalized);
                return Err(error);
            }
            let initial_ms = crate::tts::duration_ms_of(&normalized)?;
            crate::tts::validate_clip_completeness(&request.text, initial_ms)?;
            let fitted = tts_dir.join(format!("{}.{}.pending.fit.wav", segment.id, run_id));
            let actual_ms = crate::tts::fit_clip_to_window(
                &normalized,
                &fitted,
                segment.end_ms - segment.start_ms,
            )?;
            let publish_source = if fitted.is_file() {
                &fitted
            } else {
                &normalized
            };
            let content_hash = file_sha256(publish_source)?;
            std::fs::rename(publish_source, &target)
                .map_err(|error| AppError::Media(format!("无法原子发布系统片段配音：{error}")))?;
            if normalized.is_file() {
                let _ = std::fs::remove_file(&normalized);
            }
            (actual_ms, content_hash, true)
        };
        let too_long = actual_ms > segment.end_ms - segment.start_ms + 150;
        if too_long {
            warning_ids.push(segment.id.clone());
        } else {
            selected_clips.insert(segment.id.clone(), target.clone());
        }
        segment_updates.push(TtsSegmentPublication {
            segment_id: segment.id.clone(),
            expected_script_revision: segment.script_revision,
            state: if too_long { "stale" } else { "ready" }.into(),
            settings_hash: (!too_long).then(|| settings_hash.clone()),
            duration_ms: (!too_long).then_some(actual_ms),
            error_message: too_long.then(|| "系统语音超过片段时长，请压缩口播稿或减少停顿".into()),
            display_status: if too_long { "warning" } else { "ready" }.into(),
        });
        if new_artifact && !too_long {
            segment_artifacts.push(ArtifactRecord {
                id: format!("tts-system-{}-{}", segment.id, &settings_hash[..16]),
                project_id: project.id.clone(),
                segment_id: Some(segment.id.clone()),
                kind: "tts_aligned".into(),
                path: target.to_string_lossy().into_owned(),
                content_hash,
                dependency_hash: settings_hash.clone(),
                cache_key: Some(settings_hash),
                revision: 1,
                status: "ready".into(),
                metadata_json: serde_json::json!({ "durationMs": actual_ms, "providerId": "system", "voiceId": "Tingting" }).to_string(),
                created_at: String::new(),
                updated_at: String::new(),
            });
        }
        update_tts_progress(app, job_id, index + 1, selected_segments.len());
    }

    if !warning_ids.is_empty() {
        state
            .database
            .lock()
            .expect("database mutex poisoned")
            .commit_tts_publication(
                job_id,
                publish_snapshot,
                &segment_updates,
                &segment_artifacts,
            )?;
        return Ok(TtsPipelineOutcome {
            warning_ids,
            failed_segments: Vec::new(),
            affected_segment_ids: selected_segments
                .iter()
                .map(|segment| segment.id.clone())
                .collect(),
            synthesis_unit_count: selected_segments.len(),
            cache_hit_unit_count,
        });
    }

    let mut inputs = Vec::new();
    for segment in all_segments {
        let clip = selected_clips
            .get(&segment.id)
            .cloned()
            .or_else(|| reusable_system_clip(&tts_dir, project, segment));
        if let Some(path) = clip {
            let duration = crate::tts::duration_ms_of(&path)?;
            inputs.push((
                path,
                crate::tts::positioned_start_ms(segment.start_ms, segment.end_ms, duration),
            ));
        }
    }
    if inputs.len() != all_segments.len() {
        return Err(AppError::Validation(
            "局部生成前需先完成一次全片配音，以建立可复用的系统语音缓存".into(),
        ));
    }
    publish_mixed_track(
        state,
        job_id,
        project,
        all_segments,
        publish_snapshot,
        artifact_dir,
        duration_ms,
        &run_id,
        &inputs,
        &segment_updates,
        &mut segment_artifacts,
    )?;
    Ok(TtsPipelineOutcome {
        warning_ids,
        failed_segments: Vec::new(),
        affected_segment_ids: selected_segments
            .iter()
            .map(|segment| segment.id.clone())
            .collect(),
        synthesis_unit_count: selected_segments.len(),
        cache_hit_unit_count,
    })
}

#[allow(clippy::too_many_arguments)]
#[derive(Debug, Clone)]
struct BalancedTtsBlock<'a> {
    segments: Vec<&'a SegmentRecord>,
    start_ms: i64,
    end_ms: i64,
}

#[derive(Debug)]
struct SemanticScene<'a> {
    beats: Vec<BalancedTtsBlock<'a>>,
    start_ms: i64,
    end_ms: i64,
}

fn semantic_scenes(segments: &[SegmentRecord]) -> Vec<SemanticScene<'_>> {
    let mut scenes = Vec::<SemanticScene<'_>>::new();
    for beat in balanced_tts_blocks(segments) {
        let split = scenes.last().is_some_and(|scene| {
            let next_span = beat.end_ms - scene.start_ms;
            let span = scene.end_ms - scene.start_ms;
            let gap = beat.start_ms - scene.end_ms;
            next_span > 30_000 || (span >= 22_000 && gap >= 700) || span >= 28_000
        });
        if split || scenes.is_empty() {
            scenes.push(SemanticScene {
                start_ms: beat.start_ms,
                end_ms: beat.end_ms,
                beats: vec![beat],
            });
        } else if let Some(scene) = scenes.last_mut() {
            scene.end_ms = beat.end_ms;
            scene.beats.push(beat);
        }
    }
    scenes
}

#[derive(Debug)]
struct NarrationChapter<'a> {
    segments: Vec<&'a SegmentRecord>,
    start_ms: i64,
    end_ms: i64,
}

fn narration_chapters(segments: &[SegmentRecord]) -> Vec<NarrationChapter<'_>> {
    let mut chapters = Vec::<NarrationChapter<'_>>::new();
    for segment in segments
        .iter()
        .filter(|segment| !crate::localization::is_non_speech_text(&segment.source_text))
    {
        let should_split = chapters.last().is_some_and(|chapter| {
            let first = chapter.segments[0];
            let span_ms = chapter.end_ms - chapter.start_ms;
            let next_span_ms = segment.end_ms - chapter.start_ms;
            let gap_ms = segment.start_ms - chapter.end_ms;
            let first_overrides: TtsOverrides = serde_json::from_str(&first.tts_overrides_json)
                .unwrap_or(TtsOverrides {
                    voice_id: None,
                    style: None,
                    speed: None,
                    director_enabled: None,
                });
            let next_overrides: TtsOverrides = serde_json::from_str(&segment.tts_overrides_json)
                .unwrap_or(TtsOverrides {
                    voice_id: None,
                    style: None,
                    speed: None,
                    director_enabled: None,
                });
            let explicit_voice_changed = first_overrides.voice_id.is_some()
                && next_overrides.voice_id.is_some()
                && first_overrides.voice_id != next_overrides.voice_id;
            explicit_voice_changed
                || next_span_ms > 30_000
                || (span_ms >= 22_000 && gap_ms >= 700)
                || (span_ms >= 16_000
                    && gap_ms >= 1_300
                    && first.spoken_zh.trim_end().ends_with(['。', '！', '？']))
        });
        if should_split || chapters.is_empty() {
            chapters.push(NarrationChapter {
                segments: vec![segment],
                start_ms: segment.start_ms,
                end_ms: segment.end_ms,
            });
        } else if let Some(chapter) = chapters.last_mut() {
            chapter.segments.push(segment);
            chapter.end_ms = segment.end_ms;
        }
    }
    chapters
}

#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
async fn synthesize_cloud_semantic(
    app: &AppHandle,
    state: &State<'_, AppState>,
    job_id: &str,
    project: &ProjectSummary,
    all_segments: &[SegmentRecord],
    selected_segments: &[SegmentRecord],
    profile: &ProviderProfile,
    raw_secret: &str,
    publish_snapshot: &TtsPublishSnapshot,
    artifact_dir: &std::path::Path,
    duration_ms: i64,
) -> Result<TtsPipelineOutcome, AppError> {
    use sha2::{Digest, Sha256};

    if !matches!(profile.driver.as_str(), "aliyun_tts" | "bailian_tts") {
        return Err(AppError::Provider(
            "语义旁白当前仅支持阿里百炼 Qwen3-TTS Realtime".into(),
        ));
    }
    let secret = TtsSecretBundle::from_keychain_value(&profile.driver, raw_secret)?;
    let public_config = serde_json::from_str::<serde_json::Value>(&profile.public_config_json)
        .map_err(|_| AppError::Provider("阿里语音配置无法解析".into()))?;
    let config = AliyunTtsConfig {
        endpoint: public_config
            .get("endpoint")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        region: public_config
            .get("region")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("cn-beijing")
            .to_string(),
        model: public_config
            .get("model")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| AppError::Provider("阿里语音配置缺少 model".into()))?
            .to_string(),
        optimize_instructions: public_config
            .get("optimizeInstructions")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        sample_rate: 24_000,
    };
    let adapter = AliyunTtsAdapter::new(config.clone())?;
    let tts_dir = artifact_dir.join("tts-v5-semantic");
    std::fs::create_dir_all(&tts_dir).map_err(|error| AppError::Media(error.to_string()))?;
    let default_voice = project.tts_voice_id.as_deref().unwrap_or("Cherry");
    let requested_ids = selected_segments
        .iter()
        .map(|segment| segment.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let scenes = semantic_scenes(all_segments);
    let selected_scene_count = scenes
        .iter()
        .filter(|scene| {
            scene.beats.iter().any(|beat| {
                beat.segments
                    .iter()
                    .any(|segment| requested_ids.contains(segment.id.as_str()))
            })
        })
        .count();
    let affected_segment_ids = scenes
        .iter()
        .filter(|scene| {
            scene.beats.iter().any(|beat| {
                beat.segments
                    .iter()
                    .any(|segment| requested_ids.contains(segment.id.as_str()))
            })
        })
        .flat_map(|scene| {
            scene
                .beats
                .iter()
                .flat_map(|beat| beat.segments.iter().map(|segment| segment.id.clone()))
        })
        .collect::<Vec<_>>();
    let mut warning_ids = Vec::new();
    let mut failed_segments = Vec::<TtsSegmentFailure>::new();
    let mut segment_updates = Vec::<TtsSegmentPublication>::new();
    let mut segment_artifacts = Vec::<ArtifactRecord>::new();
    let mut inputs = Vec::<(PathBuf, i64)>::new();
    let run_id = Uuid::new_v4().to_string();
    let mut completed_scenes = 0;
    let mut cache_hit_unit_count = 0;
    update_tts_progress_kind(app, job_id, 0, selected_scene_count, "scene");

    for (scene_index, scene) in scenes.iter().enumerate() {
        let selected = scene.beats.iter().any(|beat| {
            beat.segments
                .iter()
                .any(|segment| requested_ids.contains(segment.id.as_str()))
        });
        let first = scene.beats[0].segments[0];
        let overrides: TtsOverrides =
            serde_json::from_str(&first.tts_overrides_json).unwrap_or(TtsOverrides {
                voice_id: None,
                style: None,
                speed: None,
                director_enabled: None,
            });
        let voice_id = overrides.voice_id.as_deref().unwrap_or(default_voice);
        let style = overrides.style.as_deref().unwrap_or(&project.tts_style);
        let speed = overrides.speed.unwrap_or(1.0).clamp(0.9, 1.05);
        let director_enabled = overrides
            .director_enabled
            .unwrap_or(project.tts_director_enabled);
        let beat_texts = scene
            .beats
            .iter()
            .map(|beat| {
                beat.segments
                    .iter()
                    .map(|segment| {
                        ScriptDocumentV1::parse_or_fallback(
                            Some(&segment.script_doc_json),
                            &segment.spoken_zh,
                        )
                        .render_directed_text()
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>();
        if beat_texts.iter().any(|text| text.trim().is_empty()) {
            return Err(AppError::Validation(
                "语义旁白场景中仍有空白节拍，请重新生成整片语义旁白".into(),
            ));
        }
        let instructions = (director_enabled && config.model.contains("instruct")).then(|| {
            let document = ScriptDocumentV1::parse_or_fallback(
                Some(&first.script_doc_json),
                &first.spoken_zh,
            );
            let base = document.director_instruction(style);
            format!("{base}。这是同一位讲述者连续完成的中文技术视频旁白。整个会话保持音色、气息、音量、语速和情绪连续；每次提交只是画面对齐节拍，不要因此重新起范、报幕或刻意收尾。严格只朗读正文。")
        });
        let settings_json = serde_json::json!({
            "provider": profile.id, "providerRevision": profile.revision, "driver": profile.driver,
            "model": config.model, "voice": voice_id, "style": style, "speed": speed,
            "sampleRate": 24000, "sceneStartMs": scene.start_ms, "sceneEndMs": scene.end_ms,
            "beats": scene.beats.iter().zip(&beat_texts).map(|(beat, text)| serde_json::json!({
                "startMs": beat.start_ms, "endMs": beat.end_ms, "text": text,
                "segments": beat.segments.iter().map(|segment| serde_json::json!({
                    "id": segment.id, "scriptRevision": segment.script_revision,
                })).collect::<Vec<_>>()
            })).collect::<Vec<_>>(),
            "directorEnabled": director_enabled, "instructions": instructions,
            "syncMode": "semantic", "scenePolicyVersion": 1, "beatPolicyVersion": 1,
            "audioPipelineVersion": 5,
        });
        let settings_hash = hex::encode(Sha256::digest(
            serde_json::to_vec(&settings_json)
                .map_err(|error| AppError::Validation(error.to_string()))?,
        ));
        let targets = scene
            .beats
            .iter()
            .enumerate()
            .map(|(beat_index, beat)| {
                tts_dir.join(format!(
                    "scene-{:02}-beat-{:02}-{}-{}.wav",
                    scene_index + 1,
                    beat_index + 1,
                    beat.segments[0].id,
                    &settings_hash[..24]
                ))
            })
            .collect::<Vec<_>>();
        let ready = targets
            .iter()
            .all(|target| target.metadata().is_ok_and(|metadata| metadata.len() > 44));
        cache_hit_unit_count += usize::from(selected && ready);
        if !selected && !ready {
            return Err(AppError::Validation(
                "局部生成前需先完成一次语义旁白整片配音，以建立场景节拍缓存".into(),
            ));
        }

        let mut durations = Vec::<i64>::new();
        let mut content_hashes = Vec::<String>::new();
        let mut generated = false;
        if ready {
            for target in &targets {
                durations.push(crate::tts::duration_ms_of(target)?);
                content_hashes.push(file_sha256(target)?);
            }
        } else {
            let request = SynthesisRequest {
                text: beat_texts.join(" "),
                voice_id: voice_id.into(),
                style: style.into(),
                instructions: instructions.clone(),
                speed,
                pitch: 1.0,
                volume: 1.0,
                sample_rate: 24_000,
                target_duration_ms: Some(scene.end_ms - scene.start_ms),
            };
            let audios = match adapter
                .synthesize_realtime_beats(&request, &beat_texts, &secret)
                .await
            {
                Ok(audios) if audios.len() == scene.beats.len() => audios,
                Ok(_) => {
                    return Err(AppError::Provider(
                        "阿里实时语音返回的节拍数量不完整".into(),
                    ))
                }
                Err(error) => {
                    let message = error.to_string();
                    for beat in &scene.beats {
                        for segment in &beat.segments {
                            failed_segments.push(TtsSegmentFailure {
                                segment_id: segment.id.clone(),
                                message: message.clone(),
                            });
                            segment_updates.push(TtsSegmentPublication {
                                segment_id: segment.id.clone(),
                                expected_script_revision: segment.script_revision,
                                state: "failed".into(),
                                settings_hash: None,
                                duration_ms: None,
                                error_message: Some(format!("所在语义场景生成失败：{message}")),
                                display_status: "warning".into(),
                            });
                        }
                    }
                    completed_scenes += usize::from(selected);
                    if selected {
                        update_tts_progress_kind(
                            app,
                            job_id,
                            completed_scenes,
                            selected_scene_count,
                            "scene",
                        );
                    }
                    continue;
                }
            };
            for (beat_index, ((beat, audio), target)) in
                scene.beats.iter().zip(audios).zip(&targets).enumerate()
            {
                let raw = tts_dir.join(format!(
                    "scene-{}-beat-{}.{}.raw.pending",
                    scene_index + 1,
                    beat_index + 1,
                    run_id
                ));
                let normalized = tts_dir.join(format!(
                    "scene-{}-beat-{}.{}.pending.wav",
                    scene_index + 1,
                    beat_index + 1,
                    run_id
                ));
                std::fs::write(&raw, &audio.bytes)
                    .map_err(|error| AppError::Media(error.to_string()))?;
                let normalized_result =
                    normalize_provider_audio(&raw, &normalized, audio.encoding, audio.sample_rate);
                let _ = std::fs::remove_file(&raw);
                normalized_result?;
                let fitted = tts_dir.join(format!(
                    "scene-{}-beat-{}.{}.pending.fit.wav",
                    scene_index + 1,
                    beat_index + 1,
                    run_id
                ));
                let fitted_ms = crate::tts::fit_clip_to_window(
                    &normalized,
                    &fitted,
                    beat.end_ms - beat.start_ms,
                )?;
                let publish_source = if fitted.is_file() {
                    fitted.as_path()
                } else {
                    normalized.as_path()
                };
                let content_hash = file_sha256(publish_source)?;
                std::fs::rename(publish_source, target).map_err(|error| {
                    AppError::Media(format!("无法原子发布语义旁白节拍：{error}"))
                })?;
                if normalized.is_file() {
                    let _ = std::fs::remove_file(&normalized);
                }
                durations.push(fitted_ms);
                content_hashes.push(content_hash);
            }
            generated = true;
        }

        for (beat_index, beat) in scene.beats.iter().enumerate() {
            let actual_ms = durations[beat_index];
            let too_long = actual_ms > beat.end_ms - beat.start_ms + 150;
            if too_long {
                warning_ids.extend(beat.segments.iter().map(|segment| segment.id.clone()));
            } else {
                inputs.push((
                    targets[beat_index].clone(),
                    crate::tts::positioned_start_ms(beat.start_ms, beat.end_ms, actual_ms),
                ));
            }
            if selected {
                for segment in &beat.segments {
                    segment_updates.push(TtsSegmentPublication {
                        segment_id: segment.id.clone(),
                        expected_script_revision: segment.script_revision,
                        state: if too_long { "stale" } else { "ready" }.into(),
                        settings_hash: (!too_long).then(|| settings_hash.clone()),
                        duration_ms: None,
                        error_message: too_long
                            .then(|| "语义旁白节拍超过画面对齐窗口，请精简当前节拍".into()),
                        display_status: if too_long { "warning" } else { "ready" }.into(),
                    });
                }
                if generated && !too_long {
                    segment_artifacts.push(ArtifactRecord {
                        id: format!("tts-semantic-{}-{}", beat.segments[0].id, &settings_hash[..16]),
                        project_id: project.id.clone(), segment_id: None,
                        kind: "tts_semantic_beat".into(), path: targets[beat_index].to_string_lossy().into_owned(),
                        content_hash: content_hashes[beat_index].clone(), dependency_hash: settings_hash.clone(),
                        cache_key: Some(settings_hash.clone()), revision: 1, status: "ready".into(),
                        metadata_json: serde_json::json!({
                            "durationMs": actual_ms, "startMs": beat.start_ms, "endMs": beat.end_ms,
                            "sceneStartMs": scene.start_ms, "sceneEndMs": scene.end_ms,
                            "segmentIds": beat.segments.iter().map(|segment| &segment.id).collect::<Vec<_>>(),
                            "providerId": profile.id, "providerRevision": profile.revision,
                            "voiceId": voice_id, "style": style, "syncMode": "semantic",
                            "sameRealtimeSession": true, "commitBoundary": "beat",
                        }).to_string(), created_at: String::new(), updated_at: String::new(),
                    });
                }
            }
        }
        if selected {
            completed_scenes += 1;
            update_tts_progress_kind(app, job_id, completed_scenes, selected_scene_count, "scene");
        }
    }

    if !warning_ids.is_empty() || !failed_segments.is_empty() {
        let database = state.database.lock().expect("database mutex poisoned");
        database.commit_tts_publication(
            job_id,
            publish_snapshot,
            &segment_updates,
            &segment_artifacts,
        )?;
        return Ok(TtsPipelineOutcome {
            warning_ids,
            failed_segments,
            affected_segment_ids,
            synthesis_unit_count: selected_scene_count,
            cache_hit_unit_count,
        });
    }
    let expected_beats = scenes.iter().map(|scene| scene.beats.len()).sum::<usize>();
    if inputs.len() != expected_beats {
        return Err(AppError::Validation(
            "仍有语义旁白节拍没有可用配音，未覆盖旧的中文音轨".into(),
        ));
    }
    publish_mixed_track(
        state,
        job_id,
        project,
        all_segments,
        publish_snapshot,
        artifact_dir,
        duration_ms,
        &run_id,
        &inputs,
        &segment_updates,
        &mut segment_artifacts,
    )?;
    Ok(TtsPipelineOutcome {
        warning_ids,
        failed_segments,
        affected_segment_ids,
        synthesis_unit_count: selected_scene_count,
        cache_hit_unit_count,
    })
}

#[allow(clippy::too_many_arguments)]
async fn synthesize_cloud_narration(
    app: &AppHandle,
    state: &State<'_, AppState>,
    job_id: &str,
    project: &ProjectSummary,
    all_segments: &[SegmentRecord],
    selected_segments: &[SegmentRecord],
    profile: &ProviderProfile,
    raw_secret: &str,
    publish_snapshot: &TtsPublishSnapshot,
    artifact_dir: &std::path::Path,
    duration_ms: i64,
) -> Result<TtsPipelineOutcome, AppError> {
    use sha2::{Digest, Sha256};

    if !matches!(profile.driver.as_str(), "aliyun_tts" | "bailian_tts") {
        return Err(AppError::Provider(
            "连续旁白当前仅支持阿里百炼 Qwen3-TTS Realtime".into(),
        ));
    }
    let secret = TtsSecretBundle::from_keychain_value(&profile.driver, raw_secret)?;
    let config = serde_json::from_str::<AliyunTtsConfig>(&profile.public_config_json)
        .map_err(|_| AppError::Provider("阿里语音配置无法解析".into()))?;
    let adapter = AliyunTtsAdapter::new(config.clone())?;
    let tts_dir = artifact_dir.join("tts-v4-narration");
    std::fs::create_dir_all(&tts_dir).map_err(|error| AppError::Media(error.to_string()))?;
    let default_voice = project.tts_voice_id.as_deref().unwrap_or("Cherry");
    let requested_ids = selected_segments
        .iter()
        .map(|segment| segment.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let chapters = narration_chapters(all_segments);
    let selected_chapter_count = chapters
        .iter()
        .filter(|chapter| {
            chapter
                .segments
                .iter()
                .any(|segment| requested_ids.contains(segment.id.as_str()))
        })
        .count();
    let affected_segment_ids = chapters
        .iter()
        .filter(|chapter| {
            chapter
                .segments
                .iter()
                .any(|segment| requested_ids.contains(segment.id.as_str()))
        })
        .flat_map(|chapter| chapter.segments.iter().map(|segment| segment.id.clone()))
        .collect::<Vec<_>>();
    let mut warning_ids = Vec::new();
    let mut failed_segments = Vec::<TtsSegmentFailure>::new();
    let mut segment_updates = Vec::<TtsSegmentPublication>::new();
    let mut segment_artifacts = Vec::<ArtifactRecord>::new();
    let mut inputs = Vec::<(PathBuf, i64)>::new();
    let run_id = Uuid::new_v4().to_string();
    let mut completed_chapters = 0;
    let mut cache_hit_unit_count = 0;

    update_tts_progress_kind(app, job_id, 0, selected_chapter_count, "chapter");

    for (chapter_index, chapter) in chapters.iter().enumerate() {
        let selected = chapter
            .segments
            .iter()
            .any(|segment| requested_ids.contains(segment.id.as_str()));
        let first = chapter.segments[0];
        let overrides: TtsOverrides =
            serde_json::from_str(&first.tts_overrides_json).unwrap_or(TtsOverrides {
                voice_id: None,
                style: None,
                speed: None,
                director_enabled: None,
            });
        let voice_id = overrides.voice_id.as_deref().unwrap_or(default_voice);
        let style = overrides.style.as_deref().unwrap_or(&project.tts_style);
        let speed = overrides.speed.unwrap_or(1.0).clamp(0.9, 1.05);
        let director_enabled = overrides
            .director_enabled
            .unwrap_or(project.tts_director_enabled);
        let documents = chapter
            .segments
            .iter()
            .map(|segment| {
                ScriptDocumentV1::parse_or_fallback(
                    Some(&segment.script_doc_json),
                    &segment.spoken_zh,
                )
            })
            .collect::<Vec<_>>();
        for document in &documents {
            document.validate()?;
        }
        let text_chunks = documents
            .iter()
            .map(ScriptDocumentV1::render_directed_text)
            .collect::<Vec<_>>();
        if text_chunks.iter().any(|text| text.trim().is_empty()) {
            return Err(AppError::Validation(
                "连续旁白章节中仍有空白口播稿，请先完成翻译".into(),
            ));
        }
        let text = text_chunks.join(" ");
        let instructions = (director_enabled && config.model.contains("instruct")).then(|| {
            let base = documents[0].director_instruction(style);
            format!("{base}。这是一段从头到尾由同一个人完成的连续中文视频旁白。保持音色、气息、音量、语速和情绪状态连续；相邻句自然衔接，不要逐句重新起范，不要报幕，不要朗读任何指令，只朗读正文。")
        });
        let settings_json = serde_json::json!({
            "provider": profile.id, "providerRevision": profile.revision, "driver": profile.driver,
            "model": config.model, "voice": voice_id, "style": style, "speed": speed,
            "sampleRate": 24000, "segments": chapter.segments.iter().map(|segment| serde_json::json!({
                "id": segment.id, "scriptRevision": segment.script_revision, "spokenZh": segment.spoken_zh,
            })).collect::<Vec<_>>(),
            "directorEnabled": director_enabled, "instructions": instructions,
            "syncMode": "narration", "chapterPolicyVersion": 1, "chapterFitPolicyVersion": 2,
            "audioPipelineVersion": 4,
        });
        let settings_hash = hex::encode(Sha256::digest(
            serde_json::to_vec(&settings_json)
                .map_err(|error| AppError::Validation(error.to_string()))?,
        ));
        let target = tts_dir.join(format!(
            "chapter-{:02}-{}-{}.wav",
            chapter_index + 1,
            first.id,
            &settings_hash[..24]
        ));
        let ready = target.metadata().is_ok_and(|metadata| metadata.len() > 44);
        cache_hit_unit_count += usize::from(selected && ready);
        if !selected && !ready {
            return Err(AppError::Validation(
                "局部生成前需先完成一次连续旁白整片配音，以建立可复用章节缓存".into(),
            ));
        }

        let (actual_ms, content_hash, new_artifact) = if ready {
            (
                crate::tts::duration_ms_of(&target)?,
                file_sha256(&target)?,
                false,
            )
        } else {
            let request = SynthesisRequest {
                text: text.clone(),
                voice_id: voice_id.into(),
                style: style.into(),
                instructions: instructions.clone(),
                speed,
                pitch: 1.0,
                volume: 1.0,
                sample_rate: 24_000,
                target_duration_ms: Some(chapter.end_ms - chapter.start_ms),
            };
            let normalized = tts_dir.join(format!(
                "chapter-{:02}.{}.pending.wav",
                chapter_index + 1,
                run_id
            ));
            let audio = match adapter
                .synthesize_realtime_session(&request, &text_chunks, &secret)
                .await
            {
                Ok(audio) => audio,
                Err(error) => {
                    let message = error.to_string();
                    for segment in &chapter.segments {
                        failed_segments.push(TtsSegmentFailure {
                            segment_id: segment.id.clone(),
                            message: message.clone(),
                        });
                        segment_updates.push(TtsSegmentPublication {
                            segment_id: segment.id.clone(),
                            expected_script_revision: segment.script_revision,
                            state: "failed".into(),
                            settings_hash: None,
                            duration_ms: None,
                            error_message: Some(format!("所在连续旁白章节生成失败：{message}")),
                            display_status: "warning".into(),
                        });
                    }
                    completed_chapters += usize::from(selected);
                    if selected {
                        update_tts_progress_kind(
                            app,
                            job_id,
                            completed_chapters,
                            selected_chapter_count,
                            "chapter",
                        );
                    }
                    continue;
                }
            };
            let temporary = tts_dir.join(format!(
                "chapter-{:02}.{}.raw.pending",
                chapter_index + 1,
                run_id
            ));
            std::fs::write(&temporary, &audio.bytes)
                .map_err(|error| AppError::Media(error.to_string()))?;
            let normalized_result = normalize_provider_audio(
                &temporary,
                &normalized,
                audio.encoding,
                audio.sample_rate,
            );
            let _ = std::fs::remove_file(&temporary);
            normalized_result?;
            let normalized_ms = crate::tts::duration_ms_of(&normalized)?;
            crate::tts::validate_clip_completeness(&request.text, normalized_ms)?;
            let fitted = tts_dir.join(format!(
                "chapter-{:02}.{}.pending.fit.wav",
                chapter_index + 1,
                run_id
            ));
            let fitted_ms = crate::tts::fit_narration_to_window(
                &normalized,
                &fitted,
                chapter.end_ms - chapter.start_ms,
            )?;
            let publish_source = if fitted.is_file() {
                fitted.as_path()
            } else {
                normalized.as_path()
            };
            let content_hash = file_sha256(publish_source)?;
            std::fs::rename(publish_source, &target)
                .map_err(|error| AppError::Media(format!("无法原子发布连续旁白章节：{error}")))?;
            if normalized.is_file() {
                let _ = std::fs::remove_file(&normalized);
            }
            (fitted_ms, content_hash, true)
        };
        let too_long = actual_ms > chapter.end_ms - chapter.start_ms + 150;
        if too_long {
            warning_ids.extend(chapter.segments.iter().map(|segment| segment.id.clone()));
        } else {
            inputs.push((
                target.clone(),
                crate::tts::positioned_start_ms(chapter.start_ms, chapter.end_ms, actual_ms),
            ));
        }
        if selected {
            for segment in &chapter.segments {
                segment_updates.push(TtsSegmentPublication {
                    segment_id: segment.id.clone(),
                    expected_script_revision: segment.script_revision,
                    state: if too_long { "stale" } else { "ready" }.into(),
                    settings_hash: (!too_long).then(|| settings_hash.clone()),
                    duration_ms: None,
                    error_message: too_long
                        .then(|| "所在连续旁白章节超过可用时间，请精简章节口播稿".into()),
                    display_status: if too_long { "warning" } else { "ready" }.into(),
                });
            }
            if new_artifact && !too_long {
                segment_artifacts.push(ArtifactRecord {
                    id: format!("tts-narration-{}-{}", first.id, &settings_hash[..16]), project_id: project.id.clone(), segment_id: None,
                    kind: "tts_narration_chapter".into(), path: target.to_string_lossy().into_owned(), content_hash,
                    dependency_hash: settings_hash.clone(), cache_key: Some(settings_hash.clone()), revision: 1, status: "ready".into(),
                    metadata_json: serde_json::json!({
                        "durationMs": actual_ms, "startMs": chapter.start_ms, "endMs": chapter.end_ms,
                        "segmentIds": chapter.segments.iter().map(|segment| &segment.id).collect::<Vec<_>>(),
                        "providerId": profile.id, "providerRevision": profile.revision, "voiceId": voice_id,
                        "style": style, "syncMode": "narration", "realtimeSession": true,
                        "wordTimings": crate::localization::estimate_word_timings(&text, actual_ms),
                    }).to_string(), created_at: String::new(), updated_at: String::new(),
                });
            }
            completed_chapters += 1;
            update_tts_progress_kind(
                app,
                job_id,
                completed_chapters,
                selected_chapter_count,
                "chapter",
            );
        }
    }

    if !warning_ids.is_empty() || !failed_segments.is_empty() {
        let database = state.database.lock().expect("database mutex poisoned");
        database.commit_tts_publication(
            job_id,
            publish_snapshot,
            &segment_updates,
            &segment_artifacts,
        )?;
        return Ok(TtsPipelineOutcome {
            warning_ids,
            failed_segments,
            affected_segment_ids,
            synthesis_unit_count: selected_chapter_count,
            cache_hit_unit_count,
        });
    }
    if inputs.len() != chapters.len() {
        return Err(AppError::Validation(
            "仍有连续旁白章节没有可用配音，未覆盖旧的中文音轨".into(),
        ));
    }
    publish_mixed_track(
        state,
        job_id,
        project,
        all_segments,
        publish_snapshot,
        artifact_dir,
        duration_ms,
        &run_id,
        &inputs,
        &segment_updates,
        &mut segment_artifacts,
    )?;
    Ok(TtsPipelineOutcome {
        warning_ids,
        failed_segments,
        affected_segment_ids,
        synthesis_unit_count: selected_chapter_count,
        cache_hit_unit_count,
    })
}

fn balanced_tts_blocks(segments: &[SegmentRecord]) -> Vec<BalancedTtsBlock<'_>> {
    let mut blocks = Vec::<BalancedTtsBlock<'_>>::new();
    for segment in segments
        .iter()
        .filter(|segment| !crate::localization::is_non_speech_text(&segment.source_text))
    {
        let should_split = blocks.last().is_some_and(|block| {
            let first = block.segments[0];
            let gap_ms = segment.start_ms - block.end_ms;
            let span_ms = block.end_ms - block.start_ms;
            let next_span_ms = segment.end_ms - block.start_ms;
            first.tts_overrides_json != segment.tts_overrides_json
                || gap_ms >= 1_200
                || block.segments.len() >= 6
                || span_ms >= 12_000
                || (span_ms >= 5_000 && next_span_ms > 15_000)
        });
        if should_split || blocks.is_empty() {
            blocks.push(BalancedTtsBlock {
                segments: vec![segment],
                start_ms: segment.start_ms,
                end_ms: segment.end_ms,
            });
        } else if let Some(block) = blocks.last_mut() {
            block.segments.push(segment);
            block.end_ms = segment.end_ms;
        }
    }
    blocks
}

#[allow(clippy::too_many_arguments)]
async fn synthesize_cloud_blocks(
    app: &AppHandle,
    state: &State<'_, AppState>,
    job_id: &str,
    project: &ProjectSummary,
    all_segments: &[SegmentRecord],
    selected_segments: &[SegmentRecord],
    profile: &ProviderProfile,
    raw_secret: &str,
    publish_snapshot: &TtsPublishSnapshot,
    artifact_dir: &std::path::Path,
    duration_ms: i64,
) -> Result<TtsPipelineOutcome, AppError> {
    use sha2::{Digest, Sha256};

    let secret = TtsSecretBundle::from_keychain_value(&profile.driver, raw_secret)?;
    if profile.driver == "iflytek_super_tts" || profile.driver == "iflytek" {
        let public = serde_json::from_str::<serde_json::Value>(&profile.public_config_json)
            .map_err(|_| AppError::Provider("讯飞语音配置无法解析".into()))?;
        secret.validate_public_app_id(public.get("appId").and_then(serde_json::Value::as_str))?;
    }
    let adapter = provider_adapter(profile)?;
    let tts_dir = artifact_dir.join("tts-v3");
    std::fs::create_dir_all(&tts_dir).map_err(|error| AppError::Media(error.to_string()))?;
    let config = serde_json::from_str::<serde_json::Value>(&profile.public_config_json)
        .map_err(|_| AppError::Provider("语音服务配置无法解析".into()))?;
    let default_voice = project
        .tts_voice_id
        .as_deref()
        .or_else(|| config.get("voice").and_then(serde_json::Value::as_str))
        .unwrap_or(if profile.driver == "iflytek_super_tts" {
            "x6_lingxiaoxuan_flow"
        } else {
            "Cherry"
        });
    let requested_ids = selected_segments
        .iter()
        .map(|segment| segment.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let blocks = balanced_tts_blocks(all_segments);
    let selected_block_count = blocks
        .iter()
        .filter(|block| {
            block
                .segments
                .iter()
                .any(|segment| requested_ids.contains(segment.id.as_str()))
        })
        .count();
    let affected_segment_ids = blocks
        .iter()
        .filter(|block| {
            block
                .segments
                .iter()
                .any(|segment| requested_ids.contains(segment.id.as_str()))
        })
        .flat_map(|block| block.segments.iter().map(|segment| segment.id.clone()))
        .collect::<Vec<_>>();
    let mut warning_ids = Vec::new();
    let mut failed_segments = Vec::<TtsSegmentFailure>::new();
    let mut segment_updates = Vec::<TtsSegmentPublication>::new();
    let mut segment_artifacts = Vec::<ArtifactRecord>::new();
    let mut inputs = Vec::<(PathBuf, i64)>::new();
    let run_id = Uuid::new_v4().to_string();
    let mut completed_blocks = 0;
    let mut cache_hit_unit_count = 0;
    let mut missing_unselected_blocks = 0;
    let effective_sync_mode = if project.tts_sync_mode == "semantic" {
        "semantic"
    } else {
        "balanced"
    };

    for block in &blocks {
        let selected = block
            .segments
            .iter()
            .any(|segment| requested_ids.contains(segment.id.as_str()));
        let first = block.segments[0];
        let overrides: TtsOverrides =
            serde_json::from_str(&first.tts_overrides_json).unwrap_or(TtsOverrides {
                voice_id: None,
                style: None,
                speed: None,
                director_enabled: None,
            });
        let voice_id = overrides.voice_id.as_deref().unwrap_or(default_voice);
        let style = overrides.style.as_deref().unwrap_or(&project.tts_style);
        let speed = overrides.speed.unwrap_or(1.0).clamp(0.8, 1.08);
        let director_enabled = overrides
            .director_enabled
            .unwrap_or(project.tts_director_enabled);
        let documents = block
            .segments
            .iter()
            .map(|segment| {
                ScriptDocumentV1::parse_or_fallback(
                    Some(&segment.script_doc_json),
                    &segment.spoken_zh,
                )
            })
            .collect::<Vec<_>>();
        for document in &documents {
            document.validate()?;
        }
        let text = documents
            .iter()
            .map(|document| {
                if profile.driver == "iflytek_super_tts" {
                    document.render_iflytek_text()
                } else {
                    document.render_directed_text()
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        if text.trim().is_empty() {
            return Err(AppError::Validation(
                "语音块中仍有空白口播稿，请先完成翻译".into(),
            ));
        }
        let instructions = (director_enabled && matches!(profile.driver.as_str(), "aliyun_tts" | "bailian_tts")).then(|| {
            let base = documents[0].director_instruction(style);
            format!("{base}。这是一个连续讲解语音块：句间自然承接，保持相同气息、音量和说话状态，不要在每句开头重新起范；严格只朗读正文。")
        });
        let settings_json = serde_json::json!({
            "provider": profile.id, "providerRevision": profile.revision, "driver": profile.driver,
            "voice": voice_id, "style": style, "speed": speed, "sampleRate": 24000,
            "segments": block.segments.iter().map(|segment| serde_json::json!({
                "id": segment.id, "scriptRevision": segment.script_revision, "spokenZh": segment.spoken_zh,
            })).collect::<Vec<_>>(),
            "directorEnabled": director_enabled, "instructions": instructions,
            "syncMode": effective_sync_mode, "blockPolicyVersion": 1,
            "semanticRewriteVersion": (effective_sync_mode == "semantic").then_some(1),
            "audioPipelineVersion": 4,
        });
        let settings_hash = hex::encode(Sha256::digest(
            serde_json::to_vec(&settings_json)
                .map_err(|error| AppError::Validation(error.to_string()))?,
        ));
        let target = tts_dir.join(format!("block-{}-{}.wav", first.id, &settings_hash[..32]));
        let target_ready = target.metadata().is_ok_and(|metadata| metadata.len() > 44);
        let ready = target_ready;
        cache_hit_unit_count += usize::from(selected && ready);
        if !selected && !ready {
            missing_unselected_blocks += 1;
            continue;
        }

        let update_start = segment_updates.len();
        let (actual_ms, content_hash, new_artifact) = if ready {
            (
                crate::tts::duration_ms_of(&target)?,
                file_sha256(&target)?,
                false,
            )
        } else {
            let request = SynthesisRequest {
                text,
                voice_id: voice_id.into(),
                style: style.into(),
                instructions: instructions.clone(),
                speed,
                pitch: 1.0,
                volume: 1.0,
                sample_rate: 24_000,
                target_duration_ms: Some(block.end_ms - block.start_ms),
            };
            let normalized = tts_dir.join(format!("block-{}.{}.pending.wav", first.id, run_id));
            let mut synthesis_error = None;
            let mut complete = false;
            for attempt in 0..2 {
                let audio = match adapter.synthesize(&request, &secret).await {
                    Ok(audio) => audio,
                    Err(error) => {
                        if let Some(fatal) = non_retryable_tts_error(&error) {
                            return Err(fatal);
                        }
                        synthesis_error = Some(error);
                        break;
                    }
                };
                let temporary = tts_dir.join(format!(
                    "block-{}.{}.{}.raw.pending",
                    first.id, run_id, attempt
                ));
                std::fs::write(&temporary, &audio.bytes)
                    .map_err(|error| AppError::Media(error.to_string()))?;
                let normalized_result = normalize_provider_audio(
                    &temporary,
                    &normalized,
                    audio.encoding,
                    audio.sample_rate,
                );
                let _ = std::fs::remove_file(&temporary);
                match normalized_result.and_then(|_| {
                    let duration = crate::tts::duration_ms_of(&normalized)?;
                    crate::tts::validate_clip_completeness(&request.text, duration)
                }) {
                    Ok(()) => {
                        complete = true;
                        break;
                    }
                    Err(error) => {
                        synthesis_error = Some(error);
                        let _ = std::fs::remove_file(&normalized);
                    }
                }
            }
            if !complete {
                let message = synthesis_error
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| "语音服务没有返回完整音频".into());
                for segment in &block.segments {
                    failed_segments.push(TtsSegmentFailure {
                        segment_id: segment.id.clone(),
                        message: message.clone(),
                    });
                    segment_updates.push(TtsSegmentPublication {
                        segment_id: segment.id.clone(),
                        expected_script_revision: segment.script_revision,
                        state: "failed".into(),
                        settings_hash: None,
                        duration_ms: None,
                        error_message: Some(format!("所在语音块生成失败：{message}")),
                        display_status: "warning".into(),
                    });
                }
                {
                    let database = state.database.lock().expect("database mutex poisoned");
                    database.commit_tts_publication(
                        job_id,
                        publish_snapshot,
                        &segment_updates[update_start..],
                        &[],
                    )?;
                }
                completed_blocks += usize::from(selected);
                if selected {
                    update_tts_progress_kind(
                        app,
                        job_id,
                        completed_blocks,
                        selected_block_count,
                        if effective_sync_mode == "semantic" {
                            "scene"
                        } else {
                            "chapter"
                        },
                    );
                }
                continue;
            }
            let fitted = tts_dir.join(format!("block-{}.{}.pending.fit.wav", first.id, run_id));
            let fitted_ms = crate::tts::fit_clip_to_window(
                &normalized,
                &fitted,
                block.end_ms - block.start_ms,
            )?;
            let publish_source = if fitted.is_file() {
                fitted.as_path()
            } else {
                normalized.as_path()
            };
            let content_hash = file_sha256(publish_source)?;
            std::fs::rename(publish_source, &target)
                .map_err(|error| AppError::Media(format!("无法原子发布语音块：{error}")))?;
            if normalized.is_file() {
                let _ = std::fs::remove_file(&normalized);
            }
            (fitted_ms, content_hash, true)
        };
        // fit_clip_to_window validates the synthesized block against its shared
        // 5–15 second visual window. In semantic mode the individual subtitle
        // rows are only edit/navigation handles. An overlong block must still
        // become a visible warning: publishing it would overlap the next block
        // and produce two simultaneous Chinese voices.
        let too_long = actual_ms > block.end_ms - block.start_ms + 20;
        if too_long {
            warning_ids.extend(block.segments.iter().map(|segment| segment.id.clone()));
        } else {
            inputs.push((
                target.clone(),
                crate::tts::positioned_start_ms(block.start_ms, block.end_ms, actual_ms),
            ));
        }
        if selected {
            for segment in &block.segments {
                segment_updates.push(TtsSegmentPublication {
                    segment_id: segment.id.clone(),
                    expected_script_revision: segment.script_revision,
                    state: if too_long { "stale" } else { "ready" }.into(),
                    settings_hash: (!too_long).then(|| settings_hash.clone()),
                    duration_ms: None,
                    error_message: too_long
                        .then(|| "所在语音块超过可用时间，请缩短口播稿或拆分语音块".into()),
                    display_status: if too_long { "warning" } else { "ready" }.into(),
                });
            }
            let block_artifact = (new_artifact && !too_long).then(|| ArtifactRecord {
                    id: format!("tts-block-{}-{}", first.id, &settings_hash[..16]), project_id: project.id.clone(), segment_id: None,
                    kind: if effective_sync_mode == "semantic" { "tts_semantic_anchored" } else { "tts_block_aligned" }.into(), path: target.to_string_lossy().into_owned(), content_hash,
                    dependency_hash: settings_hash.clone(), cache_key: Some(settings_hash.clone()), revision: 1, status: "ready".into(),
                    metadata_json: serde_json::json!({
                        "durationMs": actual_ms, "startMs": block.start_ms, "endMs": block.end_ms,
                        "segmentIds": block.segments.iter().map(|segment| &segment.id).collect::<Vec<_>>(),
                        "providerId": profile.id, "providerRevision": profile.revision, "voiceId": voice_id, "style": style,
                        "syncMode": effective_sync_mode,
                        "semanticRewrite": effective_sync_mode == "semantic",
                    }).to_string(), created_at: String::new(), updated_at: String::new(),
                });
            {
                let database = state.database.lock().expect("database mutex poisoned");
                let artifacts = block_artifact.as_slice();
                database.commit_tts_publication(
                    job_id,
                    publish_snapshot,
                    &segment_updates[update_start..],
                    artifacts,
                )?;
            }
            completed_blocks += 1;
            update_tts_progress_kind(
                app,
                job_id,
                completed_blocks,
                selected_block_count,
                if effective_sync_mode == "semantic" {
                    "scene"
                } else {
                    "chapter"
                },
            );
        }
    }

    if !warning_ids.is_empty() || !failed_segments.is_empty() {
        return Ok(TtsPipelineOutcome {
            warning_ids,
            failed_segments,
            affected_segment_ids,
            synthesis_unit_count: selected_block_count,
            cache_hit_unit_count,
        });
    }
    if missing_unselected_blocks > 0 {
        return Ok(TtsPipelineOutcome {
            warning_ids,
            failed_segments,
            affected_segment_ids,
            synthesis_unit_count: selected_block_count,
            cache_hit_unit_count,
        });
    }
    if inputs.len() != blocks.len() {
        return Err(AppError::Validation(
            "仍有语音块没有可用配音，未覆盖旧的中文音轨".into(),
        ));
    }
    crate::tts::validate_non_overlapping_inputs(&inputs)?;
    publish_mixed_track(
        state,
        job_id,
        project,
        all_segments,
        publish_snapshot,
        artifact_dir,
        duration_ms,
        &run_id,
        &inputs,
        &segment_updates,
        &mut segment_artifacts,
    )?;
    Ok(TtsPipelineOutcome {
        warning_ids,
        failed_segments,
        affected_segment_ids,
        synthesis_unit_count: selected_block_count,
        cache_hit_unit_count,
    })
}

#[allow(clippy::too_many_arguments)]
async fn synthesize_cloud_segments(
    app: &AppHandle,
    state: &State<'_, AppState>,
    job_id: &str,
    project: &ProjectSummary,
    all_segments: &[SegmentRecord],
    selected_segments: &[SegmentRecord],
    profile: &ProviderProfile,
    raw_secret: &str,
    publish_snapshot: &TtsPublishSnapshot,
    artifact_dir: &std::path::Path,
    duration_ms: i64,
) -> Result<TtsPipelineOutcome, AppError> {
    use sha2::{Digest, Sha256};

    let secret = TtsSecretBundle::from_keychain_value(&profile.driver, raw_secret)?;
    if profile.driver == "iflytek_super_tts" || profile.driver == "iflytek" {
        let public = serde_json::from_str::<serde_json::Value>(&profile.public_config_json)
            .map_err(|_| AppError::Provider("讯飞语音配置无法解析".into()))?;
        secret.validate_public_app_id(public.get("appId").and_then(serde_json::Value::as_str))?;
    }
    let adapter = provider_adapter(profile)?;
    let tts_dir = artifact_dir.join("tts-v3");
    std::fs::create_dir_all(&tts_dir).map_err(|error| AppError::Media(error.to_string()))?;
    let config = serde_json::from_str::<serde_json::Value>(&profile.public_config_json)
        .map_err(|_| AppError::Provider("语音服务配置无法解析".into()))?;
    let default_voice = project
        .tts_voice_id
        .as_deref()
        .or_else(|| config.get("voice").and_then(serde_json::Value::as_str))
        .unwrap_or(if profile.driver == "iflytek_super_tts" {
            "x6_lingxiaoxuan_flow"
        } else {
            "Cherry"
        });
    let selected_ids = selected_segments
        .iter()
        .map(|segment| segment.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut warning_ids = Vec::new();
    let mut selected_clips = std::collections::HashMap::<String, PathBuf>::new();
    let mut segment_updates = Vec::<TtsSegmentPublication>::new();
    let mut segment_artifacts = Vec::<ArtifactRecord>::new();
    let mut failed_segments = Vec::<TtsSegmentFailure>::new();
    let mut cache_hit_unit_count = 0;
    let run_id = Uuid::new_v4().to_string();

    for (index, segment) in selected_segments.iter().enumerate() {
        let document =
            ScriptDocumentV1::parse_or_fallback(Some(&segment.script_doc_json), &segment.spoken_zh);
        document.validate()?;
        let overrides: TtsOverrides =
            serde_json::from_str(&segment.tts_overrides_json).unwrap_or(TtsOverrides {
                voice_id: None,
                style: None,
                speed: None,
                director_enabled: None,
            });
        let voice_id = overrides.voice_id.as_deref().unwrap_or(default_voice);
        let style = overrides.style.as_deref().unwrap_or(&project.tts_style);
        let speed = overrides.speed.unwrap_or(1.0).clamp(0.8, 1.08);
        let text = if profile.driver == "iflytek_super_tts" {
            document.render_iflytek_text()
        } else {
            document.render_directed_text()
        };
        let director_enabled = overrides
            .director_enabled
            .unwrap_or(project.tts_director_enabled);
        let instructions = (director_enabled
            && (profile.driver == "aliyun_tts" || profile.driver == "bailian_tts"))
            .then(|| {
                let base = document.director_instruction(style);
                format!(
                    "{base}。{}",
                    continuity_instruction(all_segments, segment.ordinal)
                )
            });
        let settings_json = serde_json::json!({
            "provider": profile.id, "providerRevision": profile.revision, "driver": profile.driver,
            "voice": voice_id, "style": style, "speed": speed, "sampleRate": 24000,
            "scriptRevision": segment.script_revision, "text": text,
            "directorEnabled": director_enabled, "instructions": instructions,
            "audioPipelineVersion": 2,
        });
        let settings_hash = hex::encode(Sha256::digest(
            serde_json::to_vec(&settings_json)
                .map_err(|error| AppError::Validation(error.to_string()))?,
        ));
        let target = tts_dir.join(format!("{}-{}.wav", segment.id, &settings_hash[..32]));
        let ready = target.metadata().is_ok_and(|metadata| metadata.len() > 44);
        cache_hit_unit_count += usize::from(ready);
        let (actual_ms, content_hash, new_artifact) = if ready {
            (
                crate::tts::duration_ms_of(&target)?,
                file_sha256(&target)?,
                false,
            )
        } else {
            let request = SynthesisRequest {
                text,
                voice_id: voice_id.into(),
                style: style.into(),
                instructions: instructions.clone(),
                speed,
                pitch: 1.0,
                volume: 1.0,
                sample_rate: 24_000,
                target_duration_ms: Some(segment.end_ms - segment.start_ms),
            };
            let normalized = tts_dir.join(format!("{}.{}.pending.wav", segment.id, run_id));
            let mut synthesis_error = None;
            let mut complete = false;
            for attempt in 0..2 {
                let audio = match adapter.synthesize(&request, &secret).await {
                    Ok(audio) => audio,
                    Err(error) => {
                        synthesis_error = Some(error);
                        break;
                    }
                };
                let temporary =
                    tts_dir.join(format!("{}.{}.{}.raw.pending", segment.id, run_id, attempt));
                std::fs::write(&temporary, &audio.bytes)
                    .map_err(|error| AppError::Media(error.to_string()))?;
                let normalized_result = normalize_provider_audio(
                    &temporary,
                    &normalized,
                    audio.encoding,
                    audio.sample_rate,
                );
                let _ = std::fs::remove_file(&temporary);
                match normalized_result.and_then(|_| {
                    let duration = crate::tts::duration_ms_of(&normalized)?;
                    crate::tts::validate_clip_completeness(&request.text, duration)
                }) {
                    Ok(()) => {
                        complete = true;
                        break;
                    }
                    Err(error) => {
                        synthesis_error = Some(error);
                        let _ = std::fs::remove_file(&normalized);
                    }
                }
            }
            if !complete {
                let message = synthesis_error
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| "语音服务没有返回完整音频".into());
                failed_segments.push(TtsSegmentFailure {
                    segment_id: segment.id.clone(),
                    message: message.clone(),
                });
                segment_updates.push(TtsSegmentPublication {
                    segment_id: segment.id.clone(),
                    expected_script_revision: segment.script_revision,
                    state: "failed".into(),
                    settings_hash: None,
                    duration_ms: None,
                    error_message: Some(message),
                    display_status: "warning".into(),
                });
                update_tts_progress(app, job_id, index + 1, selected_segments.len());
                continue;
            }
            let target_window_ms = segment.end_ms - segment.start_ms;
            let fitted = tts_dir.join(format!("{}.{}.pending.fit.wav", segment.id, run_id));
            let fitted_ms = crate::tts::fit_clip_to_window(&normalized, &fitted, target_window_ms)?;
            let (mut publish_source, mut actual_ms) = if fitted.is_file() {
                (fitted.as_path(), fitted_ms)
            } else {
                (normalized.as_path(), fitted_ms)
            };
            let forced = tts_dir.join(format!("{}.{}.pending.force-fit.wav", segment.id, run_id));
            let fallback_tempo = actual_ms as f64 / target_window_ms.max(1) as f64;
            if actual_ms > target_window_ms + 150
                && request.text.chars().count() <= 32
                && fallback_tempo <= crate::tts::MAX_SHORT_CLIP_TEMPO
            {
                actual_ms = crate::tts::force_fit_clip_to_window(
                    publish_source,
                    &forced,
                    target_window_ms,
                )?;
                publish_source = forced.as_path();
            }
            let content_hash = file_sha256(publish_source)?;
            std::fs::rename(publish_source, &target)
                .map_err(|error| AppError::Media(format!("无法原子发布片段配音：{error}")))?;
            if normalized.is_file() {
                let _ = std::fs::remove_file(&normalized);
            }
            (actual_ms, content_hash, true)
        };
        let too_long = actual_ms > segment.end_ms - segment.start_ms + 150;
        if too_long {
            warning_ids.push(segment.id.clone());
        } else {
            selected_clips.insert(segment.id.clone(), target.clone());
        }
        segment_updates.push(TtsSegmentPublication {
            segment_id: segment.id.clone(),
            expected_script_revision: segment.script_revision,
            state: if too_long { "stale" } else { "ready" }.into(),
            settings_hash: (!too_long).then(|| settings_hash.clone()),
            duration_ms: (!too_long).then_some(actual_ms),
            error_message: too_long.then(|| "合成音频超过片段时长，请压缩口播稿或降低停顿".into()),
            display_status: if too_long { "warning" } else { "ready" }.into(),
        });
        if new_artifact && !too_long {
            segment_artifacts.push(ArtifactRecord {
                id: format!("tts-{}-{}", segment.id, &settings_hash[..16]),
                project_id: project.id.clone(),
                segment_id: Some(segment.id.clone()),
                kind: "tts_aligned".into(),
                path: target.to_string_lossy().into_owned(),
                content_hash,
                dependency_hash: settings_hash.clone(),
                cache_key: Some(settings_hash.clone()),
                revision: 1,
                status: "ready".into(),
                metadata_json: serde_json::json!({
                    "durationMs": actual_ms,
                    "providerId": profile.id,
                    "providerRevision": profile.revision,
                    "voiceId": voice_id,
                    "style": style,
                })
                .to_string(),
                created_at: String::new(),
                updated_at: String::new(),
            });
        }
        update_tts_progress(app, job_id, index + 1, selected_segments.len());
    }

    let mut inputs = Vec::new();
    for segment in all_segments {
        let clip = if let Some(path) = selected_clips.get(&segment.id) {
            Some(path.clone())
        } else if segment.tts_state == "ready" {
            segment
                .tts_settings_hash
                .as_deref()
                .map(|hash| {
                    tts_dir.join(format!(
                        "{}-{}.wav",
                        segment.id,
                        &hash[..32.min(hash.len())]
                    ))
                })
                .filter(|path| path.metadata().is_ok_and(|metadata| metadata.len() > 44))
        } else {
            None
        };
        if let Some(path) = clip {
            let duration = crate::tts::duration_ms_of(&path)?;
            inputs.push((
                path,
                crate::tts::positioned_start_ms(segment.start_ms, segment.end_ms, duration),
            ));
        }
    }
    if !warning_ids.is_empty() || !failed_segments.is_empty() {
        // A clip that exceeds its slot is deliberately not mixed: otherwise it
        // bleeds into the next segment. Keep the previous full track intact and
        // publish only the warning state so the user can fit/retry that clip.
        let database = state.database.lock().expect("database mutex poisoned");
        database.commit_tts_publication(
            job_id,
            publish_snapshot,
            &segment_updates,
            &segment_artifacts,
        )?;
        return Ok(TtsPipelineOutcome {
            warning_ids,
            failed_segments,
            affected_segment_ids: selected_segments
                .iter()
                .map(|segment| segment.id.clone())
                .collect(),
            synthesis_unit_count: selected_segments.len(),
            cache_hit_unit_count,
        });
    }
    if inputs.len() < all_segments.len() && selected_ids.len() != all_segments.len() {
        return Err(AppError::Validation(
            "局部生成前需先完成一次全片配音，以建立可复用片段缓存".into(),
        ));
    }
    if inputs.len() != all_segments.len() {
        return Err(AppError::Validation(
            "仍有片段没有可用配音，未覆盖旧的中文音轨".into(),
        ));
    }
    publish_mixed_track(
        state,
        job_id,
        project,
        all_segments,
        publish_snapshot,
        artifact_dir,
        duration_ms,
        &run_id,
        &inputs,
        &segment_updates,
        &mut segment_artifacts,
    )?;
    Ok(TtsPipelineOutcome {
        warning_ids,
        failed_segments,
        affected_segment_ids: selected_segments
            .iter()
            .map(|segment| segment.id.clone())
            .collect(),
        synthesis_unit_count: selected_segments.len(),
        cache_hit_unit_count,
    })
}

#[allow(clippy::too_many_arguments)]
fn publish_mixed_track(
    state: &State<'_, AppState>,
    job_id: &str,
    project: &ProjectSummary,
    all_segments: &[SegmentRecord],
    publish_snapshot: &TtsPublishSnapshot,
    artifact_dir: &std::path::Path,
    duration_ms: i64,
    _run_id: &str,
    inputs: &[(PathBuf, i64)],
    segment_updates: &[TtsSegmentPublication],
    segment_artifacts: &mut Vec<ArtifactRecord>,
) -> Result<(), AppError> {
    let final_track = artifact_dir.join("chinese-voice.wav");
    let pending_track =
        crate::infrastructure::artifact_publisher::AtomicArtifactPublisher::stage_file(
            &final_track,
        )?;
    crate::tts::mix_track(inputs, duration_ms, &pending_track)?;
    let mut published_segments = all_segments.to_vec();
    for update in segment_updates {
        let segment = published_segments
            .iter_mut()
            .find(|segment| segment.id == update.segment_id)
            .expect("selected segment belongs to snapshot");
        segment.tts_state = update.state.clone();
        segment.tts_settings_hash.clone_from(&update.settings_hash);
        segment.tts_duration_ms = update.duration_ms;
        segment.tts_error_message.clone_from(&update.error_message);
    }
    let mix_dependency = tts_mix_dependency(project, &published_segments)?;
    let content_hash = file_sha256(&pending_track)?;
    segment_artifacts.push(ArtifactRecord {
        id: format!("tts-mix-{}", project.id),
        project_id: project.id.clone(),
        segment_id: None,
        kind: "tts_mix".into(),
        path: final_track.to_string_lossy().into_owned(),
        content_hash,
        dependency_hash: mix_dependency.clone(),
        cache_key: Some(mix_dependency),
        revision: 1,
        status: "ready".into(),
        metadata_json: serde_json::json!({ "durationMs": duration_ms }).to_string(),
        created_at: String::new(),
        updated_at: String::new(),
    });
    let database = state.database.lock().expect("database mutex poisoned");
    database.validate_tts_publish_snapshot(job_id, publish_snapshot)?;
    crate::infrastructure::artifact_publisher::AtomicArtifactPublisher::publish_file(
        &pending_track,
        &final_track,
        || {
            database.commit_tts_publication(
                job_id,
                publish_snapshot,
                segment_updates,
                segment_artifacts,
            )
        },
    )
}

fn file_sha256(path: &std::path::Path) -> Result<String, AppError> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|error| AppError::Media(error.to_string()))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| AppError::Media(error.to_string()))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(hex::encode(digest.finalize()))
}

fn tts_mix_dependency(
    project: &ProjectSummary,
    segments: &[SegmentRecord],
) -> Result<String, AppError> {
    use sha2::{Digest, Sha256};

    let value = serde_json::json!({
        "projectId": project.id,
        "providerId": project.tts_provider_id,
        "projectTtsRevision": project.tts_settings_revision,
        "mixPolicyVersion": 2,
        "segments": segments.iter().map(|segment| serde_json::json!({
            "id": segment.id,
            "ordinal": segment.ordinal,
            "startMs": segment.start_ms,
            "endMs": segment.end_ms,
            "scriptRevision": segment.script_revision,
            "ttsSettingsHash": segment.tts_settings_hash,
        })).collect::<Vec<_>>(),
    });
    let bytes =
        serde_json::to_vec(&value).map_err(|error| AppError::Validation(error.to_string()))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn system_tts_settings_hash(project: &ProjectSummary, segment: &SegmentRecord) -> String {
    use sha2::{Digest, Sha256};

    let value = serde_json::json!({
        "provider": "system",
        "voice": project.tts_voice_id.as_deref().unwrap_or("Tingting"),
        "style": project.tts_style,
        "settings": project.tts_settings_json,
        "overrides": segment.tts_overrides_json,
        "rate": 200,
        "segmentId": segment.id,
        "scriptRevision": segment.script_revision,
        "spokenZh": segment.spoken_zh,
        "audioPipelineVersion": 2,
    });
    hex::encode(Sha256::digest(
        serde_json::to_vec(&value).expect("system TTS settings are serializable"),
    ))
}

fn continuity_instruction(segments: &[SegmentRecord], ordinal: i64) -> String {
    let position = segments
        .iter()
        .position(|segment| segment.ordinal == ordinal);
    let previous = position
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| segments.get(index))
        .map(|segment| compact_context(&segment.spoken_zh));
    let next = position
        .and_then(|index| segments.get(index + 1))
        .map(|segment| compact_context(&segment.spoken_zh));
    match (previous, next) {
        (Some(previous), Some(next)) => format!(
            "这是连续讲解中的一句：语气承接上句“{previous}”，自然引向下句“{next}”；上下文只用于演绎，严格只朗读当前正文"
        ),
        (None, Some(next)) => format!(
            "这是连续讲解的开场，自然引向下句“{next}”；上下文只用于演绎，严格只朗读当前正文"
        ),
        (Some(previous), None) => format!(
            "这是连续讲解的收束，语气承接上句“{previous}”；上下文只用于演绎，严格只朗读当前正文"
        ),
        (None, None) => "保持完整自然的一段技术讲解；严格只朗读当前正文".into(),
    }
}

fn compact_context(text: &str) -> String {
    let compact = text.split_whitespace().collect::<String>();
    let mut output = compact.chars().take(34).collect::<String>();
    if compact.chars().count() > 34 {
        output.push('…');
    }
    output
}

fn system_segment_cache_path(
    tts_dir: &std::path::Path,
    segment: &SegmentRecord,
    settings_hash: &str,
) -> PathBuf {
    tts_dir.join(format!("{}-{}.wav", segment.id, &settings_hash[..32]))
}

fn reusable_system_clip(
    tts_dir: &std::path::Path,
    project: &ProjectSummary,
    segment: &SegmentRecord,
) -> Option<PathBuf> {
    let settings_hash = system_tts_settings_hash(project, segment);
    let target = system_segment_cache_path(tts_dir, segment, &settings_hash);
    target
        .metadata()
        .is_ok_and(|metadata| metadata.len() > 44)
        .then_some(target)
}

fn validate_system_partial_cache(
    project: &ProjectSummary,
    all_segments: &[SegmentRecord],
    selected_ids: &std::collections::HashSet<&str>,
    tts_dir: &std::path::Path,
) -> Result<(), AppError> {
    let missing = all_segments
        .iter()
        .filter(|segment| !selected_ids.contains(segment.id.as_str()))
        .filter(|segment| reusable_system_clip(tts_dir, project, segment).is_none())
        .map(|segment| segment.id.as_str())
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    Err(AppError::Validation(format!(
        "局部生成前需先完成一次全片配音，以建立可复用的系统语音缓存；当前缺少片段：{}",
        missing.join("、")
    )))
}

fn validate_tts_mix_for_export(
    project: &ProjectSummary,
    segments: &[SegmentRecord],
    artifact: &ArtifactRecord,
) -> Result<PathBuf, AppError> {
    let directory = project
        .artifact_dir
        .as_ref()
        .ok_or_else(|| AppError::Validation("请先完成当前版本的中文配音".into()))?;
    let expected_path = PathBuf::from(directory).join("chinese-voice.wav");
    let dependency = tts_mix_dependency(project, segments)?;
    if artifact.project_id != project.id
        || artifact.kind != "tts_mix"
        || artifact.status != "ready"
        || artifact.dependency_hash != dependency
        || artifact.cache_key.as_deref() != Some(dependency.as_str())
        || std::path::Path::new(&artifact.path) != expected_path
        || !expected_path.is_file()
    {
        return Err(AppError::Validation(
            "中文音轨不是当前口播稿版本，请重新生成后导出".into(),
        ));
    }
    if file_sha256(&expected_path)? != artifact.content_hash {
        return Err(AppError::Validation(
            "中文音轨文件已改变或不完整，请重新生成后导出".into(),
        ));
    }
    Ok(expected_path)
}

fn normalize_provider_audio(
    source: &std::path::Path,
    target: &std::path::Path,
    encoding: AudioEncoding,
    sample_rate: u32,
) -> Result<(), AppError> {
    let mut command = std::process::Command::new(crate::tts::resolve_ffmpeg());
    command.args(["-hide_banner", "-loglevel", "error", "-y"]);
    if encoding == AudioEncoding::PcmS16Le {
        command.args(["-f", "s16le", "-ar", &sample_rate.to_string(), "-ac", "1"]);
    }
    command
        .arg("-i")
        .arg(source)
        .args([
            "-af",
            crate::tts::safe_edge_trim_filter(),
            "-ac",
            "1",
            "-ar",
            "48000",
            "-c:a",
            "pcm_s16le",
            "-f",
            "wav",
        ])
        .arg(target);
    crate::tts::run(&mut command, "在线配音音频标准化失败")
}

fn file_revision(path: &std::path::Path) -> u64 {
    path.metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or_default()
}

#[tauri::command]
pub async fn tts_audition(
    _app: AppHandle,
    state: State<'_, AppState>,
    request: TtsAuditionRequest,
) -> Result<TtsPreviewAudio, AppError> {
    use sha2::{Digest, Sha256};

    request.document.validate()?;
    let (project, segment, profile, raw_secret) = {
        let database = state.database.lock().expect("database mutex poisoned");
        let segment = database.get_segment(&request.segment_id)?;
        if segment.script_revision != request.script_revision {
            return Err(AppError::Validation("口播稿已变化，请重新试听".into()));
        }
        let project = database.get_project(&segment.project_id)?;
        let provider_id = request
            .provider_id
            .as_deref()
            .unwrap_or(&project.tts_provider_id);
        if provider_id == "system" {
            (project, segment, None, None)
        } else {
            let profile = database.get_provider(provider_id)?;
            let reference = profile
                .secret_bundle_ref
                .as_ref()
                .or(profile.credential_ref.as_ref())
                .ok_or_else(|| AppError::Provider("请先配置中文语音 API Key".into()))?;
            let secret = credentials::get(reference)?;
            (project, segment, Some(profile), Some(secret))
        }
    };
    let provider_id = request
        .provider_id
        .as_deref()
        .unwrap_or(&project.tts_provider_id);
    let voice = request
        .voice_id
        .as_deref()
        .or(project.tts_voice_id.as_deref())
        .unwrap_or("Tingting");
    let style = request.style.as_deref().unwrap_or(&project.tts_style);
    let speed = request.speed.unwrap_or(1.0).clamp(0.8, 1.08);
    let driver = profile
        .as_ref()
        .map_or("system", |value| value.driver.as_str());
    let text = if driver == "iflytek_super_tts" {
        request.document.render_iflytek_text()
    } else {
        request.document.render_directed_text()
    };
    let hash_input = serde_json::json!({ "provider": provider_id, "revision": profile.as_ref().map_or(1, |value| value.revision), "voice": voice, "style": style, "speed": speed, "document": request.document, "audioPipelineVersion": 2 });
    let cache_key = hex::encode(Sha256::digest(
        serde_json::to_vec(&hash_input).map_err(|error| AppError::Validation(error.to_string()))?,
    ));
    let root = PathBuf::from(
        project
            .artifact_dir
            .as_ref()
            .ok_or_else(|| AppError::Media("请先完成媒体准备再试听配音".into()))?,
    )
    .join("tts-auditions");
    std::fs::create_dir_all(&root).map_err(|error| AppError::Media(error.to_string()))?;
    let target = root.join(format!("{}.wav", &cache_key[..32]));
    let cache_hit = target.metadata().is_ok_and(|metadata| metadata.len() > 44);
    if !cache_hit {
        let synthesis = SynthesisRequest {
            text,
            voice_id: voice.into(),
            style: style.into(),
            instructions: (driver == "aliyun_tts")
                .then(|| request.document.director_instruction(style)),
            speed,
            pitch: 1.0,
            volume: 1.0,
            sample_rate: 24_000,
            target_duration_ms: Some(segment.end_ms - segment.start_ms),
        };
        let secret = if let Some(raw) = raw_secret.as_deref() {
            TtsSecretBundle::from_keychain_value(driver, raw)?
        } else {
            TtsSecretBundle::local()
        };
        let audio = if let Some(profile) = profile.as_ref() {
            provider_adapter(profile)?
                .synthesize(&synthesis, &secret)
                .await?
        } else {
            crate::tts_provider::SystemTtsAdapter
                .synthesize(&synthesis, &secret)
                .await?
        };
        let temporary = target.with_extension("pending");
        std::fs::write(&temporary, audio.bytes)
            .map_err(|error| AppError::Media(error.to_string()))?;
        normalize_provider_audio(&temporary, &target, audio.encoding, audio.sample_rate)?;
        let _ = std::fs::remove_file(temporary);
    }
    Ok(TtsPreviewAudio {
        request_id: Uuid::new_v4().to_string(),
        path: target.to_string_lossy().into_owned(),
        revision: file_revision(&target),
        duration_ms: crate::tts::duration_ms_of(&target)?,
        cache_hit,
    })
}

#[tauri::command]
pub fn tts_audition_cancel(_request_id: String) {
    // Auditions are short, bounded requests. The response is revision-guarded;
    // a future streaming cancellation registry can terminate the provider call.
}

#[tauri::command]
pub async fn tts_fit_warnings(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    job_id: String,
    segment_ids: Option<Vec<String>>,
) -> Result<TtsFitResult, AppError> {
    let (project, mut warnings, profile) = {
        let database = state.database.lock().expect("database mutex poisoned");
        let project = database.get_project(&project_id)?;
        let selected = segment_ids
            .as_ref()
            .map(|ids| ids.iter().collect::<std::collections::HashSet<_>>());
        let warnings = database
            .list_segments(&project_id)?
            .into_iter()
            .filter(|segment| segment.status == "warning" && segment.tts_state != "failed")
            .filter(|segment| {
                selected
                    .as_ref()
                    .is_none_or(|ids| ids.contains(&segment.id))
            })
            .collect::<Vec<_>>();
        let provider_id = project
            .translation_provider_id
            .clone()
            .ok_or_else(|| AppError::Provider("项目没有翻译服务配置".into()))?;
        let profile = database.get_provider(&provider_id)?;
        (project, warnings, profile)
    };
    let initial_ids = warnings
        .iter()
        .map(|segment| segment.id.clone())
        .collect::<Vec<_>>();
    let initial_count = initial_ids.len();
    if warnings.is_empty() {
        return Ok(TtsFitResult {
            initial_count: 0,
            resolved_count: 0,
            remaining_ids: Vec::new(),
            modified_segment_ids: Vec::new(),
            undo_available: false,
        });
    }
    state
        .tts_fit_snapshots
        .lock()
        .expect("TTS fit snapshot map poisoned")
        .insert(project_id.clone(), warnings.clone());
    emit_tts_fit_progress(
        &app,
        TtsFitProgress {
            project_id: project_id.clone(),
            stage: "compressing".into(),
            completed: 0,
            total: initial_count,
            progress: 0,
        },
    );
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
            database.reopen_job(&job_id, JobStage::Tts, 63, "tts:fit-started")?;
        } else if matches!(
            current.status,
            JobStatus::Paused | JobStatus::Failed | JobStatus::WaitingUser
        ) {
            database.transition_job(&job_id, JobStatus::Queued)?;
        }
        database.start_job(&job_id)?;
        database.checkpoint_job(&job_id, JobStage::Tts, 63, "tts:fit-started")?;
        emit_job_state(&app, &database.get_job(&job_id)?);
    }
    for attempt in 0..2 {
        if warnings.is_empty() {
            break;
        }
        let warning_count = warnings.len();
        if project.tts_sync_mode == "semantic" {
            let all_segments = {
                let database = state.database.lock().expect("database mutex poisoned");
                database.list_segments(&project_id)?
            };
            let warning_ids = warnings
                .iter()
                .map(|segment| segment.id.as_str())
                .collect::<std::collections::HashSet<_>>();
            let affected_blocks = balanced_tts_blocks(&all_segments)
                .into_iter()
                .filter(|block| {
                    block
                        .segments
                        .iter()
                        .any(|segment| warning_ids.contains(segment.id.as_str()))
                })
                .collect::<Vec<_>>();
            for (index, block) in affected_blocks.iter().enumerate() {
                let original_chars = block
                    .segments
                    .iter()
                    .map(|segment| segment.spoken_zh.chars().count())
                    .sum::<usize>();
                let window_ms = (block.end_ms - block.start_ms).max(500) as usize;
                let duration_budget = window_ms / if attempt == 0 { 230 } else { 270 };
                let factor = if attempt == 0 { 0.72 } else { 0.58 };
                let target_chars = ((original_chars as f64 * factor).round() as usize)
                    .min(duration_budget)
                    .max(block.segments.len() * 2);
                let fit_segments = block
                    .segments
                    .iter()
                    .map(|segment| crate::translation::SemanticFitSegment {
                        id: &segment.id,
                        spoken_zh: &segment.spoken_zh,
                    })
                    .collect::<Vec<_>>();
                let compressed = crate::translation::compress_semantic_block(
                    &client,
                    &config,
                    &secret,
                    &fit_segments,
                    target_chars,
                )
                .await?;
                let database = state.database.lock().expect("database mutex poisoned");
                for (id, spoken) in compressed {
                    database.update_segment_spoken(&id, &spoken)?;
                }
                emit_tts_fit_progress(
                    &app,
                    TtsFitProgress {
                        project_id: project_id.clone(),
                        stage: "compressing".into(),
                        completed: index + 1,
                        total: affected_blocks.len(),
                        progress: (((index + 1) * 70 / affected_blocks.len().max(1)) as u8).min(70),
                    },
                );
            }
            break;
        }
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
            let progress = 63 + (done * 16 / total.max(1)) as u8;
            database.checkpoint_job(
                &job_id,
                JobStage::Tts,
                progress,
                &format!("tts:compress-{}/{total}", done),
            )?;
            emit_job_state(&app, &database.get_job(&job_id)?);
            emit_tts_fit_progress(
                &app,
                TtsFitProgress {
                    project_id: project_id.clone(),
                    stage: "compressing".into(),
                    completed: (index + 1).min(warning_count),
                    total: warning_count,
                    progress: (((index + 1) * 70 / warning_count.max(1)) as u8).min(70),
                },
            );
        }
        emit_tts_fit_progress(
            &app,
            TtsFitProgress {
                project_id: project_id.clone(),
                stage: "synthesizing".into(),
                completed: attempt + 1,
                total: 2,
                progress: 78 + attempt as u8 * 8,
            },
        );
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
        let diagnostic_track = artifact_dir.join(format!("tts-fit.{}.pending.wav", Uuid::new_v4()));
        let diagnostic_for_worker = diagnostic_track.clone();
        let output = tauri::async_runtime::spawn_blocking(move || {
            crate::tts::synthesize_to(
                &candidates,
                &artifact_for_worker,
                duration_ms,
                Some(&diagnostic_for_worker),
                |_, _| {},
            )
        })
        .await
        .map_err(|error| AppError::Media(error.to_string()))??;
        let _ = std::fs::remove_file(&output.track_path);
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
            .filter(|segment| segment.status == "warning" && segment.tts_state != "failed")
            .collect();
    }
    emit_tts_fit_progress(
        &app,
        TtsFitProgress {
            project_id: project_id.clone(),
            stage: "validating".into(),
            completed: 1,
            total: 1,
            progress: 95,
        },
    );
    {
        let database = state.database.lock().expect("database mutex poisoned");
        database.transition_job(&job_id, JobStatus::Paused)?;
    }
    let result = tts_run(app.clone(), state, project_id.clone(), job_id, None).await?;
    emit_tts_fit_progress(
        &app,
        TtsFitProgress {
            project_id: project_id.clone(),
            stage: "complete".into(),
            completed: initial_count,
            total: initial_count,
            progress: 100,
        },
    );
    let initial_id_set = initial_ids.iter().collect::<std::collections::HashSet<_>>();
    let remaining_ids = result
        .warning_ids
        .into_iter()
        .filter(|id| initial_id_set.contains(id))
        .collect::<Vec<_>>();
    Ok(TtsFitResult {
        initial_count,
        resolved_count: initial_count.saturating_sub(remaining_ids.len()),
        remaining_ids,
        modified_segment_ids: initial_ids,
        undo_available: true,
    })
}

#[tauri::command]
pub fn tts_fit_undo(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<String>, AppError> {
    let snapshot = state
        .tts_fit_snapshots
        .lock()
        .expect("TTS fit snapshot map poisoned")
        .get(&project_id)
        .cloned()
        .ok_or_else(|| AppError::Validation("没有可撤销的自动修复记录".into()))?;
    let database = state.database.lock().expect("database mutex poisoned");
    let mut restored_ids = Vec::with_capacity(snapshot.len());
    for segment in snapshot {
        database.restore_segment_spoken_snapshot(&segment)?;
        restored_ids.push(segment.id.clone());
    }
    drop(database);
    state
        .tts_fit_snapshots
        .lock()
        .expect("TTS fit snapshot map poisoned")
        .remove(&project_id);
    Ok(restored_ids)
}

#[tauri::command]
pub async fn export_start(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    job_id: String,
    output_directory: String,
    subtitle_mode: String,
    export_preset: Option<String>,
) -> Result<crate::exporter::ExportOutput, AppError> {
    if !matches!(subtitle_mode.as_str(), "none" | "chinese" | "bilingual") {
        return Err(AppError::Validation("未知的字幕导出模式".into()));
    }
    let export_preset = export_preset.unwrap_or_else(|| "balanced".into());
    if !matches!(export_preset.as_str(), "share" | "balanced" | "high") {
        return Err(AppError::Validation("未知的视频导出预设".into()));
    }
    let (project, segments, tts_artifacts, timeline_edits) = {
        let database = state.database.lock().expect("database mutex poisoned");
        let project = database.get_project(&project_id)?;
        let segments = database.list_segments(&project_id)?;
        let mut preflight = export_preflight_for_segments(&segments);
        apply_safe_background_preflight(&project, &mut preflight);
        if !preflight.can_export {
            return Err(AppError::Validation(preflight.message));
        }
        let artifact = database.get_artifact(&format!("tts-mix-{project_id}"))?;
        validate_tts_mix_for_export(&project, &segments, &artifact)?;
        let mut tts_artifacts =
            database.list_artifacts(&project_id, None, Some("tts_semantic_anchored"))?;
        tts_artifacts.extend(database.list_artifacts(
            &project_id,
            None,
            Some("tts_block_aligned"),
        )?);
        tts_artifacts.extend(database.list_artifacts(
            &project_id,
            None,
            Some("tts_narration_chapter"),
        )?);
        let timeline_edits = database.list_timeline_edits(&project_id)?;
        (project, segments, tts_artifacts, timeline_edits)
    };
    {
        let database = state.database.lock().expect("database mutex poisoned");
        let current = database.get_job(&job_id)?;
        match current.status {
            JobStatus::Succeeded => {
                database.reopen_job(&job_id, JobStage::Export, 80, "export:queued")?;
            }
            JobStatus::Paused | JobStatus::Failed | JobStatus::WaitingUser => {
                database.transition_job(&job_id, JobStatus::Queued)?;
            }
            _ => {}
        }
        database.start_job(&job_id)?;
        database.checkpoint_job(&job_id, JobStage::Export, 82, "export:started")?;
        emit_job_state(&app, &database.get_job(&job_id)?);
    }
    let output = PathBuf::from(output_directory);
    let result = tauri::async_runtime::spawn_blocking(move || {
        crate::exporter::export(
            &project,
            &segments,
            &tts_artifacts,
            &timeline_edits,
            &output,
            &subtitle_mode,
            &export_preset,
        )
    })
    .await
    .map_err(|e| AppError::Media(e.to_string()))?;
    match result {
        Ok(value) => {
            let database = state.database.lock().expect("database mutex poisoned");
            database.checkpoint_job(
                &job_id,
                JobStage::Export,
                100,
                &format!("export:{}", value.directory),
            )?;
            let job = database.transition_job(&job_id, JobStatus::Succeeded)?;
            drop(database);
            if let Some(run) = state.workflow_store.find_run_by_legacy_job_id(&job_id)? {
                let awaiting_export = run.state
                    == crate::workflow::WorkflowRunState::WaitingForInput
                    && run.current_node_id.as_deref()
                        == Some(crate::application::production_workflow::EXPORT_NODE_ID);
                if awaiting_export {
                    // The files and legacy job are already committed. A workflow
                    // projection failure must never turn that durable success into
                    // a user-visible export failure.
                    let _ = state.workflow_store.complete_external_node(
                        &job_id,
                        crate::application::production_workflow::EXPORT_NODE_ID,
                        crate::application::production_workflow::EXPORT_NODE_VERSION,
                        &[value.video_path.clone(), value.audio_path.clone()],
                        &format!("export:{}", value.directory),
                    );
                }
            }
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
pub fn tts_catalog(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<TtsCatalog, AppError> {
    let profile = if provider_id == "system" {
        None
    } else {
        Some(
            state
                .database
                .lock()
                .expect("database mutex poisoned")
                .get_provider(&provider_id)?,
        )
    };
    let driver = profile
        .as_ref()
        .map_or("system", |value| value.driver.as_str());
    let provider_name = profile
        .as_ref()
        .map_or("macOS 系统语音", |value| value.name.as_str());
    let configured = provider_id == "system"
        || profile.as_ref().is_some_and(|value| {
            value.secret_bundle_ref.is_some() || value.credential_ref.is_some()
        });
    let public_config = profile.as_ref().and_then(|value| {
        serde_json::from_str::<serde_json::Value>(&value.public_config_json).ok()
    });
    let configured_voice = public_config
        .as_ref()
        .and_then(|value| value.get("voice"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let voices = match driver {
        "aliyun_tts" | "bailian_tts" => {
            let model = public_config
                .as_ref()
                .and_then(|value| value.get("model"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("qwen3-tts-instruct-flash");
            let defaults = if model.starts_with("cosyvoice-") {
                cosyvoice_default_voices(model)
            } else {
                vec![
                    ("Cherry", "Cherry", "明亮 · 自然", "female"),
                    ("Serena", "Serena", "知性 · 讲解", "female"),
                    ("Ethan", "Ethan", "沉稳 · 清晰", "male"),
                ]
            };
            catalog_voices(
                &provider_id,
                provider_name,
                configured,
                configured_voice,
                defaults,
            )
        }
        "iflytek_super_tts" | "iflytek" => catalog_voices(
            &provider_id,
            provider_name,
            configured,
            configured_voice,
            // iFLYTEK grants character quota and voice entitlement separately.
            // The account's configured VCN is the only voice we can truthfully
            // expose as a candidate; a static marketing catalog would let users
            // select voices their account cannot synthesize.
            vec![],
        ),
        _ => vec![tts_voice(
            "system",
            "macOS 系统语音",
            "Tingting",
            "Tingting",
            "本地 · 免费",
            "female",
            true,
        )],
    };
    Ok(TtsCatalog {
        provider_id,
        driver: driver.into(),
        local: driver == "system",
        voices,
        styles: tts_styles(),
        supports_preview: true,
        supports_instructions: matches!(driver, "aliyun_tts" | "bailian_tts"),
        data_scope: if driver == "system" {
            "local"
        } else {
            "text_only"
        }
        .into(),
    })
}

fn cosyvoice_default_voices(
    model: &str,
) -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    if model == "cosyvoice-v3-plus" {
        vec![
            ("longanhuan", "龙安欢", "自然 · 讲解", "female"),
            ("longanyang", "龙安洋", "阳光 · 清晰", "male"),
        ]
    } else {
        vec![
            ("longanhuan_v3", "龙安欢", "自然 · 讲解", "female"),
            ("longxiaochun_v3", "龙小淳", "沉稳 · 清晰", "male"),
        ]
    }
}

fn catalog_voices(
    provider_id: &str,
    provider_name: &str,
    configured: bool,
    configured_voice: Option<&str>,
    defaults: Vec<(&str, &str, &str, &str)>,
) -> Vec<TtsVoiceDescriptor> {
    let mut voices = defaults
        .into_iter()
        .map(|(id, name, traits, gender)| {
            tts_voice(
                provider_id,
                provider_name,
                id,
                name,
                traits,
                gender,
                configured,
            )
        })
        .collect::<Vec<_>>();
    if let Some(voice_id) = configured_voice {
        if !voices.iter().any(|voice| voice.id == voice_id) {
            voices.insert(
                0,
                tts_voice(
                    provider_id,
                    provider_name,
                    voice_id,
                    voice_id,
                    "当前配置 · 需服务商授权",
                    "neutral",
                    configured,
                ),
            );
        }
    }
    voices
}

fn tts_voice(
    provider_id: &str,
    provider_name: &str,
    id: &str,
    name: &str,
    traits: &str,
    gender: &str,
    available: bool,
) -> TtsVoiceDescriptor {
    TtsVoiceDescriptor {
        id: id.into(),
        provider_id: provider_id.into(),
        provider_name: provider_name.into(),
        name: name.into(),
        locale: "zh-CN".into(),
        gender: Some(gender.into()),
        traits: traits.split(" · ").map(str::to_string).collect(),
        available,
    }
}

fn tts_styles() -> Vec<TtsStyleDescriptor> {
    [
        ("auto", "自动", "根据语义选择自然表达"),
        ("professional", "专业讲解", "克制、清晰、可信"),
        ("conversational", "自然口语", "轻松、有交流感"),
        ("documentary", "沉稳纪录", "低起伏、叙事感"),
        ("upbeat", "轻快分享", "更明亮活泼"),
        ("emphasis", "重点强调", "突出关键信息"),
    ]
    .into_iter()
    .map(|(id, label, description)| TtsStyleDescriptor {
        id: id.into(),
        label: label.into(),
        description: description.into(),
    })
    .collect()
}

#[tauri::command]
pub fn provider_save(
    state: State<'_, AppState>,
    id: String,
    kind: String,
    name: String,
    public_config_json: String,
    secret: Option<String>,
    driver: Option<String>,
) -> Result<ProviderProfile, AppError> {
    serde_json::from_str::<serde_json::Value>(&public_config_json)
        .map_err(|error| AppError::Validation(format!("invalid provider config: {error}")))?;
    let existing = state
        .database
        .lock()
        .expect("database mutex poisoned")
        .get_provider(&id)
        .ok();
    let existing_credential = existing.as_ref().and_then(|profile| {
        profile
            .secret_bundle_ref
            .clone()
            .or_else(|| profile.credential_ref.clone())
    });
    let previous_secret = existing_credential
        .as_deref()
        .and_then(|reference| credentials::get(reference).ok());
    let secret_to_save = secret.filter(|value| !value.trim().is_empty());
    let credential_ref = match secret_to_save.as_deref() {
        Some(value) => Some(credentials::save(&id, value)?),
        _ => existing_credential,
    };
    let config = serde_json::from_str::<serde_json::Value>(&public_config_json)
        .map_err(|error| AppError::Validation(format!("invalid provider config: {error}")))?;
    let driver = driver
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            config
                .get("vendor")
                .and_then(serde_json::Value::as_str)
                .map(|vendor| match vendor {
                    "bailian_tts" => "aliyun_tts".to_string(),
                    "iflytek" => "iflytek_super_tts".to_string(),
                    other => other.to_string(),
                })
        })
        .unwrap_or_else(|| kind.clone());
    let synthesis_config_changed = existing.as_ref().is_some_and(|profile| {
        profile.kind != kind
            || profile.public_config_json != public_config_json
            || profile.driver != driver
            || secret_to_save
                .as_deref()
                .is_some_and(|value| previous_secret.as_deref() != Some(value))
    });
    let profile = ProviderProfile {
        id,
        kind,
        name,
        public_config_json,
        credential_ref: credential_ref.clone(),
        driver,
        revision: existing.as_ref().map_or(1, |profile| {
            if synthesis_config_changed {
                profile.revision.saturating_add(1)
            } else {
                profile.revision
            }
        }),
        secret_bundle_ref: credential_ref,
        updated_at: String::new(),
    };
    let save_result = state
        .database
        .lock()
        .expect("database mutex poisoned")
        .save_provider(&profile);
    if let Err(error) = save_result {
        if secret_to_save.is_some() {
            if let Some(previous) = previous_secret.as_deref() {
                let _ = credentials::save(&profile.id, previous);
            } else if let Some(reference) = profile.secret_bundle_ref.as_deref() {
                let _ = credentials::delete(reference);
            }
        }
        return Err(error);
    }
    state
        .database
        .lock()
        .expect("database mutex poisoned")
        .get_provider(&profile.id)
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
    if profile.kind == "cloud_tts" {
        let reference = profile
            .secret_bundle_ref
            .as_ref()
            .or(profile.credential_ref.as_ref())
            .ok_or_else(|| AppError::Provider("请先保存语音服务凭据".into()))?;
        let raw_secret = credentials::get(reference)?;
        let secret = TtsSecretBundle::from_keychain_value(&profile.driver, &raw_secret)?;
        let config = serde_json::from_str::<serde_json::Value>(&profile.public_config_json)
            .map_err(|_| AppError::Provider("语音服务配置无法解析".into()))?;
        if profile.driver == "iflytek_super_tts" || profile.driver == "iflytek" {
            secret
                .validate_public_app_id(config.get("appId").and_then(serde_json::Value::as_str))?;
        }
        let voice_id = config
            .get("voice")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| {
                if profile.driver == "iflytek_super_tts" {
                    "x6_lingxiaoxuan_flow"
                } else {
                    "Cherry"
                }
            });
        let request = SynthesisRequest {
            text: "连接测试。".into(),
            voice_id: voice_id.into(),
            style: "professional".into(),
            instructions: Some("简短、自然地读出连接测试，不新增内容。".into()),
            speed: 1.0,
            pitch: 1.0,
            volume: 1.0,
            sample_rate: 24_000,
            target_duration_ms: None,
        };
        let started = Instant::now();
        let output = tokio::time::timeout(
            Duration::from_secs(30),
            provider_adapter(&profile)?.synthesize(&request, &secret),
        )
        .await
        .map_err(|_| {
            AppError::Provider("语音服务连接测试超时，请检查网络、Keychain 权限或服务商状态".into())
        })??;
        if output.bytes.is_empty() {
            return Err(AppError::Provider("语音服务没有返回试听音频".into()));
        }
        return Ok(ProviderTestResult {
            ok: true,
            latency_ms: started.elapsed().as_millis(),
            message: "连接成功，音色与凭据可用".into(),
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

fn provider_adapter(profile: &ProviderProfile) -> Result<Box<dyn TtsProviderAdapter>, AppError> {
    let config = serde_json::from_str::<serde_json::Value>(&profile.public_config_json)
        .map_err(|_| AppError::Provider("语音服务配置无法解析".into()))?;
    match profile.driver.as_str() {
        "aliyun_tts" | "bailian_tts" => {
            let model = config
                .get("model")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("qwen3-tts-instruct-flash");
            let region = config
                .get("region")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("cn-beijing");
            let configured_base = config
                .get("baseUrl")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let endpoint = aliyun_tts_endpoint(model, region, configured_base)?;
            let aliyun: AliyunTtsConfig = serde_json::from_value(serde_json::json!({
                "model": model,
                "endpoint": endpoint,
                "region": region,
                "optimizeInstructions": false,
                "sampleRate": 24000
            }))
            .map_err(|_| AppError::Provider("阿里语音配置无法解析".into()))?;
            Ok(Box::new(AliyunTtsAdapter::new(aliyun)?))
        }
        "iflytek_super_tts" | "iflytek" => {
            let iflytek: IflytekSuperTtsConfig = serde_json::from_value(serde_json::json!({
                "endpoint": config.get("baseUrl").and_then(serde_json::Value::as_str).unwrap_or(crate::tts_provider::IFLYTEK_SUPER_TTS_ENDPOINT),
                "oralLevel": "mid",
                "sparkAssist": false,
                "remainOriginal": true,
                "sampleRate": 24000
            })).map_err(|_| AppError::Provider("讯飞语音配置无法解析".into()))?;
            Ok(Box::new(IflytekSuperTtsAdapter::new(iflytek)?))
        }
        _ => Err(AppError::Provider("当前语音服务驱动尚未适配".into())),
    }
}

fn aliyun_tts_endpoint(
    model: &str,
    region: &str,
    configured_base: Option<&str>,
) -> Result<String, AppError> {
    let cosyvoice = model.starts_with("cosyvoice-");
    if cosyvoice && region != "cn-beijing" {
        return Err(AppError::Provider(
            "CosyVoice HTTP 合成目前仅支持北京地域；请切换北京或改用 Qwen3-TTS".into(),
        ));
    }
    let path = if cosyvoice {
        "/api/v1/services/audio/tts/SpeechSynthesizer"
    } else {
        "/api/v1/services/aigc/multimodal-generation/generation"
    };
    let default_base = match region {
        "cn-beijing" => "https://dashscope.aliyuncs.com/api/v1",
        "ap-southeast-1" => "https://dashscope-intl.aliyuncs.com/api/v1",
        _ => return Err(AppError::Provider("不支持的阿里百炼地域".into())),
    };
    let base = configured_base
        .unwrap_or(default_base)
        .trim_end_matches('/');
    let base = base.strip_suffix("/api/v1").unwrap_or(base);
    Ok(format!("{base}{path}"))
}

#[tauri::command]
pub fn provider_delete(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    let profile = state
        .database
        .lock()
        .expect("database mutex poisoned")
        .get_provider(&id)?;
    let reference = profile
        .secret_bundle_ref
        .as_deref()
        .or(profile.credential_ref.as_deref());
    let secret = reference.and_then(|value| credentials::get(value).ok());
    if let Some(reference) = reference {
        credentials::delete(reference)?;
    }
    match state
        .database
        .lock()
        .expect("database mutex poisoned")
        .remove_provider(&id)
    {
        Ok(_) => Ok(()),
        Err(error) => {
            if let Some(secret) = secret.as_deref() {
                let _ = credentials::save(&id, secret);
            }
            Err(error)
        }
    }
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
    let separation_installed = app_data
        .as_deref()
        .is_some_and(crate::media::separation_runtime_installed);
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
            architecture: architecture.clone(),
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
        RuntimeComponent {
            id: "audio-separator".into(),
            name: "安全模式本地人声分离".into(),
            architecture,
            version: "0.44.5 · UVR-MDX-NET-Inst_HQ_3".into(),
            installed: separation_installed,
            sha256: Some("app-managed-runtime".into()),
            license: "MIT runtime · model license shown on download".into(),
            size_bytes: None,
            status: if separation_installed {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ProjectStatus;

    fn system_project() -> ProjectSummary {
        ProjectSummary {
            id: "p-system".into(),
            name: "System TTS".into(),
            status: ProjectStatus::Draft,
            progress: 0,
            source_path: None,
            source_fingerprint: None,
            duration_ms: Some(2_000),
            width: None,
            height: None,
            artifact_dir: None,
            workflow_mode: "local".into(),
            audio_mode: "replace".into(),
            translation_provider_id: None,
            tts_provider_id: "system".into(),
            tts_voice_id: Some("Tingting".into()),
            tts_style: "natural".into(),
            tts_settings_json: "{}".into(),
            tts_director_enabled: false,
            tts_sync_mode: "strict".into(),
            tts_settings_revision: 1,
            segment_count: 2,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn cosyvoice_catalog_uses_model_matched_voice_ids() {
        let v3_plus = cosyvoice_default_voices("cosyvoice-v3-plus");
        assert_eq!(v3_plus[0].0, "longanhuan");
        assert_eq!(v3_plus[1].0, "longanyang");

        let v3_flash = cosyvoice_default_voices("cosyvoice-v3-flash");
        assert_eq!(v3_flash[0].0, "longanhuan_v3");
        assert_eq!(v3_flash[1].0, "longxiaochun_v3");
    }

    #[test]
    fn iflytek_invalid_voice_is_a_non_retryable_pipeline_error() {
        let source = AppError::Provider(
            "讯飞语音合成失败（错误码：10163；服务信息：'$.parameter.tts.vcn' must be one of [x7_susu_pro]）".into(),
        );
        let error = non_retryable_tts_error(&source)
            .expect("an account voice entitlement mismatch must stop the whole run")
            .to_string();
        assert!(error.contains("未获得此账号授权"));
        assert!(error.contains("未继续重复请求"));
        assert!(non_retryable_tts_error(&AppError::Provider(
            "讯飞语音合成失败（错误码：11200；LiccCheck failed）".into()
        ))
        .is_some());
        assert!(non_retryable_tts_error(&AppError::Provider("连接超时".into())).is_none());
    }

    fn system_segment(id: &str, ordinal: i64) -> SegmentRecord {
        SegmentRecord {
            id: id.into(),
            project_id: "p-system".into(),
            ordinal,
            start_ms: ordinal * 1_000,
            end_ms: (ordinal + 1) * 1_000,
            source_text: "source".into(),
            subtitle_zh: "字幕".into(),
            spoken_zh: "配音".into(),
            linked: true,
            status: "ready".into(),
            script_doc_json: String::new(),
            script_revision: 1,
            tts_overrides_json: "{}".into(),
            tts_state: "stale".into(),
            tts_error_message: None,
            tts_settings_hash: None,
            tts_duration_ms: None,
        }
    }

    #[test]
    fn system_partial_run_restores_a_stale_unselected_clip_from_disk_cache() {
        let project = system_project();
        let selected = system_segment("s1", 0);
        let reusable = system_segment("s2", 1);
        let cache_root =
            std::env::temp_dir().join(format!("yisheng-system-tts-cache-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&cache_root).unwrap();
        let settings_hash = system_tts_settings_hash(&project, &reusable);
        let cached = system_segment_cache_path(&cache_root, &reusable, &settings_hash);
        std::fs::write(&cached, [0_u8; 45]).unwrap();
        let segments = [selected, reusable];
        let selected_ids = std::collections::HashSet::from(["s1"]);

        assert!(
            validate_system_partial_cache(&project, &segments, &selected_ids, &cache_root).is_ok(),
            "the selected clip itself must not need a prior full-run cache"
        );

        std::fs::remove_file(&cached).unwrap();
        let error = validate_system_partial_cache(&project, &segments, &selected_ids, &cache_root)
            .unwrap_err()
            .to_string();
        assert!(error.contains("先完成一次全片配音"));
        assert!(error.contains("s2"));
        std::fs::remove_dir(&cache_root).unwrap();
    }

    #[test]
    fn system_cache_key_is_stable_across_track_switches_but_changes_with_audio_inputs() {
        let segment = system_segment("s1", 0);
        let project = system_project();
        let baseline = system_tts_settings_hash(&project, &segment);

        let mut switched_away_and_back = project.clone();
        switched_away_and_back.tts_settings_revision += 4;
        assert_eq!(
            baseline,
            system_tts_settings_hash(&switched_away_and_back, &segment)
        );

        let mut changed = segment.clone();
        changed.tts_overrides_json = r#"{"speed":1.08}"#.into();
        assert_ne!(baseline, system_tts_settings_hash(&project, &changed));
    }

    #[test]
    fn continuity_instruction_uses_neighbors_without_changing_spoken_copy() {
        let segments = [
            system_segment("s0", 0),
            system_segment("s1", 1),
            system_segment("s2", 2),
        ];
        let instruction = continuity_instruction(&segments, 1);
        assert!(instruction.contains("承接上句"));
        assert!(instruction.contains("自然引向下句"));
        assert!(instruction.contains("严格只朗读当前正文"));
    }

    #[test]
    fn balanced_blocks_follow_time_gaps_and_override_boundaries() {
        let timed = |id: &str, ordinal: i64, start_ms: i64, end_ms: i64| {
            let mut value = system_segment(id, ordinal);
            value.start_ms = start_ms;
            value.end_ms = end_ms;
            value
        };
        let mut segments = vec![
            timed("s1", 0, 0, 3_000),
            timed("s2", 1, 3_100, 6_000),
            timed("s3", 2, 6_100, 9_000),
            timed("s4", 3, 10_500, 13_000),
            timed("s5", 4, 13_100, 16_000),
        ];
        segments[4].tts_overrides_json = r#"{"style":"emphasis"}"#.into();
        let blocks = balanced_tts_blocks(&segments);
        let ids = blocks
            .iter()
            .map(|block| {
                block
                    .segments
                    .iter()
                    .map(|segment| segment.id.as_str())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![vec!["s1", "s2", "s3"], vec!["s4"], vec!["s5"]]);
    }

    #[test]
    fn narration_chapters_use_publishable_scene_windows() {
        let timed = |id: &str, ordinal: i64, start_ms: i64, end_ms: i64| {
            let mut value = system_segment(id, ordinal);
            value.start_ms = start_ms;
            value.end_ms = end_ms;
            value.spoken_zh = "连续讲解。".into();
            value
        };
        let segments = vec![
            timed("s1", 0, 0, 30_000),
            timed("s2", 1, 30_100, 59_000),
            timed("s3", 2, 60_100, 88_000),
            timed("s4", 3, 88_100, 118_000),
        ];
        let chapters = narration_chapters(&segments);
        let ids = chapters
            .iter()
            .map(|chapter| {
                chapter
                    .segments
                    .iter()
                    .map(|segment| segment.id.as_str())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![vec!["s1"], vec!["s2"], vec!["s3"], vec!["s4"]]);
        assert!(chapters
            .iter()
            .all(|chapter| chapter.end_ms - chapter.start_ms <= 30_000));
    }

    #[test]
    fn semantic_scenes_use_short_beats_inside_bounded_context() {
        let timed = |index: usize, start_ms: i64, end_ms: i64| {
            let mut value = system_segment(&format!("s{index}"), index as i64);
            value.start_ms = start_ms;
            value.end_ms = end_ms;
            value.spoken_zh = "连续讲解。".into();
            value
        };
        let segments = (0..18)
            .map(|index| timed(index, index as i64 * 3_000, index as i64 * 3_000 + 2_800))
            .collect::<Vec<_>>();
        let scenes = semantic_scenes(&segments);
        assert!(!scenes.is_empty());
        assert!(scenes
            .iter()
            .all(|scene| scene.end_ms - scene.start_ms <= 60_000));
        assert!(scenes.iter().all(|scene| scene
            .beats
            .iter()
            .all(|beat| beat.end_ms - beat.start_ms <= 15_000)));
    }

    #[test]
    fn export_preflight_blocks_stale_audio_but_allows_timing_warnings() {
        let mut ready = system_segment("ready", 0);
        ready.tts_state = "ready".into();
        let mut warning = system_segment("warning", 1);
        warning.tts_state = "ready".into();
        warning.status = "warning".into();
        let advisory = export_preflight_for_segments(&[ready.clone(), warning]);
        assert!(advisory.can_export);
        assert_eq!(advisory.warning_count, 1);
        assert_eq!(advisory.blocking_count, 0);

        let mut unpublished_warning = system_segment("warning", 2);
        unpublished_warning.tts_state = "stale".into();
        unpublished_warning.status = "warning".into();
        let advisory = export_preflight_for_segments(&[unpublished_warning]);
        assert!(advisory.can_export);
        assert_eq!(advisory.warning_count, 1);
        assert_eq!(advisory.blocking_count, 0);

        let mut failed = ready;
        failed.tts_state = "failed".into();
        let blocked = export_preflight_for_segments(&[failed]);
        assert!(!blocked.can_export);
        assert_eq!(blocked.blocking_count, 1);
        assert_eq!(blocked.warning_count, 0);
        let issue = blocked
            .checks
            .iter()
            .find(|check| check.code == "tts_not_ready")
            .unwrap();
        assert_eq!(issue.message, "第 1 段尚未成功生成中文配音");
        assert_eq!(issue.source_range, Some([0, 1_000]));
        assert_eq!(issue.scene_id.as_deref(), Some("ready"));
    }

    #[test]
    fn export_preflight_preserves_non_speech_without_requiring_tts() {
        let mut music = system_segment("music", 0);
        music.source_text = "[MUSIC PLAYING]".into();
        music.subtitle_zh = "[音乐播放中]".into();
        music.spoken_zh = "[音乐播放中]".into();
        music.tts_state = "stale".into();
        music.status = "stale".into();

        let preflight = export_preflight_for_segments(&[music]);
        assert!(preflight.can_export);
        assert_eq!(preflight.blocking_count, 0);
        assert!(preflight
            .checks
            .iter()
            .any(|check| check.code == "non_speech_spoken" && check.severity == "info"));
    }
}
