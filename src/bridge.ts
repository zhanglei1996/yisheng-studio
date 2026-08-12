import type { MediaArtifacts, MediaProbe, PersistedJob, PersistedSegment, PreviewMedia, Project, ProviderProfile, ProviderTestResult, RuntimeComponent } from "./domain";
import { projects as demoProjects } from "./fixtures";

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
    return persisted.map((project) => ({
      ...project,
      duration: project.durationMs ? formatDuration(project.durationMs) : "—",
      updatedAt: project.updatedAt,
      thumbnail: demoProjects[0].thumbnail,
    }));
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

  async createProjectFromMedia(input: { probe: MediaProbe; workflowMode: string; audioMode: string; translationProviderId?: string | null }): Promise<Project | null> {
    const invoke = tauriInvoke();
    if (!invoke) return null;
    return invoke("project_create_from_media", input);
  },

  async prepareMedia(projectId: string, jobId: string): Promise<MediaArtifacts | null> {
    const invoke = tauriInvoke();
    return invoke ? invoke("media_prepare", { projectId, jobId }) : null;
  },

  async resolvePreviewMedia(projectId: string): Promise<PreviewMedia | null> {
    const invoke = tauriInvoke();
    return invoke ? invoke("preview_media", { projectId }) : null;
  },

  async listJobs(): Promise<PersistedJob[]> {
    const invoke = tauriInvoke();
    if (!invoke) return [];
    return invoke("job_list");
  },

  async enqueueJob(projectId: string): Promise<PersistedJob | null> {
    const invoke = tauriInvoke();
    if (!invoke) return null;
    return invoke("job_enqueue", { projectId });
  },

  async pauseJob(id: string) { const invoke = tauriInvoke(); return invoke ? invoke<PersistedJob>("job_pause", { id }) : null; },
  async resumeJob(id: string) { const invoke = tauriInvoke(); return invoke ? invoke<PersistedJob>("job_resume", { id }) : null; },
  async cancelJob(id: string) { const invoke = tauriInvoke(); return invoke ? invoke<PersistedJob>("job_cancel", { id }) : null; },
  async retryJob(id: string) { const invoke = tauriInvoke(); return invoke ? invoke<PersistedJob>("job_retry", { id }) : null; },

  async runAsr(projectId: string, jobId: string): Promise<PersistedSegment[]> {
    const invoke = tauriInvoke();
    if (!invoke) return [];
    return invoke("asr_run", { projectId, jobId });
  },

  async runTranslation(projectId: string, jobId: string): Promise<PersistedSegment[]> {
    const invoke = tauriInvoke();
    if (!invoke) return [];
    return invoke("translation_run", { projectId, jobId });
  },

  async rebuildTranslation(projectId: string, jobId: string): Promise<PersistedSegment[]> {
    const invoke = tauriInvoke();
    if (!invoke) return [];
    return invoke("translation_rebuild", { projectId, jobId });
  },

  async runTts(projectId: string, jobId: string): Promise<string[]> {
    const invoke = tauriInvoke();
    return invoke ? invoke("tts_run", { projectId, jobId }) : [];
  },

  async fitTtsWarnings(projectId: string, jobId: string): Promise<string[]> {
    const invoke = tauriInvoke();
    return invoke ? invoke("tts_fit_warnings", { projectId, jobId }) : [];
  },

  async startExport(projectId: string, jobId: string, outputDirectory: string, subtitleMode: string): Promise<{ directory: string; videoPath: string; audioPath: string } | null> {
    const invoke = tauriInvoke();
    return invoke ? invoke("export_start", { projectId, jobId, outputDirectory, subtitleMode }) : null;
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

  async listRuntimes(): Promise<RuntimeComponent[]> {
    const invoke = tauriInvoke();
    if (!invoke) return [];
    return invoke("runtime_catalog");
  },

  async listProviders(): Promise<ProviderProfile[]> {
    const invoke = tauriInvoke();
    return invoke ? invoke("provider_list") : [];
  },

  async saveProvider(profile: { id: string; kind: string; name: string; publicConfigJson: string; secret?: string }) {
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
};

const formatDuration = (milliseconds: number) => {
  const seconds = Math.max(0, Math.round(milliseconds / 1000));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const remaining = seconds % 60;
  return hours > 0 ? `${hours}:${String(minutes).padStart(2, "0")}:${String(remaining).padStart(2, "0")}` : `${minutes}:${String(remaining).padStart(2, "0")}`;
};
