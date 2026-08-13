use crate::{domain::TimelineEdit, error::AppError};

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineSpan {
    pub source_start_ms: i64,
    pub source_end_ms: i64,
    pub output_start_ms: i64,
    pub output_end_ms: i64,
    pub operation: String,
    pub rate: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SourceMapping {
    Mapped(i64),
    Deleted { output_boundary_ms: i64 },
}

#[derive(Debug, Clone)]
pub struct TimelineMap {
    spans: Vec<TimelineSpan>,
    source_duration_ms: i64,
    output_duration_ms: i64,
}

impl TimelineMap {
    pub fn from_edits(source_duration_ms: i64, edits: &[TimelineEdit]) -> Result<Self, AppError> {
        if source_duration_ms < 0 {
            return Err(AppError::Validation("源视频时长不能为负数".into()));
        }
        let mut accepted = edits
            .iter()
            .filter(|edit| edit.accepted)
            .cloned()
            .collect::<Vec<_>>();
        accepted.sort_by_key(|edit| (edit.source_start_ms, edit.source_end_ms));
        let mut source_cursor = 0_i64;
        let mut output_cursor = 0_i64;
        let mut spans = Vec::new();
        for edit in accepted {
            validate_edit(&edit, source_duration_ms)?;
            if edit.source_start_ms < source_cursor {
                return Err(AppError::Validation("时间编辑区间发生重叠".into()));
            }
            if edit.source_start_ms > source_cursor {
                let duration = edit.source_start_ms - source_cursor;
                spans.push(TimelineSpan {
                    source_start_ms: source_cursor,
                    source_end_ms: edit.source_start_ms,
                    output_start_ms: output_cursor,
                    output_end_ms: output_cursor + duration,
                    operation: "keep".into(),
                    rate: 1.0,
                });
                output_cursor += duration;
            }
            let output_duration = if edit.operation == "cut" {
                0
            } else {
                edit.output_duration_ms
            };
            spans.push(TimelineSpan {
                source_start_ms: edit.source_start_ms,
                source_end_ms: edit.source_end_ms,
                output_start_ms: output_cursor,
                output_end_ms: output_cursor + output_duration,
                operation: edit.operation.clone(),
                rate: edit.rate.unwrap_or(1.0),
            });
            output_cursor += output_duration;
            source_cursor = edit.source_end_ms;
        }
        if source_cursor < source_duration_ms {
            let duration = source_duration_ms - source_cursor;
            spans.push(TimelineSpan {
                source_start_ms: source_cursor,
                source_end_ms: source_duration_ms,
                output_start_ms: output_cursor,
                output_end_ms: output_cursor + duration,
                operation: "keep".into(),
                rate: 1.0,
            });
            output_cursor += duration;
        }
        Ok(Self {
            spans,
            source_duration_ms,
            output_duration_ms: output_cursor,
        })
    }

    pub fn source_to_output(&self, source_ms: i64) -> SourceMapping {
        let source_ms = source_ms.clamp(0, self.source_duration_ms);
        let span = self
            .spans
            .iter()
            .find(|span| source_ms >= span.source_start_ms && source_ms < span.source_end_ms)
            .or_else(|| self.spans.last());
        let Some(span) = span else {
            return SourceMapping::Mapped(0);
        };
        if span.operation == "cut" {
            return SourceMapping::Deleted {
                output_boundary_ms: span.output_start_ms,
            };
        }
        let source_offset = source_ms.saturating_sub(span.source_start_ms);
        let source_duration = (span.source_end_ms - span.source_start_ms).max(1);
        let output_duration = span.output_end_ms - span.output_start_ms;
        SourceMapping::Mapped(
            span.output_start_ms + source_offset * output_duration / source_duration,
        )
    }

    #[cfg(test)]
    pub fn output_to_source(&self, output_ms: i64) -> i64 {
        let output_ms = output_ms.clamp(0, self.output_duration_ms);
        let span = self
            .spans
            .iter()
            .filter(|span| span.operation != "cut")
            .find(|span| output_ms >= span.output_start_ms && output_ms < span.output_end_ms)
            .or_else(|| self.spans.iter().rev().find(|span| span.operation != "cut"));
        let Some(span) = span else { return 0 };
        let output_offset = output_ms.saturating_sub(span.output_start_ms);
        let output_duration = (span.output_end_ms - span.output_start_ms).max(1);
        let source_duration = span.source_end_ms - span.source_start_ms;
        span.source_start_ms + output_offset * source_duration / output_duration
    }

    pub fn map_interval(&self, start_ms: i64, end_ms: i64) -> Option<(i64, i64)> {
        let start = match self.source_to_output(start_ms) {
            SourceMapping::Mapped(value) => value,
            SourceMapping::Deleted { output_boundary_ms } => output_boundary_ms,
        };
        let end = match self.source_to_output(end_ms) {
            SourceMapping::Mapped(value) => value,
            SourceMapping::Deleted { output_boundary_ms } => output_boundary_ms,
        };
        (end > start).then_some((start, end))
    }

    pub fn spans(&self) -> &[TimelineSpan] {
        &self.spans
    }

    pub fn output_duration_ms(&self) -> i64 {
        self.output_duration_ms
    }
}

fn validate_edit(edit: &TimelineEdit, source_duration_ms: i64) -> Result<(), AppError> {
    if edit.source_start_ms < 0
        || edit.source_end_ms > source_duration_ms
        || edit.source_end_ms <= edit.source_start_ms
    {
        return Err(AppError::Validation("时间编辑区间无效".into()));
    }
    if !matches!(edit.operation.as_str(), "cut" | "speed" | "freeze") {
        return Err(AppError::Validation("未知的时间编辑类型".into()));
    }
    if edit.operation == "speed" {
        let rate = edit.rate.unwrap_or(0.0);
        if !(1.0..=4.0).contains(&rate) || edit.output_duration_ms <= 0 {
            return Err(AppError::Validation("画面加速参数无效".into()));
        }
    }
    if edit.operation == "freeze" && edit.output_duration_ms <= 0 {
        return Err(AppError::Validation("画面延长时长无效".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(start: i64, end: i64, operation: &str, output: i64, rate: Option<f64>) -> TimelineEdit {
        TimelineEdit {
            id: format!("{operation}-{start}"),
            project_id: "project".into(),
            source_start_ms: start,
            source_end_ms: end,
            operation: operation.into(),
            rate,
            output_duration_ms: output,
            origin: "automatic".into(),
            reason: "test".into(),
            confidence: 1.0,
            accepted: true,
            revision: 1,
        }
    }

    #[test]
    fn cut_ripples_every_later_source_time() {
        let map = TimelineMap::from_edits(10_000, &[edit(2_000, 4_000, "cut", 0, None)]).unwrap();
        assert_eq!(
            map.source_to_output(3_000),
            SourceMapping::Deleted {
                output_boundary_ms: 2_000
            }
        );
        assert_eq!(map.source_to_output(7_000), SourceMapping::Mapped(5_000));
        assert_eq!(map.output_to_source(5_000), 7_000);
        assert_eq!(map.output_duration_ms(), 8_000);
    }

    #[test]
    fn speed_maps_inside_the_changed_interval() {
        let map = TimelineMap::from_edits(10_000, &[edit(2_000, 6_000, "speed", 2_000, Some(2.0))])
            .unwrap();
        assert_eq!(map.source_to_output(4_000), SourceMapping::Mapped(3_000));
        assert_eq!(map.source_to_output(8_000), SourceMapping::Mapped(6_000));
        assert_eq!(map.output_to_source(3_000), 4_000);
    }

    #[test]
    fn overlapping_edits_are_rejected() {
        let result = TimelineMap::from_edits(
            10_000,
            &[
                edit(2_000, 5_000, "cut", 0, None),
                edit(4_000, 6_000, "cut", 0, None),
            ],
        );
        assert!(result.is_err());
    }
}
