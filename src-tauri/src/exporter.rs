use crate::{
    domain::{ProjectSummary, SegmentRecord},
    error::AppError,
};
use serde::Serialize;
use std::{
    fmt::Write,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOutput {
    pub directory: String,
    pub video_path: String,
    pub audio_path: String,
}

pub fn export(
    project: &ProjectSummary,
    segments: &[SegmentRecord],
    output_root: &Path,
    subtitle_mode: &str,
) -> Result<ExportOutput, AppError> {
    let source = PathBuf::from(
        project
            .source_path
            .as_ref()
            .ok_or_else(|| AppError::Media("项目源视频丢失".into()))?,
    );
    let artifacts = PathBuf::from(
        project
            .artifact_dir
            .as_ref()
            .ok_or_else(|| AppError::Media("项目产物目录丢失".into()))?,
    );
    let voice = artifacts.join("chinese-voice.wav");
    if !voice.is_file() {
        return Err(AppError::Media("请先完成中文配音".into()));
    }
    let directory = output_root.join(safe_name(&project.name));
    std::fs::create_dir_all(&directory).map_err(|e| AppError::Media(e.to_string()))?;
    let en = directory.join("英文字幕.srt");
    let zh = directory.join("中文字幕.srt");
    let ass = directory.join("中英双语.ass");
    std::fs::write(&en, srt(segments, false)).map_err(|e| AppError::Media(e.to_string()))?;
    std::fs::write(&zh, srt(segments, true)).map_err(|e| AppError::Media(e.to_string()))?;
    std::fs::write(&ass, ass_text(segments)).map_err(|e| AppError::Media(e.to_string()))?;
    let audio = directory.join("中文配音.wav");
    std::fs::copy(&voice, &audio).map_err(|e| AppError::Media(e.to_string()))?;
    let video = directory.join("中文配音视频.mp4");
    let mut cmd = Command::new(ffmpeg());
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(&source)
        .arg("-i")
        .arg(&voice);
    let audio_filter = if project.audio_mode == "mute" {
        "[1:a]volume=1[aout]"
    } else {
        "[0:a]volume=0.16[original];[original][1:a]amix=inputs=2:duration=first:normalize=0[aout]"
    };
    cmd.args([
        "-filter_complex",
        audio_filter,
        "-map",
        "0:v:0",
        "-map",
        "[aout]",
    ]);
    if subtitle_mode == "none" {
        cmd.args(["-c:v", "copy"]);
    } else {
        let subtitle = if subtitle_mode == "bilingual" {
            &ass
        } else {
            &zh
        };
        cmd.args([
            "-vf",
            &format!("subtitles='{}'", escape_filter_path(subtitle)),
            "-c:v",
            "h264_videotoolbox",
            "-b:v",
            "6000k",
        ]);
    }
    cmd.args(["-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"])
        .arg(&video);
    let out = cmd.output().map_err(|e| AppError::Media(e.to_string()))?;
    if !out.status.success() {
        return Err(AppError::Media(
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .take(4)
                .collect::<Vec<_>>()
                .join(" "),
        ));
    }
    Ok(ExportOutput {
        directory: directory.to_string_lossy().into_owned(),
        video_path: video.to_string_lossy().into_owned(),
        audio_path: audio.to_string_lossy().into_owned(),
    })
}
fn srt(segments: &[SegmentRecord], chinese: bool) -> String {
    let mut out = String::new();
    for (i, s) in segments.iter().enumerate() {
        let text = if chinese {
            &s.subtitle_zh
        } else {
            &s.source_text
        };
        let _ = writeln!(
            out,
            "{}\n{} --> {}\n{}\n",
            i + 1,
            srt_time(s.start_ms),
            srt_time(s.end_ms),
            text
        );
    }
    out
}
fn ass_text(segments: &[SegmentRecord]) -> String {
    let mut out=String::from("[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\n\n[V4+ Styles]\nFormat: Name,Fontname,Fontsize,PrimaryColour,SecondaryColour,OutlineColour,BackColour,Bold,Italic,Underline,StrikeOut,ScaleX,ScaleY,Spacing,Angle,BorderStyle,Outline,Shadow,Alignment,MarginL,MarginR,MarginV,Encoding\nStyle: Default,PingFang SC,44,&H00FFFFFF,&H00FFFFFF,&H00101010,&H80000000,0,0,0,0,100,100,0,0,1,2,0,2,80,80,50,1\n\n[Events]\nFormat: Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text\n");
    for s in segments {
        let source = s.source_text.replace(',', "，");
        let zh = s.subtitle_zh.replace(',', "，");
        let _ = writeln!(
            out,
            "Dialogue: 0,{},{},Default,,0,0,0,,{}\\N{}",
            ass_time(s.start_ms),
            ass_time(s.end_ms),
            source,
            zh
        );
    }
    out
}
fn srt_time(ms: i64) -> String {
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        (ms / 60_000) % 60,
        (ms / 1000) % 60,
        ms % 1000
    )
}
fn ass_time(ms: i64) -> String {
    format!(
        "{}:{:02}:{:02}.{:02}",
        ms / 3_600_000,
        (ms / 60_000) % 60,
        (ms / 1000) % 60,
        (ms % 1000) / 10
    )
}
fn safe_name(s: &str) -> String {
    s.chars()
        .map(|c| if "/:\\".contains(c) { '_' } else { c })
        .collect()
}
fn escape_filter_path(p: &Path) -> String {
    p.to_string_lossy()
        .replace('\\', "\\\\")
        .replace(':', "\\:")
        .replace('\'', "\\'")
}
fn ffmpeg() -> PathBuf {
    [
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/usr/bin/ffmpeg",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|p| p.is_file())
    .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment() -> SegmentRecord {
        SegmentRecord {
            id: "segment-1".into(),
            project_id: "project-1".into(),
            ordinal: 0,
            start_ms: 0,
            end_ms: 1_500,
            source_text: "HTTP is upgraded to HTTPS.".into(),
            subtitle_zh: "HTTP 会升级为 HTTPS。".into(),
            spoken_zh: "HTTP 会升级为 HTTPS。".into(),
            linked: true,
            status: "tts_ready".into(),
        }
    }

    #[test]
    fn writes_valid_english_chinese_and_bilingual_subtitles() {
        let segments = vec![segment()];
        let en = srt(&segments, false);
        let zh = srt(&segments, true);
        let bilingual = ass_text(&segments);

        assert!(en.contains("00:00:00,000 --> 00:00:01,500"));
        assert!(en.contains("HTTP is upgraded to HTTPS."));
        assert!(zh.contains("HTTP 会升级为 HTTPS。"));
        assert!(bilingual.contains("HTTP is upgraded to HTTPS.\\NHTTP 会升级为 HTTPS。"));
    }

    #[test]
    fn sanitizes_project_names_for_export_directories() {
        assert_eq!(safe_name("TLS: lesson/01\\demo"), "TLS_ lesson_01_demo");
    }
}
