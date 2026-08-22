use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    domain::{JobStage, MediaArtifacts, MediaProbe, PreviewMedia, TimelineEdit},
    error::AppError,
    timeline_map::TimelineMap,
};

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    streams: Vec<ProbeStream>,
    format: ProbeFormat,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    sample_rate: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
    size: Option<String>,
}

pub fn probe(path: &Path) -> Result<MediaProbe, AppError> {
    if !path.is_file() {
        return Err(AppError::Validation("选择的视频文件不存在".into()));
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "mp4" | "mov" | "mkv" | "m4v" | "webm") {
        return Err(AppError::Validation(
            "V1 仅支持 MP4、MOV、MKV、M4V 和 WebM".into(),
        ));
    }
    let output = Command::new(resolve_binary("ffprobe"))
        .args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .map_err(|error| AppError::Media(format!("无法启动 ffprobe：{error}")))?;
    if !output.status.success() {
        return Err(AppError::Media(sanitize_process_error(&output.stderr)));
    }
    let parsed: ProbeOutput = serde_json::from_slice(&output.stdout)
        .map_err(|error| AppError::Media(format!("ffprobe 输出无法解析：{error}")))?;
    let video = parsed
        .streams
        .iter()
        .find(|stream| stream.codec_type.as_deref() == Some("video"))
        .ok_or_else(|| AppError::Validation("文件中没有可用的视频轨道".into()))?;
    let audio = parsed
        .streams
        .iter()
        .find(|stream| stream.codec_type.as_deref() == Some("audio"));
    let duration_ms = parsed
        .format
        .duration
        .as_deref()
        .and_then(|value| value.parse::<f64>().ok())
        .map(|seconds| (seconds * 1000.0).round() as i64)
        .unwrap_or_default();
    if duration_ms <= 0 {
        return Err(AppError::Validation("无法读取视频时长".into()));
    }
    let metadata = std::fs::metadata(path).map_err(|error| AppError::Media(error.to_string()))?;
    Ok(MediaProbe {
        source_path: path.to_string_lossy().into_owned(),
        fingerprint: fingerprint(path, &metadata)?,
        file_name: path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("video")
            .to_string(),
        file_size: parsed
            .format
            .size
            .and_then(|value| value.parse().ok())
            .unwrap_or(metadata.len()),
        duration_ms,
        width: video.width.unwrap_or_default(),
        height: video.height.unwrap_or_default(),
        video_codec: video.codec_name.clone().unwrap_or_else(|| "unknown".into()),
        audio_codec: audio.and_then(|stream| stream.codec_name.clone()),
        audio_sample_rate: audio
            .and_then(|stream| stream.sample_rate.as_deref())
            .and_then(|value| value.parse().ok()),
    })
}

pub fn prepare<F>(
    project_id: &str,
    source: &Path,
    root: &Path,
    audio_mode: &str,
    source_fingerprint: &str,
    mut progress: F,
) -> Result<MediaArtifacts, AppError>
where
    F: FnMut(JobStage, u8, &str) -> Result<(), AppError>,
{
    let artifact_dir = root.join("projects").join(project_id).join("media");
    std::fs::create_dir_all(&artifact_dir).map_err(|error| AppError::Media(error.to_string()))?;
    let audio_path = artifact_dir.join("source-16k-mono.wav");
    let proxy_path = artifact_dir.join("preview-proxy.mp4");
    ensure_thumbnail(project_id, source, root)?;

    if !is_non_empty_file(&audio_path) {
        progress(JobStage::AudioExtract, 4, "media:extracting-audio")?;
        run_ffmpeg(
            source,
            &audio_path,
            ["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"],
        )?;
    }
    progress(JobStage::AudioExtract, 8, "media:audio-ready")?;
    if audio_mode == "separate" {
        progress(
            JobStage::SourceSeparation,
            9,
            "media:safe-mode-separating-local-audio",
        )?;
        prepare_safe_background(source, &artifact_dir, root, source_fingerprint)?;
        progress(
            JobStage::SourceSeparation,
            12,
            "media:safe-background-ready",
        )?;
    }
    if !is_non_empty_file(&proxy_path) {
        progress(JobStage::Proxy, 13, "media:creating-proxy")?;
        let scale = "scale=min(1280\\,iw):-2";
        run_ffmpeg(
            source,
            &proxy_path,
            [
                "-vf",
                scale,
                "-c:v",
                "h264_videotoolbox",
                "-b:v",
                "2800k",
                "-c:a",
                "aac",
                "-b:a",
                "128k",
                "-movflags",
                "+faststart",
            ],
        )
        .or_else(|_| {
            run_ffmpeg(
                source,
                &proxy_path,
                [
                    "-vf",
                    scale,
                    "-c:v",
                    "libx264",
                    "-preset",
                    "veryfast",
                    "-crf",
                    "23",
                    "-c:a",
                    "aac",
                    "-b:a",
                    "128k",
                    "-movflags",
                    "+faststart",
                ],
            )
        })?;
    }
    progress(JobStage::Proxy, 15, "media:proxy-ready")?;

    Ok(MediaArtifacts {
        project_id: project_id.into(),
        proxy_path: proxy_path.to_string_lossy().into_owned(),
        audio_path: audio_path.to_string_lossy().into_owned(),
        artifact_dir: artifact_dir.to_string_lossy().into_owned(),
    })
}

