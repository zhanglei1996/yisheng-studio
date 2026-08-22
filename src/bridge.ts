import type { ExportPreflight, GlossaryTerm, LocalizationAnalysis, MediaProbe, PersistedJob, PersistedSegment, PreviewMedia, Project, ProjectReadiness, ProviderProfile, ProviderTestResult, RuntimeComponent, ScriptDocument, TtsCatalog, TtsFitProgress, TtsFitResult, TtsPreviewAudio, TtsRunResult, TtsStyleId, WorkflowIntentResult } from "./domain";
import { projects as demoProjects } from "./fixtures";
import { convertFileSrc } from "@tauri-apps/api/core";

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
type Listen = <T>(event: string, handler: (event: { payload: T }) => void) => Promise<() => void>;

const tauriInvoke = (): Invoke | null => {
  const runtime = (window as Window & { __TAURI_INTERNALS__?: { invoke?: Invoke } }).__TAURI_INTERNALS__;
  return runtime?.invoke ?? null;
};

const tauriListen = (): Listen | null => {
  const runtime = (window as Window & { __TAURI_INTERNALS__?: { transformCallback?: unknown } }).__TAURI_INTERNALS__;
  if (!runtime) return null;
  return async <T>(event: string, handler: (event: { payload: T }) => void) => {
    const { listen } = await import("@tauri-apps/api/event");
    return listen<T>(event, handler);
  };
};

