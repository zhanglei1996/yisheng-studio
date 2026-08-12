use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub status: ProjectStatus,
    pub progress: u8,
    pub source_path: Option<String>,
    pub source_fingerprint: Option<String>,
    pub duration_ms: Option<i64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub artifact_dir: Option<String>,
    pub workflow_mode: String,
    pub audio_mode: String,
    pub translation_provider_id: Option<String>,
    pub tts_provider_id: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaProbe {
    pub source_path: String,
    pub fingerprint: String,
    pub file_name: String,
    pub file_size: u64,
    pub duration_ms: i64,
    pub width: u32,
    pub height: u32,
    pub video_codec: String,
    pub audio_codec: Option<String>,
    pub audio_sample_rate: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaArtifacts {
    pub project_id: String,
    pub proxy_path: String,
    pub audio_path: String,
    pub artifact_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewMedia {
    pub path: String,
    pub dubbed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Draft,
    Processing,
    WaitingUser,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeComponent {
    pub id: String,
    pub name: String,
    pub architecture: String,
    pub version: String,
    pub installed: bool,
    pub sha256: Option<String>,
    pub license: String,
    pub size_bytes: Option<u64>,
    pub status: RuntimeStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProfile {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub public_config_json: String,
    pub credential_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTestResult {
    pub ok: bool,
    pub latency_ms: u128,
    pub message: String,
    pub available_models: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStatus {
    Installed,
    Available,
    Downloading,
    Paused,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    WaitingUser,
    Paused,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSummary {
    pub id: String,
    pub project_id: String,
    pub stage: String,
    pub progress: u8,
    pub status: JobStatus,
    pub checkpoint: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentRecord {
    pub id: String,
    pub project_id: String,
    pub ordinal: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub source_text: String,
    pub subtitle_zh: String,
    pub spoken_zh: String,
    pub linked: bool,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentChange {
    SourceText,
    SubtitleLinked,
    SubtitleIndependent,
    SpokenText,
    Voice,
    Timing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Translation,
    TermValidation,
    Tts,
    Alignment,
    Mix,
    Subtitle,
    Export,
}

impl SegmentChange {
    pub fn invalidates(&self) -> &'static [ArtifactKind] {
        use ArtifactKind::*;
        match self {
            Self::SourceText => &[
                Translation,
                TermValidation,
                Tts,
                Alignment,
                Mix,
                Subtitle,
                Export,
            ],
            Self::SubtitleLinked => &[Tts, Alignment, Mix, Subtitle, Export],
            Self::SubtitleIndependent => &[Subtitle, Export],
            Self::SpokenText | Self::Voice => &[Tts, Alignment, Mix, Export],
            Self::Timing => &[Alignment, Mix, Subtitle, Export],
        }
    }
}