const SEPARATION_MODEL: &str = "UVR-MDX-NET-Inst_HQ_3.onnx";
const SEPARATION_RUNTIME_VERSION: &str = "0.44.5";
pub const SAFE_BACKGROUND_FILE: &str = "background-safe.wav";
const SAFE_BACKGROUND_MANIFEST: &str = "background-safe.json";
const DUBBED_PREVIEW_MANIFEST: &str = "dubbed-preview.json";
const PREVIEW_RECIPE_VERSION: u32 = 2;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SafeBackgroundManifest {
    source_fingerprint: String,
    model: String,
    runtime_version: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct DubbedPreviewManifest {
    audio_mode: String,
    recipe_version: u32,
}

pub fn safe_background_is_ready(artifact_dir: &Path, source_fingerprint: &str) -> bool {
    let output = artifact_dir.join(SAFE_BACKGROUND_FILE);
    let manifest_path = artifact_dir.join(SAFE_BACKGROUND_MANIFEST);
    if !is_non_empty_file(&output) || !is_non_empty_file(&manifest_path) {
        return false;
    }
    std::fs::read_to_string(manifest_path)
        .ok()
        .and_then(|value| serde_json::from_str::<SafeBackgroundManifest>(&value).ok())
        .is_some_and(|manifest| {
            manifest.source_fingerprint == source_fingerprint
                && manifest.model == SEPARATION_MODEL
                && manifest.runtime_version == SEPARATION_RUNTIME_VERSION
        })
}

pub fn separation_runtime_installed(root: &Path) -> bool {
    separation_binary(root).is_some()
}

fn prepare_safe_background(
    source: &Path,
    artifact_dir: &Path,
    root: &Path,
    source_fingerprint: &str,
) -> Result<PathBuf, AppError> {
    let target = artifact_dir.join(SAFE_BACKGROUND_FILE);
    if safe_background_is_ready(artifact_dir, source_fingerprint) {
        return Ok(target);
    }
    let separator = separation_binary(root).ok_or_else(|| {
        AppError::Media(
            "安全模式所需的本地人声分离组件未安装；为避免英文残留，本次处理已停止，没有降级回混原声"
                .into(),
        )
    })?;
    let source_audio = artifact_dir.join("source-full-stereo.wav");
    let pending_source_audio = artifact_dir.join("source-full-stereo.pending.wav");
    let _ = std::fs::remove_file(&pending_source_audio);
    run_ffmpeg(
        source,
        &pending_source_audio,
        ["-vn", "-ac", "2", "-ar", "44100", "-c:a", "pcm_s16le"],
    )?;
    std::fs::rename(&pending_source_audio, &source_audio)
        .map_err(|error| AppError::Media(format!("无法更新本地分离输入：{error}")))?;
    let model_dir = root.join("models").join("vocal-separation");
    std::fs::create_dir_all(&model_dir)
        .map_err(|error| AppError::Media(format!("无法创建本地分离模型目录：{error}")))?;
    let model_path = model_dir.join(SEPARATION_MODEL);
    if model_path
        .metadata()
        .is_ok_and(|metadata| metadata.len() < 60 * 1024 * 1024)
    {
        std::fs::remove_file(&model_path)
            .map_err(|error| AppError::Media(format!("无法清理未完成的分离模型：{error}")))?;
    }
    let pending_dir = artifact_dir.join("safe-separation.pending");
    if pending_dir.exists() {
        std::fs::remove_dir_all(&pending_dir)
            .map_err(|error| AppError::Media(format!("无法清理未完成的分离任务：{error}")))?;
    }
    std::fs::create_dir_all(&pending_dir)
        .map_err(|error| AppError::Media(format!("无法创建本地分离临时目录：{error}")))?;
    let output = Command::new(separator)
        .arg(&source_audio)
        .args(["--model_filename", SEPARATION_MODEL, "--model_file_dir"])
        .arg(&model_dir)
        .arg("--output_dir")
        .arg(&pending_dir)
        .args(["--output_format", "WAV", "--single_stem", "Instrumental"])
        .output()
        .map_err(|error| AppError::Media(format!("无法启动本地人声分离组件：{error}")))?;
    if !output.status.success() {
        let _ = std::fs::remove_dir_all(&pending_dir);
        return Err(AppError::Media(format!(
            "本地人声分离失败；为避免英文残留，本次处理已停止：{}",
            sanitize_process_error(&output.stderr)
        )));
    }
    let instrumental = std::fs::read_dir(&pending_dir)
        .map_err(|error| AppError::Media(format!("无法读取本地分离结果：{error}")))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("wav"))
                && path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.to_ascii_lowercase().contains("instrumental"))
        })
        .ok_or_else(|| AppError::Media("本地分离未生成背景与音效轨，已阻止继续处理".into()))?;
    let pending_target = artifact_dir.join("background-safe.pending.wav");
    let _ = std::fs::remove_file(&pending_target);
    run_ffmpeg(
        &instrumental,
        &pending_target,
        ["-ac", "2", "-ar", "48000", "-c:a", "pcm_s16le"],
    )?;
    std::fs::rename(&pending_target, &target)
        .map_err(|error| AppError::Media(format!("无法保存安全背景轨：{error}")))?;
    let manifest = SafeBackgroundManifest {
        source_fingerprint: source_fingerprint.into(),
        model: SEPARATION_MODEL.into(),
        runtime_version: SEPARATION_RUNTIME_VERSION.into(),
    };
    let manifest_path = artifact_dir.join(SAFE_BACKGROUND_MANIFEST);
    let pending_manifest = artifact_dir.join("background-safe.pending.json");
    std::fs::write(
        &pending_manifest,
        serde_json::to_vec_pretty(&manifest)
            .map_err(|error| AppError::Media(format!("无法记录安全背景轨依赖：{error}")))?,
    )
    .map_err(|error| AppError::Media(format!("无法记录安全背景轨依赖：{error}")))?;
    std::fs::rename(&pending_manifest, manifest_path)
        .map_err(|error| AppError::Media(format!("无法发布安全背景轨依赖：{error}")))?;
    let _ = std::fs::remove_dir_all(&pending_dir);
    Ok(target)
}

