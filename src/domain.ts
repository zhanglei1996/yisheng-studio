export type ProjectStatus = "draft" | "processing" | "waiting_user" | "ready" | "failed";
export type SegmentStatus = "ready" | "processing" | "warning" | "stale";

export interface Project {
  id: string;
  name: string;
  duration: string;
  progress: number;
  status: ProjectStatus;
  updatedAt: string;
  thumbnail: string;
  sourcePath?: string | null;
  sourceFingerprint?: string | null;
  durationMs?: number | null;
  width?: number | null;
  height?: number | null;
  artifactDir?: string | null;
  workflowMode?: "quick" | "review";
  audioMode?: "duck" | "mute" | "separate";
  translationProviderId?: string | null;
  ttsProviderId?: string;
  ttsVoiceId?: string | null;
  ttsStyle?: TtsStyleId;
  ttsSettingsJson?: string;
  ttsDirectorEnabled?: boolean;
  ttsSyncMode?: "strict" | "balanced" | "narration" | "semantic";
  ttsSettingsRevision?: number;
  segmentCount?: number;
}

export interface MediaProbe {
  sourcePath: string;
  fingerprint: string;
  fileName: string;
  fileSize: number;
  durationMs: number;
  width: number;
  height: number;
  videoCodec: string;
  audioCodec?: string | null;
  audioSampleRate?: number | null;
}

export interface MediaArtifacts {
  projectId: string;
  proxyPath: string;
  audioPath: string;
  artifactDir: string;
}

export interface PreviewMedia {
  path: string;
  dubbed: boolean;
  revision: number;
}

export type JobStatus = "queued" | "running" | "waiting_user" | "paused" | "succeeded" | "failed" | "cancelled";

