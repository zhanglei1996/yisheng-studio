use std::{path::Path, process::Command};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticInterval {
    pub start_ms: i64,
    pub end_ms: i64,
}

/// Uses ffmpeg's frame-difference based freeze detector. Failure is deliberately
/// non-fatal: localization can still offer low-confidence gap suggestions.
pub fn detect_static_intervals(source: &Path) -> Vec<StaticInterval> {
    let output = Command::new(resolve_ffmpeg())
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(source)
        .args([
            "-vf",
            "freezedetect=n=-48dB:d=0.8",
            "-an",
            "-f",
            "null",
            "-",
        ])
        .output();
    output
        .ok()
        .map(|output| parse_freeze_log(&String::from_utf8_lossy(&output.stderr)))
        .unwrap_or_default()
}

fn parse_freeze_log(log: &str) -> Vec<StaticInterval> {
    let mut intervals = Vec::new();
    let mut start_ms = None;
    for line in log.lines() {
        if let Some(value) = value_after(line, "freeze_start:") {
            start_ms = Some((value * 1_000.0).round() as i64);
        }
        if let (Some(start), Some(value)) = (start_ms, value_after(line, "freeze_end:")) {
            let end = (value * 1_000.0).round() as i64;
            if end > start {
                intervals.push(StaticInterval {
                    start_ms: start,
                    end_ms: end,
                });
            }
            start_ms = None;
        }
    }
    intervals
}

fn value_after(line: &str, marker: &str) -> Option<f64> {
    let (_, rest) = line.split_once(marker)?;
    rest.split_whitespace().next()?.parse().ok()
}

fn resolve_ffmpeg() -> std::path::PathBuf {
    ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"]
        .into_iter()
        .map(|root| Path::new(root).join("ffmpeg"))
        .find(|path| path.is_file())
        .unwrap_or_else(|| "ffmpeg".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_freeze_intervals_from_ffmpeg_log() {
        let log = "[freezedetect] freeze_start: 2.04\n[freezedetect] freeze_duration: 1.8\n[freezedetect] freeze_end: 3.84";
        assert_eq!(
            parse_freeze_log(log),
            vec![StaticInterval {
                start_ms: 2_040,
                end_ms: 3_840
            }]
        );
    }
}