fn separation_binary(root: &Path) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("YISHENG_AUDIO_SEPARATOR").map(PathBuf::from) {
        if path.is_file() {
            return Some(path);
        }
    }
    let managed = root
        .join("runtimes")
        .join("audio-separator")
        .join("bin")
        .join("audio-separator");
    if managed.is_file() {
        return Some(managed);
    }
    ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"]
        .into_iter()
        .map(|directory| Path::new(directory).join("audio-separator"))
        .find(|path| path.is_file())
}

/// Return a cached project cover rendered from the source video's first
/// decoded frame. The source stays in place; only the small JPEG is cached.
pub fn ensure_thumbnail(project_id: &str, source: &Path, root: &Path) -> Result<PathBuf, AppError> {
    if !source.is_file() {
        return Err(AppError::Media("项目原始视频不存在，无法生成封面".into()));
    }
    let artifact_dir = root.join("projects").join(project_id).join("media");
    std::fs::create_dir_all(&artifact_dir).map_err(|error| AppError::Media(error.to_string()))?;
    let target = artifact_dir.join("cover-first-frame.jpg");
    if is_non_empty_file(&target) {
        return Ok(target);
    }
    let temporary = artifact_dir.join("cover-first-frame.pending.jpg");
    let _ = std::fs::remove_file(&temporary);
    let output = Command::new(resolve_binary("ffmpeg"))
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(source)
        .args([
            "-map",
            "0:v:0",
            "-frames:v",
            "1",
            "-vf",
            "scale=min(960\\,iw):-2",
            "-q:v",
            "3",
        ])
        .arg(&temporary)
        .output()
        .map_err(|error| AppError::Media(format!("无法启动首帧封面生成：{error}")))?;
    if !output.status.success() || !is_non_empty_file(&temporary) {
        let _ = std::fs::remove_file(&temporary);
        return Err(AppError::Media(format!(
            "首帧封面生成失败：{}",
            sanitize_process_error(&output.stderr)
        )));
    }
    std::fs::rename(&temporary, &target)
        .map_err(|error| AppError::Media(format!("无法保存首帧封面：{error}")))?;
    Ok(target)
}

