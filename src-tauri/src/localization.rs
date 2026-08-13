use crate::{
    domain::{
        LocalizationAnalysis, NarrationScene, NonSpeechEvent, SegmentRecord, SyncAnchor,
        TimelineEdit,
    },
    error::AppError,
    timeline_map::TimelineMap,
    visual_analysis::StaticInterval,
};
use uuid::Uuid;

const MIN_SCENE_MS: i64 = 12_000;
const TARGET_SCENE_MS: i64 = 22_000;
const MAX_SCENE_MS: i64 = 30_000;

pub fn analyze(
    project_id: &str,
    segments: &[SegmentRecord],
    source_duration_ms: i64,
    static_intervals: &[StaticInterval],
) -> Result<LocalizationAnalysis, AppError> {
    let events = non_speech_events(project_id, segments);
    let spoken = segments
        .iter()
        .filter(|segment| !is_non_speech_text(&segment.source_text))
        .cloned()
        .collect::<Vec<_>>();
    let scenes = build_scenes(project_id, &spoken);
    let anchors = build_anchors(project_id, &scenes, &spoken);
    let edits = suggest_timeline_edits(
        project_id,
        &spoken,
        &events,
        source_duration_ms,
        static_intervals,
    );
    let map = TimelineMap::from_edits(source_duration_ms, &edits)?;
    Ok(LocalizationAnalysis {
        estimated_savings_ms: source_duration_ms - map.output_duration_ms(),
        output_duration_ms: map.output_duration_ms(),
        source_duration_ms,
        scenes,
        anchors,
        timeline_edits: edits,
        non_speech_events: events,
    })
}

pub fn is_non_speech_text(text: &str) -> bool {
    let normalized = text
        .trim()
        .to_ascii_lowercase()
        .replace(['[', ']', '(', ')', '（', '）', ' '], "");
    [
        "musicplaying",
        "music",
        "音乐播放中",
        "音乐",
        "applause",
        "掌声",
        "laughter",
        "笑声",
    ]
    .iter()
    .any(|marker| normalized == *marker)
}

/// Low-confidence fallback for providers that do not expose token timestamps.
pub fn estimate_word_timings(text: &str, duration_ms: i64) -> Vec<crate::domain::WordTiming> {
    let units = text
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<Vec<_>>();
    if units.is_empty() || duration_ms <= 0 {
        return Vec::new();
    }
    units
        .iter()
        .enumerate()
        .map(|(index, character)| crate::domain::WordTiming {
            text: character.to_string(),
            start_ms: duration_ms * index as i64 / units.len() as i64,
            end_ms: duration_ms * (index + 1) as i64 / units.len() as i64,
            confidence: 0.45,
        })
        .collect()
}

fn event_kind(text: &str) -> &'static str {
    let lower = text.to_ascii_lowercase();
    if lower.contains("music") || text.contains("音乐") {
        "music"
    } else if lower.contains("applause") || text.contains("掌声") {
        "applause"
    } else if lower.contains("laughter") || text.contains("笑声") {
        "ambience"
    } else {
        "unknown"
    }
}

fn non_speech_events(project_id: &str, segments: &[SegmentRecord]) -> Vec<NonSpeechEvent> {
    segments
        .iter()
        .filter(|segment| is_non_speech_text(&segment.source_text))
        .map(|segment| NonSpeechEvent {
            id: format!("event-{}", segment.id),
            project_id: project_id.into(),
            source_start_ms: segment.start_ms,
            source_end_ms: segment.end_ms,
            kind: event_kind(&segment.source_text).into(),
            label: if segment.subtitle_zh.trim().is_empty() {
                segment.source_text.trim().into()
            } else {
                segment.subtitle_zh.trim().into()
            },
            confidence: 1.0,
        })
        .collect()
}

