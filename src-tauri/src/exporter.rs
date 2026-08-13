use crate::{
    domain::{ArtifactRecord, ProjectSummary, SegmentRecord, TimelineEdit},
    error::AppError,
    timeline_map::TimelineMap,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TtsBlockMetadata {
    start_ms: i64,
    end_ms: i64,
    duration_ms: i64,
    segment_ids: Vec<String>,
}

#[derive(Debug)]
struct TtsBlock {
    start_ms: i64,
    end_ms: i64,
    duration_ms: i64,
    audio_path: PathBuf,
    segment_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DubbingCue {
    start_ms: i64,
    end_ms: i64,
    source_text: String,
    spoken_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SilenceInterval {
    start_ms: i64,
    end_ms: i64,
}

pub fn export(
    project: &ProjectSummary,
    segments: &[SegmentRecord],
    tts_artifacts: &[ArtifactRecord],
    timeline_edits: &[TimelineEdit],
    output_root: &Path,
    subtitle_mode: &str,
    export_preset: &str,
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
    let source_duration_ms = project
        .duration_ms
        .unwrap_or_else(|| segments.last().map_or(0, |segment| segment.end_ms));
    let timeline_map = TimelineMap::from_edits(source_duration_ms, timeline_edits)?;
    let directory = available_export_directory(output_root, &safe_name(&project.name));
    std::fs::create_dir_all(&directory).map_err(|e| AppError::Media(e.to_string()))?;
    let en = directory.join("英文字幕.srt");
    let zh = directory.join("中文字幕.srt");
    let ass = directory.join("中英双语.ass");
    let dubbing_zh = directory.join("配音同步字幕.srt");
    let dubbing_ass = directory.join("配音同步双语.ass");
    std::fs::write(&en, mapped_srt(segments, false, &timeline_map))
        .map_err(|e| AppError::Media(e.to_string()))?;
    std::fs::write(&zh, mapped_srt(segments, true, &timeline_map))
        .map_err(|e| AppError::Media(e.to_string()))?;
    std::fs::write(&ass, mapped_ass_text(segments, &timeline_map))
        .map_err(|e| AppError::Media(e.to_string()))?;
    let dubbing_cues = dubbing_cues(segments, tts_artifacts);
    let dubbing_cues = if dubbing_cues.is_empty() {
        fallback_dubbing_cues(segments)
    } else {
        dubbing_cues
    };
    let dubbing_cues = map_and_stabilize_cues(&dubbing_cues, &timeline_map);
    std::fs::write(&dubbing_zh, dubbing_srt(&dubbing_cues))
        .map_err(|e| AppError::Media(e.to_string()))?;
    std::fs::write(&dubbing_ass, dubbing_ass_text(&dubbing_cues))
        .map_err(|e| AppError::Media(e.to_string()))?;
    let audio = directory.join("中文配音.wav");
    render_timeline_voice(&voice, &audio, &timeline_map)?;
    let video = directory.join("中文配音视频.mp4");
    let mut cmd = Command::new(ffmpeg());
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(&source)
        .arg("-i")
        .arg(&audio);
    let edited = timeline_map
        .spans()
        .iter()
        .any(|span| span.operation != "keep");
    let (video_label, original_audio_label, mut timeline_filter) =
        media_timeline_filter(&timeline_map, edited);
    let source_gain = if project.audio_mode == "mute" {
        0.42
    } else {
        0.72
    };
    timeline_filter.push_str(&format!(
        "{original_audio_label}volume={source_gain}[original_level];\
         [original_level][1:a]sidechaincompress=threshold=0.015:ratio=18:attack=12:release=360[ducked];\
         [ducked][1:a]amix=inputs=2:duration=longest:normalize=0,\
         loudnorm=I=-16:TP=-1.0:LRA=7[aout]"
    ));
    let mut video_output_label = video_label.to_string();
    if subtitle_mode != "none" {
        let subtitle = if subtitle_mode == "bilingual" {
            &dubbing_ass
        } else {
            &dubbing_zh
        };
        timeline_filter.push_str(&format!(
            ";{video_label}subtitles='{}'[vsub]",
            escape_filter_path(subtitle)
        ));
        video_output_label = "[vsub]".into();
    }
    cmd.args([
        "-filter_complex",
        &timeline_filter,
        "-map",
        &video_output_label,
        "-map",
        "[aout]",
    ]);
    if subtitle_mode == "none" && !edited {
        cmd.args(["-c:v", "copy"]);
    } else {
        let (bitrate, maxrate) = match export_preset {
            "share" => ("1800k", "3000k"),
            "high" => ("5000k", "8000k"),
            _ => ("2800k", "5000k"),
        };
        cmd.args([
            "-c:v",
            "h264_videotoolbox",
            "-b:v",
            bitrate,
            "-maxrate",
            maxrate,
            "-bufsize",
            "10000k",
        ]);
    }
    cmd.args([
        "-c:a",
        "aac",
        "-b:a",
        "192k",
        "-ac",
        "2",
        "-movflags",
        "+faststart",
    ])
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

pub(crate) fn media_timeline_filter(
    map: &TimelineMap,
    edited: bool,
) -> (&'static str, &'static str, String) {
    if !edited {
        return ("[0:v]", "[0:a]", String::new());
    }
    let mut filter = String::new();
    let mut video_inputs = String::new();
    let mut audio_inputs = String::new();
    let mut count = 0;
    for span in map.spans().iter().filter(|span| span.operation != "cut") {
        let start = span.source_start_ms as f64 / 1000.0;
        let end = span.source_end_ms as f64 / 1000.0;
        let rate = if span.operation == "speed" {
            span.rate
        } else {
            1.0
        };
        filter.push_str(&format!(
            "[0:v]trim=start={start:.3}:end={end:.3},setpts=(PTS-STARTPTS)/{rate:.5}[v{count}];\
             [0:a]atrim=start={start:.3}:end={end:.3},asetpts=PTS-STARTPTS,{}[o{count}];",
            atempo_filter(rate)
        ));
        video_inputs.push_str(&format!("[v{count}]"));
        audio_inputs.push_str(&format!("[o{count}]"));
        count += 1;
    }
    filter.push_str(&format!(
        "{video_inputs}concat=n={count}:v=1:a=0[vtime];\
         {audio_inputs}concat=n={count}:v=0:a=1[otime];"
    ));
    ("[vtime]", "[otime]", filter)
}

fn atempo_filter(mut rate: f64) -> String {
    let mut parts = Vec::new();
    while rate > 2.0 {
        parts.push("atempo=2.0".to_string());
        rate /= 2.0;
    }
    parts.push(format!("atempo={:.5}", rate.clamp(0.5, 2.0)));
    parts.join(",")
}

pub(crate) fn render_timeline_voice(
    source: &Path,
    target: &Path,
    map: &TimelineMap,
) -> Result<(), AppError> {
    let edited = map.spans().iter().any(|span| span.operation != "keep");
    if !edited {
        std::fs::copy(source, target).map_err(|error| AppError::Media(error.to_string()))?;
        return Ok(());
    }
    let mut filter = String::new();
    let mut inputs = String::new();
    let mut count = 0;
    for span in map.spans().iter().filter(|span| span.operation != "cut") {
        let start = span.source_start_ms as f64 / 1000.0;
        let end = span.source_end_ms as f64 / 1000.0;
        let rate = if span.operation == "speed" {
            span.rate
        } else {
            1.0
        };
        filter.push_str(&format!(
            "[0:a]atrim=start={start:.3}:end={end:.3},asetpts=PTS-STARTPTS,{}[a{count}];",
            atempo_filter(rate)
        ));
        inputs.push_str(&format!("[a{count}]"));
        count += 1;
    }
    filter.push_str(&format!("{inputs}concat=n={count}:v=0:a=1[out]"));
    let output = Command::new(ffmpeg())
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(source)
        .args([
            "-filter_complex",
            &filter,
            "-map",
            "[out]",
            "-ar",
            "48000",
            "-ac",
            "2",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(target)
        .output()
        .map_err(|error| AppError::Media(error.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(AppError::Media(format!(
            "无法应用配音时间编辑：{}",
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .take(3)
                .collect::<Vec<_>>()
                .join(" ")
        )))
    }
}

fn map_and_stabilize_cues(cues: &[DubbingCue], map: &TimelineMap) -> Vec<DubbingCue> {
    let mut mapped = cues
        .iter()
        .filter_map(|cue| {
            map.map_interval(cue.start_ms, cue.end_ms)
                .map(|(start_ms, end_ms)| DubbingCue {
                    start_ms,
                    end_ms,
                    source_text: cue.source_text.clone(),
                    spoken_text: cue.spoken_text.clone(),
                })
        })
        .collect::<Vec<_>>();
    let mut index = 0;
    while index < mapped.len() {
        if mapped[index].end_ms - mapped[index].start_ms < 800 && mapped.len() > 1 {
            if index > 0 {
                let short = mapped.remove(index);
                mapped[index - 1].end_ms = mapped[index - 1].end_ms.max(short.end_ms);
                mapped[index - 1].spoken_text.push_str(&short.spoken_text);
                mapped[index - 1].source_text.push(' ');
                mapped[index - 1].source_text.push_str(&short.source_text);
                continue;
            } else {
                let short = mapped.remove(index);
                mapped[0].start_ms = short.start_ms.min(mapped[0].start_ms);
                mapped[0].spoken_text = format!("{}{}", short.spoken_text, mapped[0].spoken_text);
                mapped[0].source_text = format!("{} {}", short.source_text, mapped[0].source_text);
                continue;
            }
        }
        index += 1;
    }
    remove_cue_overlaps(&mut mapped);
    mapped
}

fn mapped_srt(segments: &[SegmentRecord], chinese: bool, map: &TimelineMap) -> String {
    let mut out = String::new();
    let mut index = 1;
    for segment in segments {
        if let Some((start, end)) = map.map_interval(segment.start_ms, segment.end_ms) {
            let text = if chinese {
                &segment.subtitle_zh
            } else {
                &segment.source_text
            };
            let _ = writeln!(
                out,
                "{index}\n{} --> {}\n{}\n",
                srt_time(start),
                srt_time(end),
                text
            );
            index += 1;
        }
    }
    out
}

fn mapped_ass_text(segments: &[SegmentRecord], map: &TimelineMap) -> String {
    let mut out=String::from("[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\n\n[V4+ Styles]\nFormat: Name,Fontname,Fontsize,PrimaryColour,SecondaryColour,OutlineColour,BackColour,Bold,Italic,Underline,StrikeOut,ScaleX,ScaleY,Spacing,Angle,BorderStyle,Outline,Shadow,Alignment,MarginL,MarginR,MarginV,Encoding\nStyle: Default,PingFang SC,44,&H00FFFFFF,&H00FFFFFF,&H00101010,&H80000000,0,0,0,0,100,100,0,0,1,2,0,2,80,80,50,1\n\n[Events]\nFormat: Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text\n");
    for segment in segments {
        if let Some((start, end)) = map.map_interval(segment.start_ms, segment.end_ms) {
            let _ = writeln!(
                out,
                "Dialogue: 0,{},{},Default,,0,0,0,,{}\\N{}",
                ass_time(start),
                ass_time(end),
                segment.source_text.replace(',', "，"),
                segment.subtitle_zh.replace(',', "，")
            );
        }
    }
    out
}

fn dubbing_cues(segments: &[SegmentRecord], artifacts: &[ArtifactRecord]) -> Vec<DubbingCue> {
    let segment_by_id = segments
        .iter()
        .map(|segment| (segment.id.as_str(), segment))
        .collect::<HashMap<_, _>>();
    let mut blocks = artifacts
        .iter()
        .filter(|artifact| {
            matches!(
                artifact.kind.as_str(),
                "tts_semantic_anchored" | "tts_block_aligned" | "tts_narration_chapter"
            ) && artifact.status == "ready"
        })
        .filter_map(|artifact| {
            let metadata =
                serde_json::from_str::<TtsBlockMetadata>(&artifact.metadata_json).ok()?;
            let is_current = metadata.segment_ids.iter().all(|segment_id| {
                segment_by_id
                    .get(segment_id.as_str())
                    .and_then(|segment| segment.tts_settings_hash.as_deref())
                    == Some(artifact.dependency_hash.as_str())
            });
            let audio_path = PathBuf::from(&artifact.path);
            (is_current && audio_path.is_file()).then_some(TtsBlock {
                start_ms: metadata.start_ms,
                end_ms: metadata.end_ms,
                duration_ms: metadata.duration_ms,
                audio_path,
                segment_ids: metadata.segment_ids,
            })
        })
        .collect::<Vec<_>>();
    blocks.sort_by_key(|block| block.start_ms);

    let mut cues = Vec::new();
    for block in blocks {
        let block_segments = block
            .segment_ids
            .iter()
            .filter_map(|id| segment_by_id.get(id.as_str()).copied())
            .filter(|segment| !segment.spoken_zh.trim().is_empty())
            .collect::<Vec<_>>();
        if block_segments.is_empty() {
            continue;
        }
        cues.extend(cues_for_block(&block, &block_segments));
    }
    cues.sort_by_key(|cue| cue.start_ms);
    remove_cue_overlaps(&mut cues);
    cues
}

fn remove_cue_overlaps(cues: &mut [DubbingCue]) {
    for index in 1..cues.len() {
        if cues[index - 1].end_ms > cues[index].start_ms {
            cues[index - 1].end_ms = cues[index].start_ms;
        }
    }
}

fn cues_for_block(block: &TtsBlock, segments: &[&SegmentRecord]) -> Vec<DubbingCue> {
    let duration_ms = block.duration_ms.max(1);
    let silences = detect_silences(&block.audio_path, duration_ms);
    let audible_start = silences
        .iter()
        .find(|silence| silence.start_ms <= 40 && silence.end_ms < duration_ms)
        .map(|silence| silence.end_ms)
        .unwrap_or(0);
    let audible_end = silences
        .iter()
        .rev()
        .find(|silence| silence.end_ms >= duration_ms.saturating_sub(40))
        .map(|silence| silence.start_ms)
        .unwrap_or(duration_ms)
        .max(audible_start + 1);
    let weights = segments
        .iter()
        .map(|segment| caption_weight(&segment.spoken_zh))
        .collect::<Vec<_>>();
    let total_weight = weights.iter().sum::<f64>().max(1.0);
    let mut boundaries = Vec::with_capacity(segments.len() + 1);
    boundaries.push(audible_start);
    let minimum_cue_ms = ((audible_end - audible_start) / segments.len() as i64).clamp(1, 180);
    let mut cumulative = 0.0;
    for (index, weight) in weights.iter().enumerate().take(segments.len() - 1) {
        cumulative += weight;
        let raw = audible_start
            + (((audible_end - audible_start) as f64) * cumulative / total_weight).round() as i64;
        let remaining = segments.len() - index - 1;
        let lower = boundaries.last().copied().unwrap_or(audible_start) + minimum_cue_ms;
        let upper = (audible_end - (remaining as i64 * minimum_cue_ms)).max(lower);
        let snap_limit = 1_200_i64.max((audible_end - audible_start) / 6);
        let boundary = silences
            .iter()
            .filter(|silence| silence.end_ms - silence.start_ms >= 90)
            .map(|silence| (silence.start_ms + silence.end_ms) / 2)
            .filter(|candidate| *candidate >= lower && *candidate <= upper)
            .min_by_key(|candidate| (candidate - raw).abs())
            .filter(|candidate| (candidate - raw).abs() <= snap_limit)
            .unwrap_or(raw.clamp(lower, upper));
        boundaries.push(boundary);
    }
    boundaries.push(audible_end);

    let positioned_start = positioned_start_ms(block.start_ms, block.end_ms, duration_ms);
    segments
        .iter()
        .zip(boundaries.windows(2))
        .map(|(segment, window)| DubbingCue {
            start_ms: positioned_start + window[0],
            end_ms: positioned_start + window[1],
            source_text: segment.source_text.trim().to_owned(),
            spoken_text: segment.spoken_zh.trim().to_owned(),
        })
        .collect()
}

fn positioned_start_ms(window_start_ms: i64, window_end_ms: i64, audio_ms: i64) -> i64 {
    let free_ms = (window_end_ms - window_start_ms - audio_ms).max(0);
    window_start_ms + ((free_ms as f64 * 0.35).round() as i64).min(600)
}

fn caption_weight(text: &str) -> f64 {
    text.chars()
        .map(|character| {
            if character.is_whitespace() {
                0.0
            } else if "，。！？；：、,.!?;:".contains(character) {
                0.32
            } else if character.is_ascii_alphanumeric() {
                0.56
            } else {
                1.0
            }
        })
        .sum::<f64>()
        .max(1.0)
}

fn detect_silences(audio_path: &Path, duration_ms: i64) -> Vec<SilenceInterval> {
    let output = Command::new(ffmpeg())
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(audio_path)
        .args(["-af", "silencedetect=noise=-42dB:d=0.12", "-f", "null", "-"])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    parse_silence_log(&String::from_utf8_lossy(&output.stderr), duration_ms)
}

fn parse_silence_log(log: &str, duration_ms: i64) -> Vec<SilenceInterval> {
    let mut intervals = Vec::new();
    let mut pending_start = None;
    for line in log.lines() {
        if let Some(value) = log_value(line, "silence_start:") {
            pending_start = Some((value * 1_000.0).round() as i64);
        }
        if let Some(value) = log_value(line, "silence_end:") {
            let end_ms = ((value * 1_000.0).round() as i64).clamp(0, duration_ms);
            let start_ms = pending_start.take().unwrap_or(0).clamp(0, end_ms);
            intervals.push(SilenceInterval { start_ms, end_ms });
        }
    }
    if let Some(start_ms) = pending_start {
        intervals.push(SilenceInterval {
            start_ms: start_ms.clamp(0, duration_ms),
            end_ms: duration_ms,
        });
    }
    intervals
}

fn log_value(line: &str, marker: &str) -> Option<f64> {
    let value = line.split_once(marker)?.1.split_whitespace().next()?;
    value.parse().ok()
}

fn fallback_dubbing_cues(segments: &[SegmentRecord]) -> Vec<DubbingCue> {
    segments
        .iter()
        .filter(|segment| !segment.spoken_zh.trim().is_empty())
        .map(|segment| DubbingCue {
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            source_text: segment.source_text.trim().to_owned(),
            spoken_text: segment.spoken_zh.trim().to_owned(),
        })
        .collect()
}

fn dubbing_srt(cues: &[DubbingCue]) -> String {
    let mut out = String::new();
    for (index, cue) in cues.iter().enumerate() {
        let _ = writeln!(
            out,
            "{}\n{} --> {}\n{}\n",
            index + 1,
            srt_time(cue.start_ms),
            srt_time(cue.end_ms),
            cue.spoken_text
        );
    }
    out
}

fn dubbing_ass_text(cues: &[DubbingCue]) -> String {
    let mut out=String::from("[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\n\n[V4+ Styles]\nFormat: Name,Fontname,Fontsize,PrimaryColour,SecondaryColour,OutlineColour,BackColour,Bold,Italic,Underline,StrikeOut,ScaleX,ScaleY,Spacing,Angle,BorderStyle,Outline,Shadow,Alignment,MarginL,MarginR,MarginV,Encoding\nStyle: Default,PingFang SC,44,&H00FFFFFF,&H00FFFFFF,&H00101010,&H80000000,0,0,0,0,100,100,0,0,1,2,0,2,80,80,50,1\n\n[Events]\nFormat: Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text\n");
    for cue in cues {
        let source = cue.source_text.replace(',', "，");
        let spoken = cue.spoken_text.replace(',', "，");
        let _ = writeln!(
            out,
            "Dialogue: 0,{},{},Default,,0,0,0,,{}\\N{}",
            ass_time(cue.start_ms),
            ass_time(cue.end_ms),
            source,
            spoken
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
fn available_export_directory(output_root: &Path, project_name: &str) -> PathBuf {
    let preferred = output_root.join(project_name);
    if !preferred.exists() {
        return preferred;
    }
    (2..)
        .map(|version| output_root.join(format!("{project_name} ({version})")))
        .find(|candidate| !candidate.exists())
        .expect("export directory version space exhausted")
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
            script_doc_json: String::new(),
            script_revision: 1,
            tts_overrides_json: "{}".into(),
            tts_state: "ready".into(),
            tts_error_message: None,
            tts_settings_hash: None,
            tts_duration_ms: Some(1_500),
        }
    }

    #[test]
    fn writes_valid_english_chinese_and_bilingual_subtitles() {
        let segments = vec![segment()];
        let map = TimelineMap::from_edits(1_500, &[]).unwrap();
        let en = mapped_srt(&segments, false, &map);
        let zh = mapped_srt(&segments, true, &map);
        let bilingual = mapped_ass_text(&segments, &map);

        assert!(en.contains("00:00:00,000 --> 00:00:01,500"));
        assert!(en.contains("HTTP is upgraded to HTTPS."));
        assert!(zh.contains("HTTP 会升级为 HTTPS。"));
        assert!(bilingual.contains("HTTP is upgraded to HTTPS.\\NHTTP 会升级为 HTTPS。"));
    }

    #[test]
    fn sanitizes_project_names_for_export_directories() {
        assert_eq!(safe_name("TLS: lesson/01\\demo"), "TLS_ lesson_01_demo");
    }

    #[test]
    fn versions_existing_export_directories_instead_of_overwriting() {
        let root = std::env::temp_dir().join(format!("yisheng-export-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("课程")).expect("create first export directory");
        std::fs::create_dir_all(root.join("课程 (2)")).expect("create second export directory");

        assert_eq!(
            available_export_directory(&root, "课程"),
            root.join("课程 (3)")
        );

        std::fs::remove_dir_all(root).expect("remove test export root");
    }

    #[test]
    fn media_filter_applies_the_same_cut_to_video_and_original_audio() {
        let edit = TimelineEdit {
            id: "cut-gap".into(),
            project_id: "project".into(),
            source_start_ms: 2_000,
            source_end_ms: 4_000,
            operation: "cut".into(),
            rate: None,
            output_duration_ms: 0,
            origin: "user".into(),
            reason: "test".into(),
            confidence: 1.0,
            accepted: true,
            revision: 1,
        };
        let map = TimelineMap::from_edits(10_000, &[edit]).unwrap();
        let (video, audio, filter) = media_timeline_filter(&map, true);
        assert_eq!(video, "[vtime]");
        assert_eq!(audio, "[otime]");
        assert!(filter.contains("[0:v]trim=start=0.000:end=2.000"));
        assert!(filter.contains("[0:a]atrim=start=0.000:end=2.000"));
        assert!(filter.contains("[0:v]trim=start=4.000:end=10.000"));
        assert!(filter.contains("[0:a]atrim=start=4.000:end=10.000"));
        assert!(!filter.contains("start=2.000:end=4.000"));
    }

    #[test]
    fn parses_leading_internal_and_trailing_silences() {
        let log = r#"
[silencedetect @ 0x1] silence_start: 0
[silencedetect @ 0x1] silence_end: 0.21 | silence_duration: 0.21
[silencedetect @ 0x1] silence_start: 1.42
[silencedetect @ 0x1] silence_end: 1.71 | silence_duration: 0.29
[silencedetect @ 0x1] silence_start: 3.86
"#;
        assert_eq!(
            parse_silence_log(log, 4_000),
            vec![
                SilenceInterval {
                    start_ms: 0,
                    end_ms: 210
                },
                SilenceInterval {
                    start_ms: 1_420,
                    end_ms: 1_710
                },
                SilenceInterval {
                    start_ms: 3_860,
                    end_ms: 4_000
                }
            ]
        );
    }

    #[test]
    fn dubbing_cues_use_spoken_copy_and_tts_block_timeline() {
        let mut first = segment();
        first.id = "first".into();
        first.start_ms = 10_000;
        first.end_ms = 12_000;
        first.subtitle_zh = "逐字翻译一".into();
        first.spoken_zh = "自然口播的第一句".into();
        let mut second = segment();
        second.id = "second".into();
        second.start_ms = 12_000;
        second.end_ms = 15_000;
        second.subtitle_zh = "逐字翻译二".into();
        second.spoken_zh = "接着说第二句".into();
        let block = TtsBlock {
            start_ms: 10_000,
            end_ms: 15_000,
            duration_ms: 4_000,
            audio_path: PathBuf::from("/missing/test-audio.wav"),
            segment_ids: vec![first.id.clone(), second.id.clone()],
        };

        let cues = cues_for_block(&block, &[&first, &second]);
        let rendered = dubbing_srt(&cues);

        assert_eq!(cues.first().map(|cue| cue.start_ms), Some(10_350));
        assert_eq!(cues.last().map(|cue| cue.end_ms), Some(14_350));
        assert!(cues[0].end_ms <= cues[1].start_ms);
        assert!(rendered.contains("自然口播的第一句"));
        assert!(rendered.contains("接着说第二句"));
        assert!(!rendered.contains("逐字翻译"));
    }

    #[test]
    fn final_caption_timeline_removes_cross_block_overlap() {
        let mut cues = vec![
            DubbingCue {
                start_ms: 1_000,
                end_ms: 2_010,
                source_text: "one".into(),
                spoken_text: "第一句".into(),
            },
            DubbingCue {
                start_ms: 2_000,
                end_ms: 3_000,
                source_text: "two".into(),
                spoken_text: "第二句".into(),
            },
        ];
        remove_cue_overlaps(&mut cues);
        assert_eq!(cues[0].end_ms, cues[1].start_ms);
    }
}
