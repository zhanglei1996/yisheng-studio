use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

use crate::{domain::SegmentRecord, error::AppError};

pub(crate) const MAX_SHORT_CLIP_TEMPO: f64 = 1.25;

#[derive(Debug)]
pub struct TtsOutput {
    pub track_path: PathBuf,
    pub warning_ids: Vec<String>,
}

#[cfg(test)]
pub fn synthesize(
    segments: &[SegmentRecord],
    artifact_dir: &Path,
    duration_ms: i64,
    progress: impl Fn(usize, usize),
) -> Result<TtsOutput, AppError> {
    synthesize_to(segments, artifact_dir, duration_ms, None, progress)
}

pub fn synthesize_to(
    segments: &[SegmentRecord],
    artifact_dir: &Path,
    duration_ms: i64,
    output_path: Option<&Path>,
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
            run(
                Command::new(resolve_ffmpeg())
                    .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
                    .arg(&raw)
                    .args([
                        "-af",
                        safe_edge_trim_filter(),
                        "-ac",
                        "1",
                        "-ar",
                        "48000",
                        "-c:a",
                        "pcm_s16le",
                    ])
                    .arg(&base),
                "配音音频转换失败",
            )?;
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
            let ratio = (actual_ms as f64 / target_ms as f64).min(1.08);
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
    let track_path = output_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| artifact_dir.join("chinese-voice.wav"));
    if !warning_ids.is_empty() {
        return Ok(TtsOutput {
            track_path,
            warning_ids,
        });
    }
    if output_path.is_some() {
        mix_track(&inputs, duration_ms, &track_path)?;
    } else {
        let pending_track = artifact_dir.join(format!(
            "chinese-voice.{}.pending.wav",
            uuid::Uuid::new_v4()
        ));
        mix_track(&inputs, duration_ms, &pending_track)?;
        std::fs::rename(&pending_track, &track_path)
            .map_err(|error| AppError::Media(format!("无法原子发布系统语音音轨：{error}")))?;
    }
    Ok(TtsOutput {
        track_path,
        warning_ids,
    })
}

fn spoken_cache_key(segment: &SegmentRecord) -> String {
    let mut digest = Sha256::new();
    digest.update(segment.spoken_zh.as_bytes());
    digest.update(b"\0Tingting\0");
    digest.update(b"200\0v2-safe-edge-trim");
    hex::encode(digest.finalize())[..16].to_string()
}

pub(crate) fn mix_track(
    inputs: &[(PathBuf, i64)],
    duration_ms: i64,
    target: &Path,
) -> Result<(), AppError> {
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

/// Rejects a publish layout where a synthesized clip would run into the next
/// visual anchor. `amix` deliberately supports overlap, so this validation has
/// to happen before FFmpeg; otherwise two narration blocks become two voices.
pub(crate) fn validate_non_overlapping_inputs(inputs: &[(PathBuf, i64)]) -> Result<(), AppError> {
    for pair in inputs.windows(2) {
        let (path, start_ms) = &pair[0];
        let next_start_ms = pair[1].1;
        let end_ms = *start_ms + duration_ms_of(path)?;
        if end_ms > next_start_ms + 20 {
            return Err(AppError::Validation(format!(
                "中文配音块在 {:.2} 秒处发生重叠，请先缩短口播稿或重新适配",
                next_start_ms as f64 / 1000.0
            )));
        }
    }
    Ok(())
}

/// Trim only the beginning and end. A positive `stop_periods` in a single
/// silenceremove pass stops at the first natural pause and can truncate the
/// remainder of a sentence, so the tail is trimmed by reversing the stream.
pub(crate) fn safe_edge_trim_filter() -> &'static str {
    "silenceremove=start_periods=1:start_duration=0.04:start_threshold=-60dB,areverse,silenceremove=start_periods=1:start_duration=0.06:start_threshold=-60dB,areverse"
}

pub(crate) fn plausible_min_duration_ms(text: &str) -> i64 {
    let mut units = 0.0_f64;
    let mut in_ascii_word = false;
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            if !in_ascii_word {
                units += 1.5;
                in_ascii_word = true;
            }
        } else {
            in_ascii_word = false;
            if !character.is_whitespace() && !character.is_ascii_punctuation() {
                units += 1.0;
            }
        }
    }
    (units * 65.0).round().max(300.0) as i64
}