fn build_scenes(project_id: &str, segments: &[SegmentRecord]) -> Vec<NarrationScene> {
    if segments.is_empty() {
        return Vec::new();
    }
    let mut groups = Vec::<Vec<&SegmentRecord>>::new();
    let mut current = Vec::<&SegmentRecord>::new();
    for segment in segments {
        let should_split = current.last().is_some_and(|last| {
            let span = last.end_ms - current[0].start_ms;
            let next_span = segment.end_ms - current[0].start_ms;
            let gap = segment.start_ms - last.end_ms;
            next_span > MAX_SCENE_MS
                || (span >= TARGET_SCENE_MS && gap >= 700)
                || (span >= MIN_SCENE_MS && gap >= 2_400)
                || (span >= MIN_SCENE_MS && ends_thought(&last.spoken_zh) && gap >= 1_300)
        });
        if should_split && !current.is_empty() {
            groups.push(std::mem::take(&mut current));
        }
        current.push(segment);
    }
    if !current.is_empty() {
        groups.push(current);
    }

    groups
        .into_iter()
        .enumerate()
        .map(|(ordinal, group)| {
            let start = group[0].start_ms;
            let end = group.last().map_or(start, |segment| segment.end_ms);
            NarrationScene {
                id: format!("scene-{}", Uuid::new_v4()),
                project_id: project_id.into(),
                ordinal: ordinal as i64,
                source_start_ms: start,
                source_end_ms: end,
                segment_ids: group.iter().map(|segment| segment.id.clone()).collect(),
                subtitle_zh: join_natural(group.iter().map(|segment| segment.subtitle_zh.as_str())),
                spoken_zh: join_natural(group.iter().map(|segment| segment.spoken_zh.as_str())),
                duration_budget_ms: (end - start - 420).max(1_000),
                status: "ready".into(),
                revision: 1,
            }
        })
        .collect()
}

fn ends_thought(text: &str) -> bool {
    text.trim_end().ends_with(['。', '！', '？', '.', '!', '?'])
}

fn join_natural<'a>(parts: impl Iterator<Item = &'a str>) -> String {
    let mut result = String::new();
    for part in parts.map(str::trim).filter(|part| !part.is_empty()) {
        if !result.is_empty()
            && !result.ends_with(['。', '！', '？', '，', '；', '.', '!', '?', ',', ';'])
        {
            result.push('，');
        }
        result.push_str(part);
    }
    result
}

fn build_anchors(
    project_id: &str,
    scenes: &[NarrationScene],
    segments: &[SegmentRecord],
) -> Vec<SyncAnchor> {
    let by_id = segments
        .iter()
        .map(|segment| (segment.id.as_str(), segment))
        .collect::<std::collections::HashMap<_, _>>();
    let mut anchors = Vec::new();
    for scene in scenes {
        let scene_segments = scene
            .segment_ids
            .iter()
            .filter_map(|id| by_id.get(id.as_str()).copied())
            .collect::<Vec<_>>();
        for (index, segment) in scene_segments.iter().enumerate() {
            let action =
                contains_action(&segment.source_text) || contains_action(&segment.subtitle_zh);
            let is_edge = index == 0 || index + 1 == scene_segments.len();
            if !action && !is_edge {
                continue;
            }
            let priority = if action { "exact" } else { "near" };
            anchors.push(SyncAnchor {
                id: format!("anchor-{}", segment.id),
                project_id: project_id.into(),
                scene_id: scene.id.clone(),
                source_time_ms: segment.start_ms,
                phrase: anchor_phrase(&segment.spoken_zh),
                kind: if action { "action" } else { "speech" }.into(),
                priority: priority.into(),
                tolerance_ms: if priority == "exact" { 500 } else { 1_200 },
                confidence: if action { 0.86 } else { 0.72 },
                locked: false,
            });
        }
    }
    anchors
}

fn contains_action(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "click", "open", "launch", "run", "start", "type", "select", "页面", "打开", "点击",
        "运行", "启动", "输入", "选择", "结果", "显示", "切换",
    ]
    .iter()
    .any(|word| lower.contains(word))
}

fn anchor_phrase(text: &str) -> String {
    let clean = text.trim();
    clean.chars().take(18).collect()
}