pub fn resolve_preview(artifact_dir: &Path, audio_mode: &str) -> Result<PreviewMedia, AppError> {
    let proxy = artifact_dir.join("preview-proxy.mp4");
    if !is_non_empty_file(&proxy) {
        return Err(AppError::Media("预览代理尚未生成".into()));
    }
    let voice = artifact_dir.join("chinese-voice.wav");
    if !is_non_empty_file(&voice) {
        return Ok(PreviewMedia {
            path: proxy.to_string_lossy().into_owned(),
            dubbed: false,
            revision: media_revision(&proxy),
        });
    }
    let dubbed = artifact_dir.join("dubbed-preview.mp4");
    let safe_background = artifact_dir.join(SAFE_BACKGROUND_FILE);
    let dependencies: Vec<&Path> = if audio_mode == "separate" {
        vec![proxy.as_path(), voice.as_path(), safe_background.as_path()]
    } else {
        vec![proxy.as_path(), voice.as_path()]
    };
    if dubbed_preview_is_current(artifact_dir, &dubbed, &dependencies, audio_mode) {
        return Ok(PreviewMedia {
            path: dubbed.to_string_lossy().into_owned(),
            dubbed: true,
            revision: media_revision(&dubbed),
        });
    }
    Ok(PreviewMedia {
        path: proxy.to_string_lossy().into_owned(),
        dubbed: false,
        revision: media_revision(&proxy),
    })
}

fn safe_preview_audio_filter() -> &'static str {
    "[2:a]aformat=sample_rates=48000:channel_layouts=stereo[background];\
     [1:a]aformat=sample_rates=48000:channel_layouts=stereo,asplit=2[voice_sc][voice_mix];\
     [background][voice_sc]sidechaincompress=threshold=0.015:ratio=12:attack=12:release=320[ducked];\
     [ducked][voice_mix]amix=inputs=2:duration=first:normalize=0,loudnorm=I=-16:TP=-1.0:LRA=7[aout]"
}

fn duck_preview_audio_filter() -> &'static str {
    "[0:a]aformat=sample_rates=48000:channel_layouts=stereo,volume=0.72[original];\
     [1:a]aformat=sample_rates=48000:channel_layouts=stereo,asplit=2[voice_sc][voice_mix];\
     [original][voice_sc]sidechaincompress=threshold=0.015:ratio=18:attack=12:release=360[ducked];\
     [ducked][voice_mix]amix=inputs=2:duration=first:normalize=0,loudnorm=I=-16:TP=-1.0:LRA=7[aout]"
}

fn dubbed_preview_is_current(
    artifact_dir: &Path,
    target: &Path,
    dependencies: &[&Path],
    audio_mode: &str,
) -> bool {
    if !is_non_empty_file(target) || !preview_is_current(target, dependencies) {
        return false;
    }
    std::fs::read_to_string(artifact_dir.join(DUBBED_PREVIEW_MANIFEST))
        .ok()
        .and_then(|value| serde_json::from_str::<DubbedPreviewManifest>(&value).ok())
        .is_some_and(|manifest| {
            manifest.audio_mode == audio_mode && manifest.recipe_version == PREVIEW_RECIPE_VERSION
        })
}

fn write_dubbed_preview_manifest(artifact_dir: &Path, audio_mode: &str) -> Result<(), AppError> {
    let manifest = DubbedPreviewManifest {
        audio_mode: audio_mode.to_string(),
        recipe_version: PREVIEW_RECIPE_VERSION,
    };
    let payload = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| AppError::Media(format!("无法序列化中文预览缓存信息：{error}")))?;
    std::fs::write(artifact_dir.join(DUBBED_PREVIEW_MANIFEST), payload)
        .map_err(|error| AppError::Media(format!("无法写入中文预览缓存信息：{error}")))
}

