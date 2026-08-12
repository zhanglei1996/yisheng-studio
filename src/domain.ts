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
}

export interface ProviderTestResult {
  ok: boolean;
  latencyMs: number;
  message: string;
  availableModels: number;
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
}

export interface GlossaryTerm {
  id: string;
  source: string;
  target: string;
  policy: "keep" | "fixed" | "disabled";
  scope: "global" | "project";
  confidence: number;
}

export interface Job {
  id: string;
  project: string;
  stage: string;
  progress: number;
  status: "running" | "queued" | "waiting_user" | "paused" | "failed" | "succeeded" | "cancelled";
  eta: string;
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
