use crate::{domain::SegmentRecord, error::AppError};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    process::Command,
};
use uuid::Uuid;

#[derive(Deserialize)]
struct WhisperJson {
    transcription: Vec<WhisperSegment>,
}
#[derive(Deserialize)]
struct WhisperSegment {
    timestamps: WhisperTimestamps,
    text: String,
}
#[derive(Deserialize)]
struct WhisperTimestamps {
    from: String,
    to: String,
}

pub fn transcribe(
    project_id: &str,
    audio: &Path,
    model: &Path,
    output_dir: &Path,
    runtime_root: &Path,
) -> Result<Vec<SegmentRecord>, AppError> {
    let binary = resolve_binary(runtime_root)
        .ok_or_else(|| AppError::Media("未安装 whisper.cpp，请先在设置 → 运行时组件安装".into()))?;
    if !model.is_file() {
        return Err(AppError::Media("未安装 Whisper small.en 模型".into()));
    }
    let prefix = output_dir.join("source-asr");
    let output = Command::new(binary)
        .args(["-m"])
        .arg(model)
        .args(["-f"])
        .arg(audio)
        .args(["-l", "en", "-oj", "-of"])
        .arg(&prefix)
        .args(["-t", "8"])
        .output()
        .map_err(|e| AppError::Media(format!("无法启动 whisper.cpp：{e}")))?;
    if !output.status.success() {
        return Err(AppError::Media(
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .rev()
                .take(4)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join(" "),
        ));
    }
    let value: WhisperJson = serde_json::from_slice(
        &std::fs::read(prefix.with_extension("json"))
            .map_err(|e| AppError::Media(e.to_string()))?,
    )
    .map_err(|e| AppError::Media(format!("识别结果无法解析：{e}")))?;
    Ok(value
        .transcription
        .into_iter()
        .enumerate()
        .filter_map(|(i, item)| {
            let start = parse_time(&item.timestamps.from)?;
            let end = parse_time(&item.timestamps.to)?;
            (end - start >= 300).then(|| SegmentRecord {
                id: Uuid::new_v4().to_string(),
                project_id: project_id.into(),
                ordinal: i as i64,
                start_ms: start,
                end_ms: end,
                source_text: item.text.trim().into(),
                subtitle_zh: String::new(),
                spoken_zh: String::new(),
                linked: true,
                status: "ready".into(),
                script_doc_json: String::new(),
                script_revision: 1,
                tts_overrides_json: "{}".into(),
                tts_state: "missing".into(),
                tts_error_message: None,
                tts_settings_hash: None,
                tts_duration_ms: None,
            })
        })
        .collect())
}

fn resolve_binary(root: &Path) -> Option<PathBuf> {
    [
        root.join("whisper-cpp/whisper-cli"),
        root.join("whisper-cpp-v1.9.2/whisper-cli"),
        PathBuf::from("/opt/homebrew/bin/whisper-cli"),
        PathBuf::from("/usr/local/bin/whisper-cli"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}
fn parse_time(value: &str) -> Option<i64> {
    let clean = value.replace(',', ".");
    let parts = clean.split(':').collect::<Vec<_>>();
    if parts.len() != 3 {
        return None;
    }
    Some(
        (parts[0].parse::<f64>().ok()? * 3_600_000.0
            + parts[1].parse::<f64>().ok()? * 60_000.0
            + parts[2].parse::<f64>().ok()? * 1000.0)
            .round() as i64,
    )
}

#[cfg(test)]
mod tests {
    use super::parse_time;
    #[test]
    fn parses_whisper_time() {
        assert_eq!(parse_time("00:01:02,340"), Some(62_340));
    }
}
