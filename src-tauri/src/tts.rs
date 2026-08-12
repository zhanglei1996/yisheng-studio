use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

use crate::{domain::SegmentRecord, error::AppError};

pub struct TtsOutput {
    pub track_path: PathBuf,
    pub warning_ids: Vec<String>,
}

pub fn synthesize(
    segments: &[SegmentRecord],
    artifact_dir: &Path,
    duration_ms: i64,
    progress: impl Fn(usize, usize),
) -> Result<TtsOutput, AppError> {
    if !cfg!(target_os = "macos") {
        return Err(AppError::Media("系统语音当前仅支持 macOS".into()));
    }
    let tts_dir = artifact_dir.join("tts");
    std::fs::create_dir_all(&tts_dir).map_err(|error| AppError::Media(error.to_string()))?;
    let mut inputs = Vec::new();
    let mut warning_ids = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        if segment.spoken_zh.trim().is_empty() {
            continue;
        }
        let cache_key = spoken_cache_key(segment);
        let raw = tts_dir.join(format!("{}-{cache_key}.aiff", segment.id));
        let base = tts_dir.join(format!("{}-{cache_key}-base.wav", segment.id));
        let clean = tts_dir.join(format!("{}-{cache_key}.wav", segment.id));
        let target_ms = segment.end_ms - segment.start_ms;
        if !base.is_file() || duration_ms_of(&base).is_err() {
            run(
                Command::new("/usr/bin/say")
                    .args(["-v", "Tingting", "-r", "200", "-o"])
                    .arg(&raw)
                    .arg(&segment.spoken_zh),
                "系统语音生成失败",
            )?;
            run(Command::new(resolve_ffmpeg()).args(["-hide_banner","-loglevel","error","-y","-i"]).arg(&raw).args(["-af","silenceremove=start_periods=1:start_duration=0.1:start_threshold=-60dB:stop_periods=1:stop_duration=0.3:stop_threshold=-60dB","-ac","1","-ar","48000","-c:a","pcm_s16le"]).arg(&base), "配音音频转换失败")?;
            if duration_ms_of(&base).is_err() {
                run(
                    Command::new(resolve_ffmpeg())
                        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
                        .arg(&raw)
                        .args(["-ac", "1", "-ar", "48000", "-c:a", "pcm_s16le"])
                        .arg(&base),
                    "配音音频兜底转换失败",
                )?;
            }
        }
        std::fs::copy(&base, &clean).map_err(|error| AppError::Media(error.to_string()))?;
        let mut actual_ms = duration_ms_of(&clean)?;
        if actual_ms > target_ms + 150 {
            let ratio = (actual_ms as f64 / target_ms as f64).min(1.15);
            let accelerated = tts_dir.join(format!("{}-{cache_key}-fit.wav", segment.id));
            run(
                Command::new(resolve_ffmpeg())
                    .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
                    .arg(&clean)
                    .args(["-af", &format!("atempo={ratio:.4}"), "-c:a", "pcm_s16le"])
                    .arg(&accelerated),
                "配音调速失败",
            )?;
            std::fs::rename(&accelerated, &clean)
                .map_err(|error| AppError::Media(error.to_string()))?;
            actual_ms = duration_ms_of(&clean)?;
        }
        if actual_ms > target_ms + 150 {
            warning_ids.push(segment.id.clone());
        } else {
            inputs.push((clean, segment.start_ms));
        }
        let _ = std::fs::remove_file(raw);
        progress(index + 1, segments.len());
    }
    let track_path = artifact_dir.join("chinese-voice.wav");
    mix_track(&inputs, duration_ms, &track_path)?;
    Ok(TtsOutput {
        track_path,
        warning_ids,
    })
}

fn spoken_cache_key(segment: &SegmentRecord) -> String {
    let mut digest = Sha256::new();
    digest.update(segment.spoken_zh.as_bytes());
    digest.update(b"\0Tingting\0");
    digest.update(b"200\0v1");
    hex::encode(digest.finalize())[..16].to_string()
}

fn mix_track(inputs: &[(PathBuf, i64)], duration_ms: i64, target: &Path) -> Result<(), AppError> {
    let mut command = Command::new(resolve_ffmpeg());
    command.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "lavfi",
        "-i",
        "anullsrc=r=48000:cl=mono",
    ]);
    for (path, _) in inputs {
        command.arg("-i").arg(path);
    }
    let mut filter = String::new();
    for (index, (_, start)) in inputs.iter().enumerate() {
        filter.push_str(&format!(
            "[{}:a]adelay={}|{}[a{}];",
            index + 1,
            start,
            start,
            index + 1
        ));
    }
    filter.push_str("[0:a]");
    for index in 0..inputs.len() {
        filter.push_str(&format!("[a{}]", index + 1));
    }
    filter.push_str(&format!(
        "amix=inputs={}:normalize=0:dropout_transition=0[out]",
        inputs.len() + 1
    ));
    command
        .args([
            "-filter_complex",
            &filter,
            "-map",
            "[out]",
            "-t",
            &format!("{:.3}", duration_ms as f64 / 1000.0),
            "-c:a",
            "pcm_s16le",
        ])
        .arg(target);
    run(&mut command, "中文音轨混合失败")
}

fn duration_ms_of(path: &Path) -> Result<i64, AppError> {
    let output = Command::new(resolve_ffprobe())
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=nokey=1:noprint_wrappers=1",
        ])
        .arg(path)
        .output()
        .map_err(|e| AppError::Media(e.to_string()))?;
    let seconds = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<f64>()
        .map_err(|_| AppError::Media("无法测量配音时长".into()))?;
    Ok((seconds * 1000.0).round() as i64)
}
fn resolve_ffmpeg() -> PathBuf {
    resolve("ffmpeg")
}
fn resolve_ffprobe() -> PathBuf {
    resolve("ffprobe")
}
fn resolve(name: &str) -> PathBuf {
    ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"]
        .iter()
        .map(|root| Path::new(root).join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}
fn run(command: &mut Command, message: &str) -> Result<(), AppError> {
    let output = command
        .output()
        .map_err(|e| AppError::Media(format!("{message}：{e}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(AppError::Media(format!(
            "{message}：{}",
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .take(3)
                .collect::<Vec<_>>()
                .join(" ")
        )))
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    fn segment() -> SegmentRecord {
        SegmentRecord {
            id: "tts-segment".into(),
            project_id: "project".into(),
            ordinal: 0,
            start_ms: 250,
            end_ms: 2_750,
            source_text: "Hello from the test video.".into(),
            subtitle_zh: "你好，这是测试视频。".into(),
            spoken_zh: "你好，这是测试视频。".into(),
            linked: true,
            status: "translated".into(),
        }
    }

    #[test]
    fn synthesizes_and_aligns_a_local_system_voice_track() {
        let root = std::env::temp_dir().join(format!("yisheng-tts-{}", uuid::Uuid::new_v4()));
        let output = synthesize(&[segment()], &root, 3_000, |_, _| {}).unwrap();
        assert!(output.warning_ids.is_empty());
        assert!(output.track_path.is_file());
        assert!((2_950..=3_050).contains(&duration_ms_of(&output.track_path).unwrap()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn cache_key_changes_when_spoken_copy_changes() {
        let first = segment();
        let mut second = first.clone();
        second.spoken_zh.push_str("新的内容");
        assert_ne!(spoken_cache_key(&first), spoken_cache_key(&second));
    }
}