pub fn render_dubbed_preview(artifact_dir: &Path, audio_mode: &str) -> Result<PathBuf, AppError> {
    let proxy = artifact_dir.join("preview-proxy.mp4");
    let voice = artifact_dir.join("chinese-voice.wav");
    if !is_non_empty_file(&proxy) || !is_non_empty_file(&voice) {
        return Err(AppError::Media(
            "生成中文预览所需的代理视频或配音音轨缺失".into(),
        ));
    }
    let safe_background = artifact_dir.join(SAFE_BACKGROUND_FILE);
    if audio_mode == "separate" && !is_non_empty_file(&safe_background) {
        return Err(AppError::Media(
            "安全背景轨缺失；为避免英文残留，已阻止生成预览".into(),
        ));
    }
    let target = artifact_dir.join("dubbed-preview.mp4");
    let dependencies: Vec<&Path> = if audio_mode == "separate" {
        vec![proxy.as_path(), voice.as_path(), safe_background.as_path()]
    } else {
        vec![proxy.as_path(), voice.as_path()]
    };
    if preview_is_current(&target, &dependencies) {
        return Ok(target);
    }
    let temporary = artifact_dir.join("dubbed-preview.pending.mp4");
    let _ = std::fs::remove_file(&temporary);
    let result = if audio_mode == "separate" {
        render_preview_command_with_background(
            &proxy,
            &voice,
            &safe_background,
            &temporary,
            safe_preview_audio_filter(),
        )
    } else if audio_mode == "mute" {
        render_preview_command(
            &proxy,
            &voice,
            &temporary,
            "[1:a]aformat=sample_rates=48000:channel_layouts=stereo,loudnorm=I=-16:TP=-1.0:LRA=7[aout]",
        )
    } else {
        let primary_filter = duck_preview_audio_filter();
        render_preview_command(&proxy, &voice, &temporary, primary_filter).or_else(|_| {
            let _ = std::fs::remove_file(&temporary);
            render_preview_command(
                &proxy,
                &voice,
                &temporary,
                "[1:a]aformat=sample_rates=48000:channel_layouts=stereo,volume=1[aout]",
            )
        })
    };
    if let Err(error) = result {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    std::fs::rename(&temporary, &target)
        .map_err(|error| AppError::Media(format!("无法更新中文预览：{error}")))?;
    write_dubbed_preview_manifest(artifact_dir, audio_mode)?;
    Ok(target)
}

/// Prepare the best preview currently possible. An incomplete TTS run is a
/// normal product state, so a missing voice track falls back to the source
/// proxy instead of surfacing an error toast. Actual FFmpeg failures still
/// propagate once both dependencies are present.
pub fn prepare_preview(artifact_dir: &Path, audio_mode: &str) -> Result<PreviewMedia, AppError> {
    let current = resolve_preview(artifact_dir, audio_mode)?;
    let voice = artifact_dir.join("chinese-voice.wav");
    if !is_non_empty_file(&voice) {
        return Ok(current);
    }
    if current.dubbed {
        if audio_mode != "separate" {
            return Ok(current);
        }
        let safe_background = artifact_dir.join(SAFE_BACKGROUND_FILE);
        if is_non_empty_file(&safe_background)
            && dubbed_preview_is_current(
                artifact_dir,
                Path::new(&current.path),
                &[
                    proxy_path(artifact_dir).as_path(),
                    voice.as_path(),
                    safe_background.as_path(),
                ],
                audio_mode,
            )
        {
            return Ok(current);
        }
    }
    match render_dubbed_preview(artifact_dir, audio_mode) {
        Ok(path) => Ok(PreviewMedia {
            revision: media_revision(&path),
            path: path.to_string_lossy().into_owned(),
            dubbed: true,
        }),
        Err(AppError::Media(message)) if message == "生成中文预览所需的代理视频或配音音轨缺失" => {
            resolve_preview(artifact_dir, audio_mode)
        }
        Err(error) => Err(error),
    }
}

fn proxy_path(artifact_dir: &Path) -> PathBuf {
    artifact_dir.join("preview-proxy.mp4")
}

pub fn prepare_timeline_preview(
    artifact_dir: &Path,
    audio_mode: &str,
    source_duration_ms: i64,
    edits: &[TimelineEdit],
) -> Result<PreviewMedia, AppError> {
    let current = prepare_preview(artifact_dir, audio_mode)?;
    if !current.dubbed || !edits.iter().any(|edit| edit.accepted) {
        return Ok(current);
    }
    let map = TimelineMap::from_edits(source_duration_ms, edits)?;
    let target = artifact_dir.join("timeline-preview.mp4");
    let temporary = artifact_dir.join("timeline-preview.pending.mp4");
    if preview_is_current(&target, &[Path::new(&current.path)]) {
        return Ok(PreviewMedia {
            revision: media_revision(&target),
            path: target.to_string_lossy().into_owned(),
            dubbed: true,
        });
    }
    let edited = map.spans().iter().any(|span| span.operation != "keep");
    let (video_label, audio_label, filter) = crate::exporter::media_timeline_filter(&map, edited);
    let output = Command::new(resolve_binary("ffmpeg"))
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(&current.path)
        .args([
            "-filter_complex",
            &filter,
            "-map",
            video_label,
            "-map",
            audio_label,
            "-c:v",
            "h264_videotoolbox",
            "-b:v",
            "1800k",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-movflags",
            "+faststart",
        ])
        .arg(&temporary)
        .output()
        .map_err(|error| AppError::Media(format!("无法启动时间线预览：{error}")))?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&temporary);
        return Err(AppError::Media(format!(
            "时间线预览生成失败：{}",
            sanitize_process_error(&output.stderr)
        )));
    }
    std::fs::rename(&temporary, &target)
        .map_err(|error| AppError::Media(format!("无法更新时间线预览：{error}")))?;
    Ok(PreviewMedia {
        revision: media_revision(&target),
        path: target.to_string_lossy().into_owned(),
        dubbed: true,
    })
}