pub(crate) fn validate_clip_completeness(text: &str, duration_ms: i64) -> Result<(), AppError> {
    let minimum = plausible_min_duration_ms(text);
    if duration_ms < minimum {
        return Err(AppError::Provider(format!(
            "语音服务返回的音频可能不完整（仅 {:.2} 秒），已阻止发布；请重试该片段",
            duration_ms as f64 / 1000.0
        )));
    }
    Ok(())
}

/// Bring very short translations closer to their source window without
/// forcing unnatural full-window stretching. The lower tempo bound preserves
/// voice identity while reducing the long silent tails between clips.
pub(crate) fn fit_clip_to_window(
    source: &Path,
    target: &Path,
    target_ms: i64,
) -> Result<i64, AppError> {
    let actual_ms = duration_ms_of(source)?;
    let usable_ms = (target_ms - 320).max(300);
    let tempo = if actual_ms > target_ms + 150 {
        (actual_ms as f64 / target_ms as f64).clamp(1.0, 1.08)
    } else if actual_ms < usable_ms && actual_ms * 100 < usable_ms * 78 {
        (actual_ms as f64 / usable_ms as f64).clamp(0.82, 1.0)
    } else {
        1.0
    };
    if (tempo - 1.0).abs() < 0.005 {
        return Ok(actual_ms);
    }
    run(
        Command::new(resolve_ffmpeg())
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(source)
            .args([
                "-af",
                &format!(
                    "atempo={tempo:.4},afade=t=in:d=0.015,areverse,afade=t=in:d=0.025,areverse"
                ),
                "-ac",
                "1",
                "-ar",
                "48000",
                "-c:a",
                "pcm_s16le",
                "-f",
                "wav",
            ])
            .arg(target),
        "配音自然时长适配失败",
    )?;
    duration_ms_of(target)
}

/// Continuous narration can tolerate a small, chapter-wide tempo adjustment.
/// Filling the chapter window here is materially less noticeable than leaving
/// a multi-second silent tail, and avoids the per-caption tempo changes that
/// make conventional dubbing sound assembled.
pub(crate) fn fit_narration_to_window(
    source: &Path,
    target: &Path,
    target_ms: i64,
) -> Result<i64, AppError> {
    let actual_ms = duration_ms_of(source)?;
    let usable_ms = (target_ms - 420).max(300);
    // Publishing-quality narration only tolerates subtle tempo correction.
    // Larger mismatches must be solved by rewriting or timeline editing.
    let tempo = (actual_ms as f64 / usable_ms as f64).clamp(0.94, 1.06);
    if (tempo - 1.0).abs() < 0.005 {
        return Ok(actual_ms);
    }
    run(
        Command::new(resolve_ffmpeg())
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(source)
            .args([
                "-af",
                &format!(
                    "atempo={tempo:.4},afade=t=in:d=0.015,areverse,afade=t=in:d=0.025,areverse"
                ),
                "-ac",
                "1",
                "-ar",
                "48000",
                "-c:a",
                "pcm_s16le",
                "-f",
                "wav",
            ])
            .arg(target),
        "连续旁白章节适配失败",
    )?;
    duration_ms_of(target)
}

pub(crate) fn force_fit_clip_to_window(
    source: &Path,
    target: &Path,
    target_ms: i64,
) -> Result<i64, AppError> {
    let actual_ms = duration_ms_of(source)?;
    if actual_ms <= target_ms + 150 {
        std::fs::copy(source, target).map_err(|error| AppError::Media(error.to_string()))?;
        return Ok(actual_ms);
    }
    let tempo = actual_ms as f64 / target_ms.max(1) as f64;
    let filters = atempo_chain(tempo);
    run(
        Command::new(resolve_ffmpeg())
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(source)
            .args([
                "-af",
                &format!("{filters},afade=t=in:d=0.015,areverse,afade=t=in:d=0.025,areverse"),
                "-ac",
                "1",
                "-ar",
                "48000",
                "-c:a",
                "pcm_s16le",
                "-f",
                "wav",
            ])
            .arg(target),
        "配音强制适配失败",
    )?;
    duration_ms_of(target)
}

fn atempo_chain(mut tempo: f64) -> String {
    let mut filters = Vec::new();
    while tempo > 2.0 {
        filters.push("atempo=2.0000".to_string());
        tempo /= 2.0;
    }
    filters.push(format!("atempo={:.4}", tempo.clamp(0.5, 2.0)));
    filters.join(",")
}