export const desktopBridge = {
  isDesktop: () => Boolean(tauriInvoke()),

  async listProjects(): Promise<Project[]> {
    const invoke = tauriInvoke();
    if (!invoke) return demoProjects;
    const persisted = await invoke<Array<Project & { durationMs?: number | null }>>("project_list");
    return Promise.all(persisted.map(async (project) => {
      const thumbnailPath = await invoke<string | null>("project_thumbnail", { projectId: project.id }).catch(() => null);
      return {
        ...project,
        duration: project.durationMs ? formatDuration(project.durationMs) : "—",
        updatedAt: project.updatedAt,
        thumbnail: thumbnailPath ? `${convertFileSrc(thumbnailPath)}?revision=${encodeURIComponent(project.updatedAt)}` : "",
      };
    }));
  },

  async getProjectReadiness(projectId: string): Promise<ProjectReadiness> {
    const invoke = tauriInvoke();
    if (!invoke) {
      const project = demoProjects.find((item) => item.id === projectId);
      if (project?.status === "waiting_user") return { phase: "export_warning", blockingCount: 0, warningCount: 1, canExport: true, nextAction: "自动修复 1 个时长问题或知情导出", progress: project.progress };
      if (project?.status === "processing") return { phase: "processing", blockingCount: 0, warningCount: 0, canExport: false, nextAction: "等待当前处理完成", progress: project.progress };
      return { phase: "ready", blockingCount: 0, warningCount: 0, canExport: true, nextAction: "导出中文版本", progress: project?.progress ?? 100 };
    }
    return invoke("project_readiness", { projectId });
  },

  async updateProjectAudioMode(projectId: string, audioMode: "duck" | "mute" | "separate"): Promise<Project | null> {
    const invoke = tauriInvoke();
    return invoke ? invoke("project_audio_mode_update", { projectId, audioMode }) : null;
  },

  async createProject(name: string): Promise<{ id: string; name: string } | null> {
    const invoke = tauriInvoke();
    if (!invoke) return null;
    return invoke<{ id: string; name: string }>("project_create", { name });
  },

  async selectVideo(): Promise<string | null> {
    if (!tauriInvoke()) return null;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({ multiple: false, directory: false, title: "选择英文视频", filters: [{ name: "视频", extensions: ["mp4", "mov", "mkv", "m4v", "webm"] }] });
    return typeof selected === "string" ? selected : null;
  },

  async probeMedia(path: string): Promise<MediaProbe> {
    const invoke = tauriInvoke();
    if (!invoke) throw new Error("媒体检查仅在桌面应用中可用");
    return invoke("media_probe", { path });
  },

  async createProjectFromMedia(input: { probe: MediaProbe; workflowMode: string; audioMode: string; translationProviderId?: string | null; ttsProviderId?: string | null; ttsVoiceId?: string | null; projectName?: string | null }): Promise<Project | null> {
    const invoke = tauriInvoke();
    if (!invoke) return null;
    return invoke("project_create_from_media", input);
  },

  async renameProject(projectId: string, name: string): Promise<Project | null> {
    const invoke = tauriInvoke();
    return invoke ? invoke("project_rename", { projectId, name }) : null;
  },

  async deleteProject(projectId: string): Promise<void> {
    const invoke = tauriInvoke();
    if (invoke) await invoke("project_delete", { projectId });
  },

  async resolvePreviewMedia(projectId: string): Promise<PreviewMedia | null> {
    const invoke = tauriInvoke();
    return invoke ? invoke("preview_media", { projectId }) : null;
  },

  async preparePreviewMedia(projectId: string): Promise<PreviewMedia | null> {
    const invoke = tauriInvoke();
    return invoke ? invoke("preview_prepare", { projectId }) : null;
  },

  async listJobs(): Promise<PersistedJob[]> {
    const invoke = tauriInvoke();
    if (!invoke) return [];
    return invoke("job_list");
  },

  async deleteJob(id: string): Promise<void> { const invoke = tauriInvoke(); if (invoke) await invoke("job_delete", { id }); },

  async enqueueWorkflow(projectId: string): Promise<WorkflowIntentResult | null> {
    const invoke = tauriInvoke();
    if (!invoke) return null;
    return invoke("workflow_enqueue", { projectId });
  },

  async startWorkflow(jobId: string): Promise<WorkflowIntentResult | null> { const invoke = tauriInvoke(); return invoke ? invoke("workflow_start", { jobId }) : null; },
  async continueWorkflow(jobId: string): Promise<WorkflowIntentResult | null> { const invoke = tauriInvoke(); return invoke ? invoke("workflow_continue", { jobId }) : null; },
  async retryWorkflow(jobId: string): Promise<WorkflowIntentResult | null> { const invoke = tauriInvoke(); return invoke ? invoke("workflow_retry", { jobId }) : null; },
  async pauseWorkflow(jobId: string) { const invoke = tauriInvoke(); return invoke ? invoke<PersistedJob>("workflow_pause", { jobId }) : null; },
  async cancelWorkflow(jobId: string) { const invoke = tauriInvoke(); return invoke ? invoke<PersistedJob>("workflow_cancel", { jobId }) : null; },

  async rebuildTranslation(projectId: string, jobId: string): Promise<PersistedSegment[]> {
    const invoke = tauriInvoke();
    if (!invoke) return [];
    return invoke("translation_rebuild", { projectId, jobId });
  },

  async runTts(projectId: string, jobId: string, segmentIds?: string[]): Promise<TtsRunResult> {
    const invoke = tauriInvoke();
    if (!invoke) return { warningIds: [], failedSegments: [], affectedSegmentIds: segmentIds ?? [], synthesisUnitCount: segmentIds?.length ?? 0, cacheHitUnitCount: 0, trackRevision: 0 };
    const result = await invoke<TtsRunResult | string[]>("tts_run", { projectId, jobId, segmentIds });
    return Array.isArray(result)
      ? { warningIds: result, failedSegments: [], affectedSegmentIds: segmentIds ?? [], synthesisUnitCount: segmentIds?.length ?? 0, cacheHitUnitCount: 0, trackRevision: Date.now() }
      : result;
  },

  async runSemanticNarration(projectId: string, jobId: string): Promise<TtsRunResult> {
    const invoke = tauriInvoke();
    if (!invoke) return { warningIds: [], failedSegments: [], affectedSegmentIds: [], synthesisUnitCount: 0, cacheHitUnitCount: 0, trackRevision: 0 };
    return invoke("semantic_narration_run", { projectId, jobId });
  },

  async fitTtsWarnings(projectId: string, jobId: string, segmentIds?: string[]): Promise<TtsFitResult> {
    const invoke = tauriInvoke();
    return invoke ? invoke("tts_fit_warnings", { projectId, jobId, segmentIds }) : { initialCount: 0, resolvedCount: 0, remainingIds: [], modifiedSegmentIds: [], undoAvailable: false };
  },
  async undoTtsFit(projectId: string): Promise<string[]> {
    const invoke = await tauriInvoke();
    return invoke ? invoke("tts_fit_undo", { projectId }) : [];
  },

  async getExportPreflight(projectId: string): Promise<ExportPreflight> {
    const invoke = tauriInvoke();
    return invoke ? invoke("export_preflight", { projectId }) : projectId === "p1"
      ? { canExport: true, blockingCount: 0, warningCount: 1, blockingSegmentIds: [], warningSegmentIds: ["seg-02"], checks: [], message: "可导出，但仍有 1 个片段的配音可能超出原视频时间窗" }
      : { canExport: true, blockingCount: 0, warningCount: 0, blockingSegmentIds: [], warningSegmentIds: [], checks: [], message: "可以导出" };
  },

  async startExport(projectId: string, jobId: string, outputDirectory: string, subtitleMode: string, exportPreset = "balanced"): Promise<{ directory: string; videoPath: string; audioPath: string } | null> {
    const invoke = tauriInvoke();
    return invoke ? invoke("export_start", { projectId, jobId, outputDirectory, subtitleMode, exportPreset }) : null;
  },

  async analyzeLocalization(projectId: string, refresh = false): Promise<LocalizationAnalysis | null> {
    const invoke = tauriInvoke();
    return invoke ? invoke("localization_analyze", { projectId, refresh }) : null;
  },

  async acceptTimelineEdit(projectId: string, editId: string, accepted: boolean): Promise<LocalizationAnalysis | null> {
    const invoke = tauriInvoke();
    return invoke ? invoke("timeline_edit_accept", { projectId, editId, accepted }) : null;
  },

  async revealInFinder(path: string): Promise<void> {
    const invoke = tauriInvoke();
    if (invoke) await invoke("path_reveal", { path });
  },

  async listSegments(projectId: string): Promise<PersistedSegment[]> {
    const invoke = tauriInvoke();
    return invoke ? invoke("segment_list", { projectId }) : [];
  },

  async saveSegments(projectId: string, segments: PersistedSegment[]): Promise<void> {
    const invoke = tauriInvoke();
    if (invoke) await invoke("segment_replace_project", { projectId, segments });
  },

  async updateProjectTtsSettings(input: { projectId: string; providerId: string; voiceId?: string | null; style: TtsStyleId; settingsJson?: string; directorEnabled: boolean; syncMode: "strict" | "balanced" | "narration" | "semantic" }): Promise<Project | null> {
    const invoke = tauriInvoke();
    return invoke ? invoke("project_tts_settings_update", input) : null;
  },

  async updateSegmentScript(input: { segmentId: string; expectedRevision: number; document: ScriptDocument; ttsOverridesJson?: string }): Promise<PersistedSegment | null> {
    const invoke = tauriInvoke();
    return invoke ? invoke("segment_script_update", { input }) : null;
  },

  async planDirector(input: { segmentId: string; style?: TtsStyleId }): Promise<ScriptDocument | null> {
    const invoke = tauriInvoke();
    return invoke ? invoke("director_plan", { request: input }) : null;
  },

  async listTtsCatalog(providerId: string): Promise<TtsCatalog | null> {
    const invoke = tauriInvoke();
    return invoke ? invoke("tts_catalog", { providerId }) : null;
  },

  async previewTts(input: { segmentId: string; scriptRevision: number; document: ScriptDocument; providerId?: string; voiceId?: string; style?: TtsStyleId; speed?: number }): Promise<TtsPreviewAudio | null> {
    const invoke = tauriInvoke();
    return invoke ? invoke("tts_audition", { request: input }) : null;
  },

  async cancelTtsPreview(requestId: string): Promise<void> {
    const invoke = tauriInvoke();
    if (invoke) await invoke("tts_audition_cancel", { requestId });
  },

  async listGlossary(projectId?: string | null): Promise<GlossaryTerm[]> {
    const invoke = tauriInvoke();
    if (!invoke) return [];
    const terms = await invoke<Array<{ id: string; projectId?: string | null; source: string; target: string; policy: string; enabled: boolean }>>("glossary_list", { projectId });
    return terms.map((term) => ({
      ...term,
      policy: ["keep", "fixed", "disabled"].includes(term.policy) ? term.policy as GlossaryTerm["policy"] : "fixed",
      scope: term.projectId ? "project" : "global",
      confidence: term.enabled ? 1 : 0,
    }));
  },

  async saveGlossary(term: GlossaryTerm): Promise<GlossaryTerm | null> {
    const invoke = tauriInvoke();
    if (!invoke) return term;
    await invoke("glossary_save", { term: { id: term.id, projectId: term.scope === "project" ? term.projectId ?? null : null, source: term.source, target: term.target, policy: term.policy, enabled: term.enabled ?? term.policy !== "disabled" } });
    return term;
  },

  async deleteGlossary(id: string): Promise<void> {
    const invoke = tauriInvoke();
    if (invoke) await invoke("glossary_delete", { id });
  },

  async listRuntimes(): Promise<RuntimeComponent[]> {
    const invoke = tauriInvoke();
    if (!invoke) return [];
    return invoke("runtime_catalog");
  },

  async listProviders(): Promise<ProviderProfile[]> {
    const invoke = tauriInvoke();
    return invoke ? invoke("provider_list") : [];
  },

  async saveProvider(profile: { id: string; kind: string; name: string; publicConfigJson: string; secret?: string; driver?: string }) {
    const invoke = tauriInvoke();
    return invoke ? invoke<ProviderProfile>("provider_save", profile) : null;
  },

  async deleteProvider(id: string) {
    const invoke = tauriInvoke();
    return invoke ? invoke<void>("provider_delete", { id }) : undefined;
  },

  async testProvider(id: string): Promise<ProviderTestResult | null> {
    const invoke = tauriInvoke();
    return invoke ? invoke("provider_test", { id }) : null;
  },

  async onJobState(handler: (job: PersistedJob) => void) {
    const listen = tauriListen();
    return listen ? listen<PersistedJob>("job://state", (event) => handler(event.payload)) : () => undefined;
  },

  async onTtsFitProgress(handler: (progress: TtsFitProgress) => void) {
    const listen = tauriListen();
    return listen ? listen<TtsFitProgress>("tts-fit://progress", (event) => handler(event.payload)) : () => undefined;
  },
};

const formatDuration = (milliseconds: number) => {
  const seconds = Math.max(0, Math.round(milliseconds / 1000));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const remaining = seconds % 60;
  return hours > 0 ? `${hours}:${String(minutes).padStart(2, "0")}:${String(remaining).padStart(2, "0")}` : `${minutes}:${String(remaining).padStart(2, "0")}`;
};