fn render_preview_command(
    proxy: &Path,
    voice: &Path,
    target: &Path,
    audio_filter: &str,
) -> Result<(), AppError> {
    let output = Command::new(resolve_binary("ffmpeg"))
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(proxy)
        .arg("-i")
        .arg(voice)
        .args([
            "-filter_complex",
            audio_filter,
            "-map",
            "0:v:0",
            "-map",
            "[aout]",
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            "-b:a",
            "160k",
            "-ar",
            "48000",
            "-movflags",
            "+faststart",
        ])
        .arg(target)
        .output()
        .map_err(|error| AppError::Media(format!("无法启动中文预览合成：{error}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(AppError::Media(format!(
            "中文预览合成失败：{}",
            sanitize_process_error(&output.stderr)
        )))
    }
}

fn render_preview_command_with_background(
    proxy: &Path,
    voice: &Path,
    background: &Path,
    target: &Path,
    audio_filter: &str,
) -> Result<(), AppError> {
    let output = Command::new(resolve_binary("ffmpeg"))
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(proxy)
        .arg("-i")
        .arg(voice)
        .arg("-i")
        .arg(background)
        .args([
            "-filter_complex",
            audio_filter,
            "-map",
            "0:v:0",
            "-map",
            "[aout]",
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            "-b:a",
            "160k",
            "-ar",
            "48000",
            "-movflags",
            "+faststart",
        ])
        .arg(target)
        .output()
        .map_err(|error| AppError::Media(format!("无法启动安全模式预览合成：{error}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(AppError::Media(format!(
            "安全模式预览合成失败：{}",
            sanitize_process_error(&output.stderr)
        )))
    }
}

fn preview_is_current(target: &Path, dependencies: &[&Path]) -> bool {
    let Ok(target_time) = target.metadata().and_then(|metadata| metadata.modified()) else {
        return false;
    };
    dependencies.iter().all(|path| {
        path.metadata()
            .and_then(|metadata| metadata.modified())
            .map(|modified| modified <= target_time)
            .unwrap_or(false)
    })
}

fn is_non_empty_file(path: &Path) -> bool {
    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false)
}

fn media_revision(path: &Path) -> u64 {
    path.metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or_default()
}

fn run_ffmpeg<const N: usize>(
    source: &Path,
    target: &Path,
    args: [&str; N],
) -> Result<(), AppError> {
    let output = Command::new(resolve_binary("ffmpeg"))
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(source)
        .args(args)
        .arg(target)
        .output()
        .map_err(|error| AppError::Media(format!("无法启动 ffmpeg：{error}")))?;
    if !output.status.success() {
        let _ = std::fs::remove_file(target);
        return Err(AppError::Media(sanitize_process_error(&output.stderr)));
    }
    Ok(())
}

fn resolve_binary(name: &str) -> PathBuf {
    for root in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        let candidate = Path::new(root).join(name);
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from(name)
}