pub(crate) fn positioned_start_ms(start_ms: i64, end_ms: i64, audio_ms: i64) -> i64 {
    let free_ms = (end_ms - start_ms - audio_ms).max(0);
    start_ms + (free_ms * 35 / 100).min(600)
}

pub(crate) fn duration_ms_of(path: &Path) -> Result<i64, AppError> {
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
pub(crate) fn resolve_ffmpeg() -> PathBuf {
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
pub(crate) fn run(command: &mut Command, message: &str) -> Result<(), AppError> {
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

    #[test]
    fn completeness_guard_rejects_impossibly_short_chinese_audio() {
        assert!(validate_clip_completeness(
            "注意，我输入了 HTTP 地址，按回车后浏览器会自动跳转。",
            394
        )
        .is_err());
        assert!(validate_clip_completeness("注意，页面会自动跳转。", 1_500).is_ok());
    }

    #[test]
    fn positioning_reduces_large_trailing_gaps_without_large_sync_drift() {
        assert_eq!(positioned_start_ms(10_000, 18_000, 2_000), 10_600);
        assert_eq!(positioned_start_ms(10_000, 12_000, 1_800), 10_070);
    }

    #[test]
    fn publish_layout_rejects_overlapping_narration_blocks() {
        let dir = std::env::temp_dir().join(format!("yisheng-overlap-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("first.wav");
        let second = dir.join("second.wav");
        for (path, duration) in [(&first, "1.2"), (&second, "0.8")] {
            run(
                Command::new(resolve_ffmpeg())
                    .args(["-hide_banner", "-loglevel", "error", "-y"])
                    .args(["-f", "lavfi", "-i"])
                    .arg(format!("anullsrc=r=48000:cl=mono:d={duration}"))
                    .args(["-c:a", "pcm_s16le"])
                    .arg(path),
                "无法生成重叠守卫测试音频",
            )
            .unwrap();
        }
        let error = validate_non_overlapping_inputs(&[(first, 0), (second, 1_000)]).unwrap_err();
        assert!(error.to_string().contains("发生重叠"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn narration_fit_uses_one_chapter_wide_tempo_change() {
        let root =
            std::env::temp_dir().join(format!("yisheng-narration-fit-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("source.wav");
        let fitted = root.join("fitted.wav");
        run(
            Command::new(resolve_ffmpeg())
                .args(["-hide_banner", "-loglevel", "error", "-y"])
                .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=7.0"])
                .args(["-ar", "48000", "-ac", "1", "-c:a", "pcm_s16le"])
                .arg(&source),
            "无法生成章节适配测试音频",
        )
        .unwrap();
        let fitted_ms = fit_narration_to_window(&source, &fitted, 8_000).unwrap();
        assert!((7_400..=7_500).contains(&fitted_ms));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn edge_trim_preserves_a_natural_pause_inside_the_sentence() {
        let root =
            std::env::temp_dir().join(format!("yisheng-safe-edge-trim-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("source.wav");
        let trimmed = root.join("trimmed.wav");
        run(
            Command::new(resolve_ffmpeg())
                .args(["-hide_banner", "-loglevel", "error", "-y"])
                .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=0.25"])
                .args(["-f", "lavfi", "-i", "anullsrc=r=48000:cl=mono:d=0.40"])
                .args(["-f", "lavfi", "-i", "sine=frequency=660:duration=0.25"])
                .args([
                    "-filter_complex",
                    "[0:a][1:a][2:a]concat=n=3:v=0:a=1[out]",
                    "-map",
                    "[out]",
                    "-ar",
                    "48000",
                    "-ac",
                    "1",
                    "-c:a",
                    "pcm_s16le",
                ])
                .arg(&source),
            "无法生成内部停顿测试音频",
        )
        .unwrap();
        run(
            Command::new(resolve_ffmpeg())
                .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
                .arg(&source)
                .args(["-af", safe_edge_trim_filter(), "-c:a", "pcm_s16le"])
                .arg(&trimmed),
            "安全边缘裁剪测试失败",
        )
        .unwrap();

        // The edge detector may remove the sine wave's short fade-in/out, but
        // the result must still include both tones and the 400 ms inner pause.
        assert!(duration_ms_of(&trimmed).unwrap() >= 750);
        let _ = std::fs::remove_dir_all(root);
    }
}