fn suggest_timeline_edits(
    project_id: &str,
    segments: &[SegmentRecord],
    events: &[NonSpeechEvent],
    source_duration_ms: i64,
    static_intervals: &[StaticInterval],
) -> Vec<TimelineEdit> {
    let mut candidates = Vec::new();
    for pair in segments.windows(2) {
        let gap_start = pair[0].end_ms;
        let gap_end = pair[1].start_ms;
        let gap = gap_end - gap_start;
        if gap < 1_200 {
            continue;
        }
        if events
            .iter()
            .any(|event| event.source_start_ms < gap_end && event.source_end_ms > gap_start)
        {
            continue;
        }
        let preserve = if gap <= 3_000 { 520 } else { 800 };
        let removable = gap - preserve;
        if removable < 650 {
            continue;
        }
        let static_overlap_ms = static_intervals
            .iter()
            .map(|interval| {
                (interval.end_ms.min(gap_end) - interval.start_ms.max(gap_start)).max(0)
            })
            .max()
            .unwrap_or(0);
        let visually_safe = static_overlap_ms >= removable.min(800);
        let operation = if gap <= 3_000 && visually_safe {
            "cut"
        } else {
            "speed"
        };
        let output_duration = if operation == "cut" {
            0
        } else {
            preserve.max(gap / 3)
        };
        candidates.push(TimelineEdit {
            id: format!("edit-{}", Uuid::new_v4()),
            project_id: project_id.into(),
            source_start_ms: gap_start + preserve / 2,
            source_end_ms: if operation == "cut" {
                gap_end - preserve / 2
            } else {
                gap_end
            },
            operation: operation.into(),
            rate: (operation == "speed").then_some(
                ((gap - preserve / 2) as f64 / output_duration.max(1) as f64).clamp(1.0, 3.0),
            ),
            output_duration_ms: output_duration,
            origin: "automatic".into(),
            reason: if operation == "cut" {
                "语义停顿与静止画面重合，保留呼吸时间后可安全裁剪".into()
            } else if visually_safe {
                "长等待区间以静止画面为主，建议加速并保留操作因果".into()
            } else {
                "检测到语义停顿，但画面仍有变化；仅建议温和加速".into()
            },
            confidence: if operation == "cut" {
                0.92
            } else if visually_safe {
                0.82
            } else {
                0.62
            },
            accepted: false,
            revision: 1,
        });
    }
    if let Some(last) = segments.last() {
        let trailing = source_duration_ms - last.end_ms;
        if trailing > 3_000 && !events.iter().any(|event| event.source_end_ms > last.end_ms) {
            candidates.push(TimelineEdit {
                id: format!("edit-{}", Uuid::new_v4()),
                project_id: project_id.into(),
                source_start_ms: last.end_ms + 800,
                source_end_ms: source_duration_ms,
                operation: "cut".into(),
                rate: None,
                output_duration_ms: 0,
                origin: "automatic".into(),
                reason: "片尾无语义内容且没有非语言事件，建议缩短".into(),
                confidence: 0.78,
                accepted: false,
                revision: 1,
            });
        }
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(id: &str, start: i64, end: i64, source: &str, spoken: &str) -> SegmentRecord {
        SegmentRecord {
            id: id.into(),
            project_id: "p".into(),
            ordinal: start,
            start_ms: start,
            end_ms: end,
            source_text: source.into(),
            subtitle_zh: spoken.into(),
            spoken_zh: spoken.into(),
            linked: true,
            status: "ready".into(),
            script_doc_json: String::new(),
            script_revision: 1,
            tts_overrides_json: "{}".into(),
            tts_state: "ready".into(),
            tts_error_message: None,
            tts_settings_hash: None,
            tts_duration_ms: None,
        }
    }

    #[test]
    fn music_is_an_event_and_never_a_narration_scene() {
        let segments = vec![
            segment("a", 0, 2_000, "Hello", "你好。"),
            segment("music", 2_000, 8_000, "[MUSIC PLAYING]", "[音乐播放中]"),
        ];
        let analysis = analyze("p", &segments, 8_000, &[]).unwrap();
        assert_eq!(analysis.non_speech_events.len(), 1);
        assert!(analysis
            .scenes
            .iter()
            .all(|scene| !scene.segment_ids.contains(&"music".into())));
    }

    #[test]
    fn long_gap_becomes_a_safe_speed_suggestion_not_an_automatic_cut() {
        let segments = vec![
            segment("a", 0, 2_000, "First", "第一步。"),
            segment("b", 8_000, 10_000, "Open the app", "打开应用。"),
        ];
        let analysis = analyze("p", &segments, 10_000, &[]).unwrap();
        assert_eq!(analysis.timeline_edits.len(), 1);
        assert_eq!(analysis.timeline_edits[0].operation, "speed");
        assert!(!analysis.timeline_edits[0].accepted);
    }

    #[test]
    fn short_static_gap_is_a_high_confidence_cut() {
        let segments = vec![
            segment("a", 0, 2_000, "First", "第一步。"),
            segment("b", 4_400, 6_000, "Open", "打开。"),
        ];
        let analysis = analyze(
            "p",
            &segments,
            6_000,
            &[StaticInterval {
                start_ms: 2_000,
                end_ms: 4_400,
            }],
        )
        .unwrap();
        assert_eq!(analysis.timeline_edits[0].operation, "cut");
        assert!(analysis.timeline_edits[0].confidence > 0.9);
    }

    #[test]
    fn estimated_word_timings_cover_the_audio_window() {
        let timings = estimate_word_timings("你好 世界", 1_000);
        assert_eq!(timings.len(), 4);
        assert_eq!(timings[0].start_ms, 0);
        assert_eq!(timings[3].end_ms, 1_000);
    }
}