fn fingerprint(path: &Path, metadata: &std::fs::Metadata) -> Result<String, AppError> {
    let mut file = File::open(path).map_err(|error| AppError::Media(error.to_string()))?;
    let mut hasher = Sha256::new();
    hasher.update(metadata.len().to_le_bytes());
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_secs())
        .unwrap_or_default();
    hasher.update(modified.to_le_bytes());
    let mut buffer = vec![0u8; 1024 * 1024];
    let head = file
        .read(&mut buffer)
        .map_err(|error| AppError::Media(error.to_string()))?;
    hasher.update(&buffer[..head]);
    if metadata.len() > buffer.len() as u64 {
        file.seek(SeekFrom::End(-(buffer.len() as i64)))
            .map_err(|error| AppError::Media(error.to_string()))?;
        let tail = file
            .read(&mut buffer)
            .map_err(|error| AppError::Media(error.to_string()))?;
        hasher.update(&buffer[..tail]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn sanitize_process_error(stderr: &[u8]) -> String {
    let value = String::from_utf8_lossy(stderr);
    value.lines().take(4).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::{
        ensure_thumbnail, prepare, prepare_preview, probe, resolve_binary, resolve_preview,
        safe_background_is_ready, safe_preview_audio_filter, sanitize_process_error,
        DubbedPreviewManifest, SafeBackgroundManifest, DUBBED_PREVIEW_MANIFEST,
        PREVIEW_RECIPE_VERSION, SAFE_BACKGROUND_FILE, SAFE_BACKGROUND_MANIFEST, SEPARATION_MODEL,
        SEPARATION_RUNTIME_VERSION,
    };
    use std::io::Write;
    use std::process::Command;

    #[test]
    fn process_error_is_bounded() {
        let error = sanitize_process_error(b"one\ntwo\nthree\nfour\nfive");
        assert!(!error.contains("five"));
    }

    #[test]
    fn safe_background_cache_requires_matching_source_and_model_manifest() {
        let output = std::env::temp_dir().join(format!(
            "yisheng-safe-background-cache-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(output.join(SAFE_BACKGROUND_FILE), b"background").unwrap();
        let manifest = SafeBackgroundManifest {
            source_fingerprint: "source-a".into(),
            model: SEPARATION_MODEL.into(),
            runtime_version: SEPARATION_RUNTIME_VERSION.into(),
        };
        std::fs::write(
            output.join(SAFE_BACKGROUND_MANIFEST),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();

        assert!(safe_background_is_ready(&output, "source-a"));
        assert!(!safe_background_is_ready(&output, "source-b"));
        std::fs::remove_dir_all(output).unwrap();
    }

    #[test]
    fn resolving_preview_never_renders_on_the_read_path() {
        let output =
            std::env::temp_dir().join(format!("yisheng-preview-read-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&output).unwrap();
        for name in ["preview-proxy.mp4", "chinese-voice.wav"] {
            let mut file = std::fs::File::create(output.join(name)).unwrap();
            file.write_all(b"fixture").unwrap();
        }
        let preview = resolve_preview(&output, "duck").unwrap();
        assert!(!preview.dubbed);
        assert!(preview.path.ends_with("preview-proxy.mp4"));
        assert!(!output.join("dubbed-preview.mp4").exists());
        std::fs::remove_dir_all(output).unwrap();
    }

    #[test]
    fn preparing_preview_without_a_voice_track_quietly_uses_the_proxy() {
        let output =
            std::env::temp_dir().join(format!("yisheng-preview-no-voice-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&output).unwrap();
        let mut proxy = std::fs::File::create(output.join("preview-proxy.mp4")).unwrap();
        proxy.write_all(b"proxy").unwrap();
        drop(proxy);

        let preview = prepare_preview(&output, "mix").unwrap();
        assert!(!preview.dubbed);
        assert!(preview.path.ends_with("preview-proxy.mp4"));
        assert!(!output.join("dubbed-preview.mp4").exists());
        std::fs::remove_dir_all(output).unwrap();
    }

    #[test]
    fn resolving_preview_reuses_a_current_dubbed_file_without_touching_it() {
        let output =
            std::env::temp_dir().join(format!("yisheng-preview-current-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&output).unwrap();
        for name in ["preview-proxy.mp4", "chinese-voice.wav"] {
            let mut file = std::fs::File::create(output.join(name)).unwrap();
            file.write_all(b"dependency").unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        let target = output.join("dubbed-preview.mp4");
        let mut file = std::fs::File::create(&target).unwrap();
        file.write_all(b"dubbed").unwrap();
        drop(file);
        std::fs::write(
            output.join(DUBBED_PREVIEW_MANIFEST),
            serde_json::to_vec(&DubbedPreviewManifest {
                audio_mode: "duck".into(),
                recipe_version: PREVIEW_RECIPE_VERSION,
            })
            .unwrap(),
        )
        .unwrap();
        let before = target.metadata().unwrap().modified().unwrap();
        let preview = resolve_preview(&output, "duck").unwrap();
        let after = target.metadata().unwrap().modified().unwrap();
        assert!(preview.dubbed);
        assert_eq!(preview.path, target.to_string_lossy());
        assert_eq!(before, after);
        std::fs::remove_dir_all(output).unwrap();
    }

    #[test]
    fn safe_preview_splits_voice_before_sidechain_and_mix() {
        let filter = safe_preview_audio_filter();
        assert!(filter.contains("asplit=2[voice_sc][voice_mix]"));
        assert!(filter.contains("[background][voice_sc]sidechaincompress"));
        assert!(filter.contains("[ducked][voice_mix]amix"));
        assert!(!filter.contains("[background][voice]sidechaincompress"));
    }

    #[test]
    fn resolving_preview_rejects_a_manifest_from_an_old_mix_recipe() {
        let output = std::env::temp_dir().join(format!(
            "yisheng-preview-old-recipe-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&output).unwrap();
        for name in [
            "preview-proxy.mp4",
            "chinese-voice.wav",
            SAFE_BACKGROUND_FILE,
        ] {
            std::fs::write(output.join(name), b"fixture").unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(output.join("dubbed-preview.mp4"), b"fixture").unwrap();
        std::fs::write(
            output.join(DUBBED_PREVIEW_MANIFEST),
            serde_json::to_vec(&DubbedPreviewManifest {
                audio_mode: "separate".into(),
                recipe_version: PREVIEW_RECIPE_VERSION - 1,
            })
            .unwrap(),
        )
        .unwrap();

        let preview = resolve_preview(&output, "separate").unwrap();
        assert!(!preview.dubbed);
        assert!(preview.path.ends_with("preview-proxy.mp4"));
        std::fs::remove_dir_all(output).unwrap();
    }

    #[test]
    fn project_cover_is_generated_from_video_and_reuses_the_cache() {
        let output = std::env::temp_dir().join(format!("yisheng-cover-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&output).unwrap();
        let source = output.join("source.mp4");
        let ffmpeg = Command::new(resolve_binary("ffmpeg"))
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=0x3366cc:s=160x90:d=0.2",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&source)
            .output()
            .expect("start ffmpeg fixture generation");
        assert!(ffmpeg.status.success());

        let cover = ensure_thumbnail("sample", &source, &output).unwrap();
        assert!(cover.ends_with("projects/sample/media/cover-first-frame.jpg"));
        assert!(cover.metadata().unwrap().len() > 100);
        let before = cover.metadata().unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let cached = ensure_thumbnail("sample", &source, &output).unwrap();
        assert_eq!(cover, cached);
        assert_eq!(before, cached.metadata().unwrap().modified().unwrap());
        std::fs::remove_dir_all(output).unwrap();
    }

    #[test]
    #[ignore = "requires YISHENG_MEDIA_SAMPLE and local FFmpeg"]
    fn prepares_real_media_sample() {
        let source = std::env::var("YISHENG_MEDIA_SAMPLE").expect("set YISHENG_MEDIA_SAMPLE");
        let source = std::path::Path::new(&source);
        let metadata = probe(source).unwrap();
        assert!(metadata.duration_ms > 1_000);
        assert!(metadata.width > 0 && metadata.height > 0);
        let output =
            std::env::temp_dir().join(format!("yisheng-media-qa-{}", uuid::Uuid::new_v4()));
        let mut stages = Vec::new();
        let artifacts = prepare(
            "sample",
            source,
            &output,
            "duck",
            &metadata.fingerprint,
            |stage, value, _| {
                stages.push((stage.to_string(), value));
                Ok(())
            },
        )
        .unwrap();
        assert!(
            std::path::Path::new(&artifacts.audio_path)
                .metadata()
                .unwrap()
                .len()
                > 44
        );
        assert!(
            std::path::Path::new(&artifacts.proxy_path)
                .metadata()
                .unwrap()
                .len()
                > 1_024
        );
        assert!(stages
            .iter()
            .any(|(stage, value)| stage == "proxy" && *value == 15));
        std::fs::remove_dir_all(output).unwrap();
    }
}