export interface PersistedJob {
  id: string;
  projectId: string;
  stage: string;
  progress: number;
  status: JobStatus;
  checkpoint?: string | null;
  errorMessage?: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface RuntimeComponent {
  id: string;
  name: string;
  architecture: string;
  version: string;
  installed: boolean;
  sha256?: string | null;
  license: string;
  sizeBytes?: number | null;
  status: "installed" | "available" | "downloading" | "paused" | "failed";
}

export interface ProviderProfile {
  id: string;
  kind: string;
  name: string;
  publicConfigJson: string;
  credentialRef?: string | null;
  driver?: string;
  revision?: number;
  secretBundleRef?: string | null;
  updatedAt?: string;
}

export interface ProviderTestResult {
  ok: boolean;
  latencyMs: number;
  message: string;
  availableModels: number;
}

export type TtsProviderId = "iflytek" | "aliyun" | "system" | (string & {});
export type TtsStyleId = "auto" | "professional" | "conversational" | "documentary" | "upbeat" | "emphasis";
export type TtsSynthesisStatus = "idle" | "estimating" | "ready" | "previewing" | "synthesizing" | "succeeded" | "failed";

export interface TtsVoice {
  id: string;
  providerId: TtsProviderId;
  providerName: string;
  name: string;
  locale: string;
  gender?: "female" | "male" | "neutral";
  traits: string[];
  available: boolean;
}

export interface TtsStyle {
  id: TtsStyleId;
  label: string;
  description: string;
}

export type ScriptOrigin = "translation" | "auto" | "manual";
export type InlineNode =
  | { type: "text"; text: string; emphasis?: 0 | 1 | 2; delivery?: "natural" | "professional" | "storytelling" | "casual" | "focus"; origin?: ScriptOrigin }
  | { type: "pause"; durationMs: number; origin: ScriptOrigin }
  | { type: "protected"; text: string; kind: "term" | "number" | "url" | "code" | "product"; canonical: string; pronunciation?: string; origin: ScriptOrigin };

export interface ScriptDocument {
  version: 1;
  blocks: Array<{ type: "paragraph"; children: InlineNode[] }>;
}

export interface Segment {
  id: string;
  projectId?: string;
  ordinal?: number;
  startMs: number;
  endMs: number;
  sourceText: string;
  subtitleZh: string;
  spokenZh: string;
  linked: boolean;
  status: SegmentStatus;
  voice: string;
  speed: number;
  scriptDocument?: ScriptDocument;
  ttsStyle?: TtsStyleId;
  directorEnabled?: boolean;
  ttsStatus?: TtsSynthesisStatus;
  scriptRevision?: number;
  ttsState?: string;
  ttsErrorMessage?: string | null;
  ttsSettingsHash?: string | null;
  ttsDurationMs?: number | null;
  overflowMs?: number;
  locked?: boolean;
}

export interface PersistedSegment {
  id: string;
  projectId: string;
  ordinal: number;
  startMs: number;
  endMs: number;
  sourceText: string;
  subtitleZh: string;
  spokenZh: string;
  linked: boolean;
  status: string;
  scriptDocJson?: string;
  scriptRevision?: number;
  ttsOverridesJson?: string;
  ttsState?: string;
  ttsErrorMessage?: string | null;
  ttsSettingsHash?: string | null;
  ttsDurationMs?: number | null;
}

export interface TtsCatalog {
  providerId: string;
  driver: string;
  local: boolean;
  voices: TtsVoice[];
  styles: TtsStyle[];
  supportsPreview: boolean;
  supportsInstructions: boolean;
  dataScope: "local" | "text_only";
}

export interface TtsPreviewAudio {
  requestId: string;
  path: string;
  revision: number;
  durationMs: number;
  cacheHit: boolean;
}

export interface TtsRunResult {
  warningIds: string[];
  failedSegments: Array<{ segmentId: string; message: string }>;
  affectedSegmentIds: string[];
  synthesisUnitCount: number;
  trackRevision: number;
  previewMedia?: PreviewMedia | null;
}

export interface TtsFitResult {
  initialCount: number;
  resolvedCount: number;
  remainingIds: string[];
  modifiedSegmentIds: string[];
  undoAvailable: boolean;
}

export interface TtsFitProgress {
  projectId: string;
  stage: "compressing" | "synthesizing" | "validating";
  completed: number;
  total: number;
  progress: number;
}

export type ProjectReadinessPhase = "processing" | "review" | "ready" | "export_warning" | "failed";

export interface ProjectReadiness {
  phase: ProjectReadinessPhase;
  blockingCount: number;
  warningCount: number;
  canExport: boolean;
  nextAction: string;
  progress: number;
}

export interface ExportPreflight {
  canExport: boolean;
  blockingCount: number;
  warningCount: number;
  blockingSegmentIds: string[];
  warningSegmentIds: string[];
  checks: PublishCheck[];
  message: string;
}

export interface NarrationScene {
  id: string;
  projectId: string;
  ordinal: number;
  sourceStartMs: number;
  sourceEndMs: number;
  segmentIds: string[];
  subtitleZh: string;
  spokenZh: string;
  durationBudgetMs: number;
  status: "draft" | "ready" | "warning" | "blocked";
  revision: number;
}

export interface SyncAnchor {
  id: string;
  projectId: string;
  sceneId: string;
  sourceTimeMs: number;
  phrase: string;
  kind: "visual" | "action" | "speech" | "non_speech";
  priority: "exact" | "near" | "free";
  toleranceMs: number;
  confidence: number;
  locked: boolean;
}

export interface TimelineEdit {
  id: string;
  projectId: string;
  sourceStartMs: number;
  sourceEndMs: number;
  operation: "keep" | "cut" | "speed" | "freeze";
  rate?: number | null;
  outputDurationMs: number;
  origin: "automatic" | "user";
  reason: string;
  confidence: number;
  accepted: boolean;
  revision: number;
}

export interface NonSpeechEvent {
  id: string;
  projectId: string;
  sourceStartMs: number;
  sourceEndMs: number;
  kind: "music" | "applause" | "click" | "typing" | "ambience" | "unknown";
  label: string;
  confidence: number;
}

export interface PublishCheck {
  code: string;
  severity: "blocking" | "warning" | "info";
  sourceRange?: [number, number] | null;
  outputRange?: [number, number] | null;
  sceneId?: string | null;
  message: string;
  suggestedAction?: string | null;
}

export interface LocalizationAnalysis {
  scenes: NarrationScene[];
  anchors: SyncAnchor[];
  timelineEdits: TimelineEdit[];
  nonSpeechEvents: NonSpeechEvent[];
  sourceDurationMs: number;
  outputDurationMs: number;
  estimatedSavingsMs: number;
}

export interface EditorSaveState {
  status: "idle" | "saving" | "saved" | "error";
  savedAt?: string;
  message?: string;
}

export const readinessLabel: Record<ProjectReadinessPhase, string> = {
  processing: "处理中",
  review: "待复核",
  ready: "可导出",
  export_warning: "导出有提醒",
  failed: "处理失败",
};

export interface GlossaryTerm {
  id: string;
  source: string;
  target: string;
  policy: "keep" | "fixed" | "disabled";
  scope: "global" | "project";
  confidence: number;
  projectId?: string | null;
  enabled?: boolean;
}

export interface Job {
  id: string;
  project: string;
  stage: string;
  progress: number;
  status: "running" | "queued" | "waiting_user" | "paused" | "failed" | "succeeded" | "cancelled";
  eta: string;
  errorMessage?: string | null;
  checkpoint?: string | null;
  projectId?: string;
  synthesisLabel?: string;
}

export const formatTimecode = (milliseconds: number, compact = false) => {
  const safe = Math.max(0, milliseconds);
  const totalSeconds = Math.floor(safe / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  const ms = Math.floor((safe % 1000) / 10);
  if (compact) return `${String(minutes + hours * 60).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
  return `${String(hours).padStart(2, "0")}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}.${String(ms).padStart(2, "0")}`;
};

export const statusLabel: Record<ProjectStatus, string> = {
  draft: "待配置",
  processing: "处理中",
  waiting_user: "等待校对",
  ready: "可导出",
  failed: "需要处理",
};
