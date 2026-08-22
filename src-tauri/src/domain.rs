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
    pub tts_voice_id: Option<String>,
    pub tts_style: String,
    pub tts_settings_json: String,
    pub tts_director_enabled: bool,
    pub tts_sync_mode: String,
    pub tts_settings_revision: u64,
    pub segment_count: u32,
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
    pub revision: u64,
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
    pub driver: String,
    pub revision: u64,
    pub secret_bundle_ref: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTestResult {
    pub ok: bool,
    pub latency_ms: u128,
    pub message: String,
    pub available_models: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsVoiceDescriptor {
    pub id: String,
    pub provider_id: String,
    pub provider_name: String,
    pub name: String,
    pub locale: String,
    pub gender: Option<String>,
    pub traits: Vec<String>,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsStyleDescriptor {
    pub id: String,
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsCatalog {
    pub provider_id: String,
    pub driver: String,
    pub local: bool,
    pub voices: Vec<TtsVoiceDescriptor>,
    pub styles: Vec<TtsStyleDescriptor>,
    pub supports_preview: bool,
    pub supports_instructions: bool,
    pub data_scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsSegmentFailure {
    pub segment_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsRunResult {
    pub warning_ids: Vec<String>,
    pub failed_segments: Vec<TtsSegmentFailure>,
    pub affected_segment_ids: Vec<String>,
    pub synthesis_unit_count: usize,
    pub cache_hit_unit_count: usize,
    pub track_revision: u64,
    pub preview_media: Option<PreviewMedia>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsFitResult {
    pub initial_count: usize,
    pub resolved_count: usize,
    pub remaining_ids: Vec<String>,
    pub modified_segment_ids: Vec<String>,
    pub undo_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsFitProgress {
    pub project_id: String,
    pub stage: String,
    pub completed: usize,
    pub total: usize,
    pub progress: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectReadiness {
    pub phase: String,
    pub blocking_count: usize,
    pub warning_count: usize,
    pub can_export: bool,
    pub next_action: String,
    pub progress: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPreflight {
    pub can_export: bool,
    pub blocking_count: usize,
    pub warning_count: usize,
    pub blocking_segment_ids: Vec<String>,
    pub warning_segment_ids: Vec<String>,
    pub checks: Vec<PublishCheck>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NarrationScene {
    pub id: String,
    pub project_id: String,
    pub ordinal: i64,
    pub source_start_ms: i64,
    pub source_end_ms: i64,
    pub segment_ids: Vec<String>,
    pub subtitle_zh: String,
    pub spoken_zh: String,
    pub duration_budget_ms: i64,
    pub status: String,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SyncAnchor {
    pub id: String,
    pub project_id: String,
    pub scene_id: String,
    pub source_time_ms: i64,
    pub phrase: String,
    pub kind: String,
    pub priority: String,
    pub tolerance_ms: i64,
    pub confidence: f64,
    pub locked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEdit {
    pub id: String,
    pub project_id: String,
    pub source_start_ms: i64,
    pub source_end_ms: i64,
    pub operation: String,
    pub rate: Option<f64>,
    pub output_duration_ms: i64,
    pub origin: String,
    pub reason: String,
    pub confidence: f64,
    pub accepted: bool,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NonSpeechEvent {
    pub id: String,
    pub project_id: String,
    pub source_start_ms: i64,
    pub source_end_ms: i64,
    pub kind: String,
    pub label: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WordTiming {
    pub text: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PublishCheck {
    pub code: String,
    pub severity: String,
    pub source_range: Option<[i64; 2]>,
    pub output_range: Option<[i64; 2]>,
    pub scene_id: Option<String>,
    pub message: String,
    pub suggested_action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationAnalysis {
    pub scenes: Vec<NarrationScene>,
    pub anchors: Vec<SyncAnchor>,
    pub timeline_edits: Vec<TimelineEdit>,
    pub non_speech_events: Vec<NonSpeechEvent>,
    pub source_duration_ms: i64,
    pub output_duration_ms: i64,
    pub estimated_savings_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsPreviewAudio {
    pub request_id: String,
    pub path: String,
    pub revision: u64,
    pub duration_ms: i64,
    pub cache_hit: bool,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum JobStage {
    MediaCheck,
    AudioExtract,
    SourceSeparation,
    Proxy,
    Asr,
    Glossary,
    Translation,
    ScriptDirector,
    SemanticNarration,
    Tts,
    Export,
}

impl JobStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MediaCheck => "media_check",
            Self::AudioExtract => "audio_extract",
            Self::SourceSeparation => "source_separation",
            Self::Proxy => "proxy",
            Self::Asr => "asr",
            Self::Glossary => "glossary",
            Self::Translation => "translation",
            Self::ScriptDirector => "script_director",
            Self::SemanticNarration => "semantic_narration",
            Self::Tts => "tts",
            Self::Export => "export",
        }
    }
}

impl std::fmt::Display for JobStage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl TryFrom<&str> for JobStage {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "media_check" => Ok(Self::MediaCheck),
            "audio_extract" => Ok(Self::AudioExtract),
            "source_separation" => Ok(Self::SourceSeparation),
            "proxy" => Ok(Self::Proxy),
            "asr" => Ok(Self::Asr),
            "glossary" => Ok(Self::Glossary),
            "translation" => Ok(Self::Translation),
            "script_director" => Ok(Self::ScriptDirector),
            "semantic_narration" => Ok(Self::SemanticNarration),
            "tts" => Ok(Self::Tts),
            "export" => Ok(Self::Export),
            _ => Err(format!("未知任务阶段：{value}")),
        }
    }
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
    pub stage: JobStage,
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
    /// Canonical editable script document. `spoken_zh` is a compatibility
    /// projection kept for the existing subtitle/TTS pipeline.
    pub script_doc_json: String,
    pub script_revision: u64,
    pub tts_overrides_json: String,
    pub tts_state: String,
    pub tts_error_message: Option<String>,
    pub tts_settings_hash: Option<String>,
    pub tts_duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRecord {
    pub id: String,
    pub project_id: String,
    pub segment_id: Option<String>,
    pub kind: String,
    pub path: String,
    pub content_hash: String,
    pub dependency_hash: String,
    pub cache_key: Option<String>,
    pub revision: u64,
    pub status: String,
    pub metadata_json: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GlossaryTerm {
    pub id: String,
    pub project_id: Option<String>,
    pub source: String,
    pub target: String,
    pub policy: String,
    pub enabled: bool,
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
