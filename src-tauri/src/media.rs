use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::Command,
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    domain::{MediaArtifacts, MediaProbe, PreviewMedia},
    error::AppError,
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
    mut progress: F,
) -> Result<MediaArtifacts, AppError>
where
    F: FnMut(&str, u8, &str) -> Result<(), AppError>,
{
    let artifact_dir = root.join("projects").join(project_id).join("media");
    std::fs::create_dir_all(&artifact_dir).map_err(|error| AppError::Media(error.to_string()))?;
    let audio_path = artifact_dir.join("source-16k-mono.wav");
    let proxy_path = artifact_dir.join("preview-proxy.mp4");

    if !is_non_empty_file(&audio_path) {
        progress("audio_extract", 4, "media:extracting-audio")?;
        run_ffmpeg(
            source,
            &audio_path,
            ["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"],
        )?;
    }
    progress("audio_extract", 8, "media:audio-ready")?;
    if !is_non_empty_file(&proxy_path) {
        progress("proxy", 9, "media:creating-proxy")?;
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
    progress("proxy", 15, "media:proxy-ready")?;

    Ok(MediaArtifacts {
        project_id: project_id.into(),
        proxy_path: proxy_path.to_string_lossy().into_owned(),
        audio_path: audio_path.to_string_lossy().into_owned(),
        artifact_dir: artifact_dir.to_string_lossy().into_owned(),
    })
}

pub fn resolve_preview(artifact_dir: &Path) -> Result<PreviewMedia, AppError> {
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
    if is_non_empty_file(&dubbed) && preview_is_current(&dubbed, &[&proxy, &voice]) {
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

pub fn render_dubbed_preview(artifact_dir: &Path, audio_mode: &str) -> Result<PathBuf, AppError> {
    let proxy = artifact_dir.join("preview-proxy.mp4");
    let voice = artifact_dir.join("chinese-voice.wav");
    if !is_non_empty_file(&proxy) || !is_non_empty_file(&voice) {
        return Err(AppError::Media(
            "生成中文预览所需的代理视频或配音音轨缺失".into(),
        ));
    }
    let target = artifact_dir.join("dubbed-preview.mp4");
    if preview_is_current(&target, &[&proxy, &voice]) {
        return Ok(target);
    }
    let temporary = artifact_dir.join("dubbed-preview.pending.mp4");
    let _ = std::fs::remove_file(&temporary);
    let primary_filter = if audio_mode == "mute" {
        "[1:a]volume=1[aout]"
    } else {
        "[0:a]volume=0.16[original];[original][1:a]amix=inputs=2:duration=first:normalize=0[aout]"
    };
    let result = render_preview_command(&proxy, &voice, &temporary, primary_filter).or_else(|_| {
        let _ = std::fs::remove_file(&temporary);
        render_preview_command(&proxy, &voice, &temporary, "[1:a]volume=1[aout]")
    });
    if let Err(error) = result {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    std::fs::rename(&temporary, &target)
        .map_err(|error| AppError::Media(format!("无法更新中文预览：{error}")))?;
    Ok(target)
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
    use super::{prepare, probe, resolve_preview, sanitize_process_error};
    use std::io::Write;

    #[test]
    fn process_error_is_bounded() {
        let error = sanitize_process_error(b"one\ntwo\nthree\nfour\nfive");
        assert!(!error.contains("five"));
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
        let preview = resolve_preview(&output).unwrap();
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
        let before = target.metadata().unwrap().modified().unwrap();
        let preview = resolve_preview(&output).unwrap();
        let after = target.metadata().unwrap().modified().unwrap();
        assert!(preview.dubbed);
        assert_eq!(preview.path, target.to_string_lossy());
        assert_eq!(before, after);
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
        let artifacts = prepare("sample", source, &output, |stage, value, _| {
            stages.push((stage.to_string(), value));
            Ok(())
        })
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
