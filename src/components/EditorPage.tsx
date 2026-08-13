import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { useShallow } from "zustand/shallow";
import { convertFileSrc } from "@tauri-apps/api/core";
import { Button, Popconfirm, Tooltip, message } from "antd";
import { ArrowCounterClockwise, ArrowClockwise, CheckCircle, CornersOut, Export, FileVideo, Info, Pause, Play, Plus, SkipBack, SkipForward, SpeakerHigh, SpeakerSlash, Sparkle, Warning, Waveform } from "@phosphor-icons/react";
import { formatTimecode, type EditorSaveState, type InlineNode, type PersistedJob, type PersistedSegment, type ProjectReadiness, type ScriptDocument, type Segment, type TtsFitProgress, type TtsFitResult, type TtsVoice } from "../domain";
import { useEditorStore } from "../store";
import { Inspector } from "./Inspector";
import { Timeline } from "./Timeline";
import { antdIcon } from "../ui/icons";
import { desktopBridge } from "../bridge";
import { markEditorReady } from "../performance";

const UndoIcon = antdIcon(ArrowCounterClockwise);
const RedoIcon = antdIcon(ArrowClockwise);
const ExportIcon = antdIcon(Export);
const BackIcon = antdIcon(SkipBack);
const PlayIcon = antdIcon(Play);
const PauseIcon = antdIcon(Pause);
const ForwardIcon = antdIcon(SkipForward);
const SpeakerIcon = antdIcon(SpeakerHigh);
const MuteIcon = antdIcon(SpeakerSlash);
const FullscreenIcon = antdIcon(CornersOut);
const PlusIcon = antdIcon(Plus);

type SignableSegment = { id: string; startMs: number; endMs: number; sourceText: string; subtitleZh: string; spokenZh: string; linked: boolean; status: string; scriptDocJson?: string; scriptRevision?: number; ttsOverridesJson?: string; ttsState?: string; ttsErrorMessage?: string | null; ttsSettingsHash?: string | null; ttsDurationMs?: number | null };
const signatureOf = (items: SignableSegment[]) => JSON.stringify(items.map((segment) => [segment.id, segment.startMs, segment.endMs, segment.sourceText, segment.subtitleZh, segment.spokenZh, segment.linked, segment.status, segment.scriptDocJson, segment.scriptRevision, segment.ttsOverridesJson, segment.ttsState, segment.ttsErrorMessage, segment.ttsSettingsHash, segment.ttsDurationMs]));

const parseScript = (raw: string | undefined, spokenZh: string): ScriptDocument => {
  if (raw) {
    try {
      const document = JSON.parse(raw) as ScriptDocument & { blocks?: Array<{ type: "paragraph"; children: Array<InlineNode & { duration_ms?: number }> }> };
      if (document.version === 1 && Array.isArray(document.blocks)) {
        return {
          ...document,
          blocks: document.blocks.map((block) => ({
            ...block,
            children: block.children.map((node) => {
              const legacyNode = node as typeof node & { duration_ms?: number };
              return node.type === "pause" && node.durationMs == null
                ? { ...node, durationMs: legacyNode.duration_ms ?? 280 }
                : node;
            }),
          })),
        } as ScriptDocument;
      }
    } catch { /* legacy or corrupt documents fall back without blocking the editor */ }
  }
  return { version: 1, blocks: [{ type: "paragraph", children: [{ type: "text", text: spokenZh, origin: "translation" }] }] };
};

const parseOverrides = (raw: string | undefined) => {
  if (!raw) return {};
  try { return JSON.parse(raw) as { voiceId?: string; style?: Segment["ttsStyle"]; speed?: number; directorEnabled?: boolean }; }
  catch { return {}; }
};

const plainTextOf = (document: ScriptDocument) => document.blocks.flatMap((block) => block.children).map((node) => node.type === "pause" ? "" : node.text).join("");

const balancedBlocks = (items: Segment[]) => {
  const blocks: Segment[][] = [];
  const overrideKey = (segment: Segment) => JSON.stringify([segment.voice, segment.ttsStyle, segment.speed, segment.directorEnabled]);
  items.forEach((segment) => {
    const block = blocks.at(-1);
    const first = block?.[0];
    const last = block?.at(-1);
    const span = first && last ? last.endMs - first.startMs : 0;
    const nextSpan = first ? segment.endMs - first.startMs : 0;
    const split = !block || !first || !last || overrideKey(first) !== overrideKey(segment)
      || segment.startMs - last.endMs >= 1_200 || block.length >= 6 || span >= 12_000 || (span >= 5_000 && nextSpan > 15_000);
    if (split) blocks.push([segment]); else block.push(segment);
  });
  return blocks;
};

const narrationChapters = (items: Segment[]) => {
  const chapters: Segment[][] = [];
  items.forEach((segment) => {
    const chapter = chapters.at(-1);
    const first = chapter?.[0];
    const last = chapter?.at(-1);
    const span = first && last ? last.endMs - first.startMs : 0;
    const nextSpan = first ? segment.endMs - first.startMs : 0;
    const split = !chapter || !first || !last || nextSpan > 105_000
      || (span >= 58_000 && segment.startMs - last.endMs >= 900)
      || (span >= 78_000 && /[。！？]$/.test(first.spokenZh.trim()));
    if (split) chapters.push([segment]); else chapter.push(segment);
  });
  return chapters;
};

const semanticScenes = (items: Segment[]) => {
  const scenes: Segment[][] = [];
  balancedBlocks(items).forEach((beat) => {
    const scene = scenes.at(-1);
    const first = scene?.[0];
    const last = scene?.at(-1);
    const span = first && last ? last.endMs - first.startMs : 0;
    const nextSpan = first ? beat.at(-1)!.endMs - first.startMs : 0;
    const split = !scene || !first || !last || nextSpan > 60_000
      || (span >= 38_000 && beat[0].startMs - last.endMs >= 800) || span >= 52_000;
    if (split) scenes.push([...beat]); else scene.push(...beat);
  });
  return scenes;
};

const toEditorSegment = (segment: PersistedSegment, projectVoice: string, projectStyle: Segment["ttsStyle"], directorEnabled: boolean): Segment => {
  const overrides = parseOverrides(segment.ttsOverridesJson);
  const overflowMs = segment.status === "warning"
    ? segment.ttsDurationMs
      ? Math.max(1, segment.ttsDurationMs - (segment.endMs - segment.startMs))
      : 1
    : undefined;
  return {
    ...segment,
    status: ["ready", "processing", "warning", "stale"].includes(segment.status) ? segment.status as Segment["status"] : "ready",
    voice: overrides.voiceId ?? projectVoice,
    speed: overrides.speed ?? 1,
    ttsStyle: overrides.style ?? projectStyle ?? "auto",
    directorEnabled: overrides.directorEnabled ?? directorEnabled,
    ttsStatus: segment.ttsState === "ready" ? "succeeded" : segment.ttsState === "processing" ? "synthesizing" : segment.ttsState === "failed" ? "failed" : "idle",
    scriptRevision: segment.scriptRevision ?? 1,
    ttsState: segment.ttsState ?? "stale",
    ttsErrorMessage: segment.ttsErrorMessage ?? null,
    ttsSettingsHash: segment.ttsSettingsHash ?? null,
    ttsDurationMs: segment.ttsDurationMs ?? null,
    overflowMs: overflowMs && overflowMs > 0 ? overflowMs : undefined,
    scriptDocument: parseScript(segment.scriptDocJson, segment.spokenZh),
  };
};

const toPersistedSegment = (segment: Segment, projectId: string, ordinal: number): PersistedSegment => {
  const document = segment.scriptDocument && plainTextOf(segment.scriptDocument) === segment.spokenZh
    ? segment.scriptDocument
    : parseScript(undefined, segment.spokenZh);
  return {
    id: segment.id,
    projectId,
    ordinal,
    startMs: Math.round(segment.startMs),
    endMs: Math.round(segment.endMs),
    sourceText: segment.sourceText,
    subtitleZh: segment.subtitleZh,
    spokenZh: segment.spokenZh,
    linked: segment.linked,
    status: segment.status,
    scriptDocJson: JSON.stringify(document),
    scriptRevision: Math.max(1, segment.scriptRevision ?? 1),
    ttsOverridesJson: JSON.stringify({ voiceId: segment.voice, style: segment.ttsStyle ?? "auto", speed: segment.speed, directorEnabled: segment.directorEnabled ?? true }),
    ttsState: segment.status === "stale" ? "stale" : segment.status === "processing" ? "processing" : segment.ttsState ?? "stale",
    ttsErrorMessage: segment.ttsErrorMessage ?? null,
    ttsSettingsHash: segment.status === "stale" ? null : segment.ttsSettingsHash ?? null,
    ttsDurationMs: segment.status === "stale" ? null : segment.ttsDurationMs ?? null,
  };
};

export const EditorPage = memo(function EditorPage({ active, onCreate, onExport, onRegenerate, onRegenerateAll, onFitWarnings, onUndoFit, fitProgress, fitResult, fittingWarnings, readiness, activeJob, onSaveStateChange, projectId }: { active: boolean; onCreate: () => void; onExport: () => void; onRegenerate: (segmentId: string) => Promise<void>; onRegenerateAll: () => Promise<void>; onFitWarnings: (segmentIds?: string[]) => Promise<TtsFitResult | null>; onUndoFit: () => Promise<void>; fitProgress?: TtsFitProgress | null; fitResult?: TtsFitResult | null; fittingWarnings?: boolean; readiness?: ProjectReadiness | null; activeJob?: PersistedJob; onSaveStateChange?: (state: EditorSaveState) => void; projectId: string | null }) {
  const queryClient = useQueryClient();
  const { segments, loadedProjectId, selectedId, selectSegment, setInspectorTab, setPlayhead, playing, togglePlaying, muted, undo, redo, history, future, hydrateProject, setProjectVoice, setLocalization } = useEditorStore(useShallow((state) => ({
    segments: state.segments, selectedId: state.selectedId, selectSegment: state.selectSegment, playing: state.playing, togglePlaying: state.togglePlaying,
    loadedProjectId: state.loadedProjectId, setPlayhead: state.setPlayhead, muted: state.muted, undo: state.undo, redo: state.redo,
    history: state.history, future: state.future, hydrateProject: state.hydrateProject, setProjectVoice: state.setProjectVoice, setInspectorTab: state.setInspectorTab, setLocalization: state.setLocalization,
  })));
  const [regeneratingSegmentId, setRegeneratingSegmentId] = useState<string | null>(null);
  const { data: projectSegments = [] } = useQuery({ queryKey: ["segments", projectId], queryFn: () => desktopBridge.listSegments(projectId!), enabled: active && Boolean(projectId) && desktopBridge.isDesktop(), staleTime: 60_000 });
  const { data: localizationAnalysis } = useQuery({ queryKey: ["localization-analysis", projectId], queryFn: () => desktopBridge.analyzeLocalization(projectId!), enabled: active && Boolean(projectId) && desktopBridge.isDesktop(), staleTime: 60_000 });
  const { data: projects = [] } = useQuery({ queryKey: ["projects"], queryFn: desktopBridge.listProjects });
  const project = projects.find((item) => item.id === projectId);
  const { data: providers = [] } = useQuery({ queryKey: ["providers"], queryFn: desktopBridge.listProviders, enabled: active && desktopBridge.isDesktop(), staleTime: 60_000 });
  const ttsProviderId = project?.ttsProviderId ?? "system";
  const ttsProviderProfiles = useMemo(() => [
    { id: "system", revision: 1, configured: true },
    ...providers
      .filter((provider) => provider.kind === "cloud_tts" && provider.id !== "system" && Boolean(provider.secretBundleRef || provider.credentialRef))
      .map((provider) => ({
        id: provider.id,
        revision: provider.revision ?? 1,
        configured: true,
      })),
  ], [providers]);
  const ttsCatalogQueries = useQueries({
    queries: ttsProviderProfiles.map((provider) => ({
      queryKey: ["tts-catalog", provider.id, provider.revision] as const,
      queryFn: () => desktopBridge.listTtsCatalog(provider.id),
      enabled: active && Boolean(projectId) && desktopBridge.isDesktop(),
      staleTime: Number.POSITIVE_INFINITY,
      retry: false,
    })),
  });
  const voices = useMemo<TtsVoice[]>(() => {
    const configured = new Map(ttsProviderProfiles.map((provider) => [provider.id, provider.configured]));
    const catalogVoices = ttsCatalogQueries.flatMap((query) => query.data?.voices ?? []).map((voice) => ({
      ...voice,
      available: voice.providerId === "system" || Boolean(configured.get(voice.providerId)),
    }));
    if (catalogVoices.length) return catalogVoices;
    return [{ id: "Tingting", providerId: "system", providerName: "macOS 系统语音", name: "Tingting", locale: "zh-CN", gender: "female" as const, traits: ["本地", "免费"], available: true }];
  }, [ttsCatalogQueries, ttsProviderProfiles]);
  const previewEnabled = Boolean(projectId && project?.artifactDir) && desktopBridge.isDesktop();
  const { data: previewMedia, isPending: previewPending, isError: previewFailed, refetch: retryPreview } = useQuery({
    queryKey: ["preview-media", projectId, project?.updatedAt],
    queryFn: () => desktopBridge.resolvePreviewMedia(projectId!),
    enabled: active && previewEnabled,
    retry: 1,
    staleTime: 60_000,
  });
  const videoRef = useRef<HTMLVideoElement>(null);
  const previewUrl = useMemo(() => previewMedia?.path && desktopBridge.isDesktop() ? `${convertFileSrc(previewMedia.path)}?revision=${previewMedia.revision}` : null, [previewMedia?.path, previewMedia?.revision]);
  const syncingFromBackend = useRef(false);
  const persistedSignature = useRef("");
  const persistedSegments = useRef<Map<string, PersistedSegment>>(new Map());
  const persistQueue = useRef<Promise<PersistedSegment[]>>(Promise.resolve([]));
  const persistEditorSnapshot = useCallback(async (targetProjectId: string, editorSegments: Segment[]) => {
    const snapshot = editorSegments.map((segment) => ({ ...segment }));
    const run = async () => {
      const baselineById = persistedSegments.current;
      const latestSegments = await desktopBridge.listSegments(targetProjectId);
      const latestById = new Map(latestSegments.map((segment) => [segment.id, segment]));
      const candidates = snapshot.map((segment, ordinal) => toPersistedSegment(segment, targetProjectId, ordinal));
      const payload = await Promise.all(candidates.map(async (candidate) => {
        const baseline = baselineById.get(candidate.id);
        const latest = latestById.get(candidate.id) ?? baseline;
        if (!latest) return candidate;
        const scriptChanged = !baseline || candidate.scriptDocJson !== baseline.scriptDocJson || candidate.ttsOverridesJson !== baseline.ttsOverridesJson;
        if (scriptChanged) {
          const updated = await desktopBridge.updateSegmentScript({
            segmentId: candidate.id,
            expectedRevision: latest.scriptRevision ?? 1,
            document: parseScript(candidate.scriptDocJson, candidate.spokenZh),
            ttsOverridesJson: candidate.ttsOverridesJson,
          });
          if (updated) return {
            ...candidate,
            spokenZh: updated.spokenZh,
            scriptDocJson: updated.scriptDocJson,
            scriptRevision: updated.scriptRevision,
            ttsState: "stale",
            ttsErrorMessage: null,
            ttsSettingsHash: null,
            ttsDurationMs: null,
          };
        }
        const invalidated = candidate.status === "stale" || candidate.status === "processing";
        return {
          ...candidate,
          spokenZh: latest.spokenZh,
          scriptDocJson: latest.scriptDocJson,
          ttsOverridesJson: latest.ttsOverridesJson,
          scriptRevision: Math.max(candidate.scriptRevision ?? 1, latest.scriptRevision ?? 1),
          ttsState: invalidated ? candidate.ttsState : latest.ttsState ?? candidate.ttsState,
          ttsErrorMessage: invalidated ? null : latest.ttsErrorMessage ?? candidate.ttsErrorMessage,
          ttsSettingsHash: invalidated ? null : latest.ttsSettingsHash ?? candidate.ttsSettingsHash,
          ttsDurationMs: invalidated ? null : latest.ttsDurationMs ?? candidate.ttsDurationMs,
        };
      }));
      await desktopBridge.saveSegments(targetProjectId, payload);
      persistedSegments.current = new Map(payload.map((segment) => [segment.id, segment]));
      persistedSignature.current = signatureOf(payload);
      return payload;
    };
    const queued = persistQueue.current.catch(() => []).then(run);
    persistQueue.current = queued;
    return queued;
  }, []);
  useEffect(() => {
    if (localizationAnalysis) setLocalization(localizationAnalysis);
  }, [localizationAnalysis, setLocalization]);
  useEffect(() => {
    if (projectId && projectSegments.length) {
      const nextSignature = signatureOf(projectSegments);
      persistedSignature.current = nextSignature;
      persistedSegments.current = new Map(projectSegments.map((segment) => [segment.id, segment]));
      const hydrated = hydrateProject(projectId, nextSignature, projectSegments.map((segment) => toEditorSegment(segment, project?.ttsVoiceId ?? "system-tingting", project?.ttsStyle ?? "auto", project?.ttsDirectorEnabled ?? true)));
      if (hydrated) syncingFromBackend.current = true;
    }
  }, [projectId, projectSegments, hydrateProject, project?.ttsVoiceId, project?.ttsStyle, project?.ttsDirectorEnabled]);
  useEffect(() => {
    if (!projectId || loadedProjectId !== projectId || !desktopBridge.isDesktop() || !segments.length) return;
    if (syncingFromBackend.current) {
      syncingFromBackend.current = false;
      return;
    }
    const timer = window.setTimeout(() => {
      const editorState = useEditorStore.getState();
      if (editorState.loadedProjectId !== projectId) return;
      const payload = editorState.segments.map((segment, ordinal) => toPersistedSegment(segment, projectId, ordinal));
      const currentSignature = signatureOf(payload);
      if (currentSignature === persistedSignature.current) return;
      onSaveStateChange?.({ status: "saving", message: "正在保存" });
      void persistEditorSnapshot(projectId, editorState.segments)
        .then(() => onSaveStateChange?.({ status: "saved", savedAt: new Date().toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" }) }))
        .catch((error) => { onSaveStateChange?.({ status: "error", message: String(error) }); message.error(String(error)); });
    }, 500);
    return () => window.clearTimeout(timer);
  }, [loadedProjectId, onSaveStateChange, persistEditorSnapshot, projectId, segments]);
  useEffect(() => {
    const ttsReady = projectSegments.length > 0 && projectSegments.every((segment) => segment.ttsState === "ready" && segment.status !== "warning");
    if (!active || !projectId || !previewMedia || previewMedia.dubbed || !previewEnabled || !ttsReady) return;
    const timer = window.setTimeout(() => {
      void desktopBridge.preparePreviewMedia(projectId).then((prepared) => {
        if (prepared?.dubbed) queryClient.setQueryData(["preview-media", projectId, project?.updatedAt], prepared);
      }).catch((error) => message.error(`中文合成预览后台准备失败：${String(error)}`));
    }, 1_000);
    return () => window.clearTimeout(timer);
  }, [active, previewEnabled, previewMedia, projectId, project?.updatedAt, projectSegments, queryClient]);
  const current = segments.find((segment) => segment.id === selectedId) ?? segments[0];
  useEffect(() => {
    const video = videoRef.current;
    const segment = segments.find((item) => item.id === selectedId);
    if (video && segment) video.currentTime = segment.startMs / 1000;
  }, [selectedId]);
  useEffect(() => {
    if (!active) videoRef.current?.pause();
  }, [active]);
  useLayoutEffect(() => {
    if (active) markEditorReady();
  }, [active]);
  const togglePlayback = useCallback(async () => {
    const video = videoRef.current;
    if (!video) { togglePlaying(); return; }
    if (video.paused) await video.play(); else video.pause();
  }, [togglePlaying]);
  const regenerateSegment = useCallback(async (segmentId: string) => {
    setRegeneratingSegmentId(segmentId);
    try {
      if (projectId && desktopBridge.isDesktop()) {
        await persistEditorSnapshot(projectId, useEditorStore.getState().segments);
      }
      await onRegenerate(segmentId);
      const issues = useEditorStore.getState().segments.filter((segment) => segment.status === "warning" || segment.ttsState === "failed");
      const next = issues.find((segment) => segment.id !== segmentId);
      if (next) { useEditorStore.getState().selectSegment(next.id); useEditorStore.getState().setInspectorTab(next.ttsState === "failed" ? "voice" : "align"); }
    } finally { setRegeneratingSegmentId(null); }
  }, [onRegenerate, persistEditorSnapshot, projectId]);
  const regenerateAll = useCallback(async () => {
    if (projectId && desktopBridge.isDesktop()) {
      await persistEditorSnapshot(projectId, useEditorStore.getState().segments);
    }
    await onRegenerateAll();
  }, [onRegenerateAll, persistEditorSnapshot, projectId]);
  const previewVoice = useCallback(async (segmentId: string) => {
    let segment = useEditorStore.getState().segments.find((item) => item.id === segmentId);
    if (!segment?.scriptDocument) return;
    if (projectId && desktopBridge.isDesktop()) {
      await persistEditorSnapshot(projectId, useEditorStore.getState().segments);
      segment = useEditorStore.getState().segments.find((item) => item.id === segmentId);
      if (!segment?.scriptDocument) return;
    }
    const preview = await desktopBridge.previewTts({
      segmentId,
      scriptRevision: persistedSegments.current.get(segmentId)?.scriptRevision ?? segment.scriptRevision ?? projectSegments.find((item) => item.id === segmentId)?.scriptRevision ?? 1,
      document: segment.scriptDocument,
      providerId: voices.find((voice) => voice.id === segment.voice)?.providerId ?? ttsProviderId,
      voiceId: segment.voice,
      style: segment.ttsStyle,
      speed: segment.speed,
    });
    if (!preview?.path) return;
    const audio = new Audio(desktopBridge.isDesktop() ? convertFileSrc(preview.path) : preview.path);
    await audio.play();
  }, [persistEditorSnapshot, projectId, projectSegments, ttsProviderId, voices]);
  const changeProjectVoice = useCallback(async (voice: TtsVoice) => {
    if (!projectId) return;
    if (!desktopBridge.isDesktop()) { setProjectVoice(voice.id); return; }
    try {
      await desktopBridge.updateProjectTtsSettings({
        projectId,
        providerId: voice.providerId,
        voiceId: voice.id,
        style: project?.ttsStyle ?? "auto",
        settingsJson: project?.ttsSettingsJson ?? "{}",
        directorEnabled: project?.ttsDirectorEnabled ?? true,
        syncMode: project?.ttsSyncMode ?? "strict",
      });
      setProjectVoice(voice.id);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["projects"] }),
        queryClient.invalidateQueries({ queryKey: ["segments", projectId] }),
        queryClient.invalidateQueries({ queryKey: ["readiness", projectId] }),
      ]);
      message.success(`项目默认声音已更新为 ${voice.name}；之后的整片生成和待更新片段都会使用它`);
    } catch (error) {
      message.error(`切换项目音色失败：${String(error)}`);
    }
  }, [project?.ttsDirectorEnabled, project?.ttsSettingsJson, project?.ttsStyle, project?.ttsSyncMode, project?.ttsVoiceId, projectId, queryClient, setProjectVoice]);
  const changeSyncMode = useCallback(async (syncMode: "strict" | "balanced" | "narration" | "semantic") => {
    if (!projectId || !project || syncMode === (project.ttsSyncMode ?? "strict")) return;
    try {
      await desktopBridge.updateProjectTtsSettings({
        projectId,
        providerId: project.ttsProviderId ?? "system",
        voiceId: project.ttsVoiceId,
        style: project.ttsStyle ?? "auto",
        settingsJson: project.ttsSettingsJson ?? "{}",
        directorEnabled: project.ttsDirectorEnabled ?? true,
        syncMode,
      });
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["projects"] }),
        queryClient.invalidateQueries({ queryKey: ["segments", projectId] }),
        queryClient.invalidateQueries({ queryKey: ["readiness", projectId] }),
      ]);
      message.success(syncMode === "semantic" ? "已切换语义旁白；整片生成时将先按场景改写，再以 5–15 秒语义节拍重新对齐" : syncMode === "narration" ? "已切换旧版连续旁白" : syncMode === "balanced" ? "已切换平衡模式；现有配音已标记待更新，请重新生成整片" : "已切换严格同步；现有配音已标记待更新，请重新生成整片");
    } catch (error) {
      message.error(`切换配音连续性失败：${String(error)}`);
    }
  }, [project, projectId, queryClient]);
  const runDirector = useCallback(async (segmentId: string) => {
    const segment = useEditorStore.getState().segments.find((item) => item.id === segmentId);
    if (!segment) return;
    if (!desktopBridge.isDesktop()) return;
    if (projectId) await persistEditorSnapshot(projectId, useEditorStore.getState().segments);
    const document = await desktopBridge.planDirector({ segmentId, style: segment.ttsStyle });
    if (!document) return;
    useEditorStore.getState().updateSegment(segmentId, {
      scriptDocument: document,
      spokenZh: plainTextOf(document),
      directorEnabled: true,
    });
    message.success("已重新生成本片段演绎标记；人工调整仍可撤销");
  }, [persistEditorSnapshot, projectId]);

  const timingWarnings = segments.filter((segment) => segment.status === "warning" && segment.ttsState !== "failed");
  const currentSyncUnit = useMemo(() => {
    const units = project?.ttsSyncMode === "semantic" ? semanticScenes(segments) : project?.ttsSyncMode === "narration" ? narrationChapters(segments) : balancedBlocks(segments);
    return units.find((block) => block.some((segment) => segment.id === selectedId)) ?? (current ? [current] : []);
  }, [current, project?.ttsSyncMode, segments, selectedId]);
  const blockingFailures = segments.filter((segment) => segment.status !== "warning" && (segment.ttsState === "failed" || ["missing", "stale"].includes(segment.ttsState ?? "")));
  const ttsProgressMatch = activeJob?.status === "running" && ["tts", "semantic_narration"].includes(activeJob.stage) ? activeJob.checkpoint?.match(/^(?:tts:(segment|chapter|scene)|semantic:(scene))-(\d+)\/(\d+)$/) : null;
  const needsInitialFullTts = segments.length > 0
    && blockingFailures.length === segments.length
    && segments.every((segment) => ["missing", "stale"].includes(segment.ttsState ?? "") && (!segment.ttsDurationMs || project?.ttsSyncMode !== "strict"));
  const selectIssue = useCallback((kind: "timing" | "failed") => {
    const issues = kind === "timing" ? timingWarnings : blockingFailures;
    if (!issues.length) return;
    const index = issues.findIndex((segment) => segment.id === selectedId);
    const next = issues[(index + 1) % issues.length];
    selectSegment(next.id);
    setInspectorTab(kind === "timing" ? "align" : "voice");
  }, [blockingFailures, selectSegment, selectedId, setInspectorTab, timingWarnings]);
  const saveNow = useCallback(async () => {
    if (!projectId) return;
    onSaveStateChange?.({ status: "saving", message: "正在保存" });
    try {
      await persistEditorSnapshot(projectId, useEditorStore.getState().segments);
      onSaveStateChange?.({ status: "saved", savedAt: new Date().toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" }) });
    } catch (error) { onSaveStateChange?.({ status: "error", message: String(error) }); }
  }, [onSaveStateChange, persistEditorSnapshot, projectId]);
  useEffect(() => {
    if (!active) return;
    const handler = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.matches("input, textarea, select, [contenteditable='true']")) return;
      if (event.code === "Space") { event.preventDefault(); void togglePlayback(); }
      else if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "z") { event.preventDefault(); event.shiftKey ? redo() : undo(); }
      else if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") { event.preventDefault(); void saveNow(); }
      else if (event.key === "ArrowUp" || event.key === "ArrowDown") { event.preventDefault(); const index = segments.findIndex((segment) => segment.id === selectedId); const next = event.key === "ArrowUp" ? segments[index - 1] : segments[index + 1]; if (next) selectSegment(next.id); }
      else if (event.key === "[") { event.preventDefault(); selectIssue("failed"); }
      else if (event.key === "]") { event.preventDefault(); selectIssue("timing"); }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [active, redo, saveNow, segments, selectIssue, selectSegment, selectedId, togglePlayback, undo]);

  if (!projectId) {
    return <div className="editor-page editor-empty-page">
      <div className="editor-actionbar empty"><div className="project-context"><strong>编辑器</strong><span>尚未打开项目</span></div></div>
      <section className="editor-empty-state" aria-labelledby="editor-empty-title">
        <div className="editor-empty-visual" aria-hidden="true">
          <FileVideo size={48} weight="duotone" />
          <span><Waveform size={27} weight="bold" /></span>
          <i><Sparkle size={19} weight="fill" /></i>
        </div>
        <span className="eyebrow">开始第一个中文配音项目</span>
        <h1 id="editor-empty-title">这里还没有可编辑的项目</h1>
        <p>导入一个英文视频，译声工坊会在本地完成识别，并使用你选择的服务商生成中文字幕与中文配音。</p>
        <Button type="primary" size="large" icon={<PlusIcon />} onClick={onCreate}>新建项目</Button>
        <div className="editor-empty-steps" aria-label="项目生成步骤">
          <span><CheckCircle weight="fill" />导入本地视频</span>
          <span><CheckCircle weight="fill" />选择翻译与配音服务</span>
          <span><CheckCircle weight="fill" />自动生成并在这里编辑</span>
        </div>
        <small>原始视频不会被复制或上传；第三方服务只接收界面中明确说明的文本。</small>
      </section>
      <footer className="editor-statusbar"><span>等待创建项目</span><em>存储空间&nbsp; 本地 1.23 TB 可用 <i /></em></footer>
    </div>;
  }

  return <div className="editor-page">
    <div className="editor-actionbar">
      <div className="project-context"><strong>{project?.name ?? "Building Reliable AI Agents"}</strong><span>{project?.workflowMode === "review" ? "先校对模式" : "快速生成模式"}</span></div>
      <div className="undo-group"><Button type="text" disabled={!history.length} icon={<UndoIcon />} onClick={undo}>撤销</Button><Button type="text" disabled={!future.length} icon={<RedoIcon />} onClick={redo}>重做</Button></div>
      <div className="editor-spacer" />
      <span className={`project-state ${readiness?.phase ?? "processing"}`}><i />{readiness?.nextAction ?? "本地处理"}</span>
      <Popconfirm
        title="重新生成整片中文配音？"
        description={`将按当前声音和口播稿重新生成 ${segments.length} 个片段，在线服务可能产生费用。`}
        okText="开始生成"
        cancelText="取消"
        onConfirm={regenerateAll}
      >
        <Button size="small" icon={<Waveform size={15} />}>整片重新配音</Button>
      </Popconfirm>
      <Button type="primary" size="small" icon={<ExportIcon />} onClick={onExport}>{readiness?.blockingCount ? `导出 · ${readiness.blockingCount} 个问题` : readiness?.warningCount ? `导出 · ${readiness.warningCount} 条提醒` : "导出"}</Button>
    </div>
    {(timingWarnings.length > 0 || blockingFailures.length > 0 || fitResult) && <section className={`duration-task-banner ${blockingFailures.length ? "blocking" : "warning"}`} aria-live="polite">
      <div className="duration-task-icon">{blockingFailures.length ? <Warning weight="fill" /> : <Info weight="fill" />}</div>
      <div className="duration-task-copy">
        <strong>{needsInitialFullTts ? project?.ttsSyncMode === "semantic" ? "语义旁白需要重新理解并生成整片" : project?.ttsSyncMode === "narration" ? "连续旁白需要生成一次章节音轨" : project?.ttsSyncMode === "balanced" ? "平衡模式需要生成一次连续语音块" : "尚未完成首次全片配音" : blockingFailures.length ? `${blockingFailures.length} 个片段尚未成功生成配音` : timingWarnings.length ? `${timingWarnings.length} 个片段的配音超过原视频时间窗` : "自动修复已完成"}</strong>
        <span>{needsInitialFullTts ? project?.ttsSyncMode === "semantic" ? "阿里百炼先按 30–60 秒场景理解原意并重写口播，再由项目当前语音服务落到 5–15 秒画面锚点连续合成；允许语义改写，但不会长期偏离画面。" : project?.ttsSyncMode === "narration" ? "将按约 60–100 秒自然章节连接阿里百炼 Realtime。" : project?.ttsSyncMode === "balanced" ? "将把相邻字幕组合为 5–15 秒语音块并使用项目当前语音服务连续合成，减少每句重新起范；字幕时间码不会改变。" : "当前还没有可复用的片段音频。请先生成整片配音，完成后才可以单独重试片段。" : blockingFailures.length ? "这些问题会阻止导出。建议先逐个重试失败片段。" : timingWarnings.length ? "可能出现语音重叠。推荐先自动缩短口播稿并重新校验，字幕译文不会改变。" : "所有时长问题均已解决，项目现在可以安全导出。"}</span>
        {ttsProgressMatch && <div className="fit-progress"><i style={{ width: `${activeJob?.progress ?? 0}%` }} /><em>{activeJob?.stage === "semantic_narration" ? `正在改写语义场景 ${ttsProgressMatch[3]}/${ttsProgressMatch[4]}` : ttsProgressMatch[1] === "chapter" ? Number(ttsProgressMatch[3]) === 0 ? "正在连接阿里百炼并发送第 1 章正文" : `已完成 ${ttsProgressMatch[3]} / ${ttsProgressMatch[4]} 个连续旁白章节` : ttsProgressMatch[1] === "scene" ? `正在合成语义场景 ${ttsProgressMatch[3]}/${ttsProgressMatch[4]}` : `正在合成语音块 ${ttsProgressMatch[3]}/${ttsProgressMatch[4]}`} · {activeJob?.progress ?? 0}%</em></div>}
        {fittingWarnings && fitProgress && <div className="fit-progress"><i style={{ width: `${fitProgress.progress}%` }} /><em>{fitProgress.stage === "compressing" ? "正在压缩口播稿" : fitProgress.stage === "synthesizing" ? "正在重新合成" : "正在校验时长"} · {fitProgress.progress}%</em></div>}
        {!fittingWarnings && fitResult && <small>上次自动修复：已解决 {fitResult.resolvedCount} 个，剩余 {fitResult.remainingIds.length} 个需要确认。</small>}
      </div>
      <div className="duration-task-actions">
        {needsInitialFullTts && !ttsProgressMatch && <Popconfirm title={project?.ttsSyncMode === "semantic" ? "生成语义旁白整片音轨？" : project?.ttsSyncMode === "narration" ? "生成连续旁白整片音轨？" : project?.ttsSyncMode === "balanced" ? "按连续语音块重新生成整片？" : "生成首次全片中文配音？"} description={project?.ttsSyncMode === "semantic" ? "阿里百炼会接收场景文字与字幕用于语义改写；项目当前语音服务只接收中文口播稿和合成参数。不上传原视频或原声，在线服务可能产生费用。" : project?.ttsSyncMode === "narration" ? "将仅向阿里百炼发送中文旁白章节和合成参数，在线服务可能产生费用。" : project?.ttsSyncMode === "balanced" ? "将使用项目当前语音服务按 5–15 秒连续语音块合成，在线服务可能产生费用。" : `将使用项目当前服务商生成 ${segments.length} 个片段，在线服务可能产生费用。`} okText="开始生成" cancelText="取消" onConfirm={regenerateAll}><Button type="primary">{project?.ttsSyncMode === "semantic" ? "生成语义旁白整片配音" : project?.ttsSyncMode === "narration" ? "生成连续旁白整片配音" : project?.ttsSyncMode === "balanced" ? "生成平衡模式整片配音" : "首次生成整片配音"}</Button></Popconfirm>}
        {timingWarnings.length > 0 && <Button type="primary" loading={fittingWarnings} onClick={() => onFitWarnings()}>{fittingWarnings ? "正在自动修复" : `自动修复 ${timingWarnings.length} 个片段`}</Button>}
        {!needsInitialFullTts && (blockingFailures.length > 0 || timingWarnings.length > 0) && <Button onClick={() => selectIssue(blockingFailures.length ? "failed" : "timing")}>{blockingFailures.length ? "定位失败片段" : "逐个检查"}</Button>}
        {fitResult?.undoAvailable && <Button onClick={onUndoFit}>撤销自动修复</Button>}
        {(timingWarnings.length > 0 || needsInitialFullTts) && <Tooltip title={needsInitialFullTts ? "首次整片生成会建立可复用的片段缓存；之后只需重新生成有修改或失败的片段。" : "翻译后的中文通常比原语音更长。系统会优先精简口播稿；常规加速最高 1.08x，极短片段可在 1.25x 内兜底适配。"}><Button type="text">为什么会出现</Button></Tooltip>}
      </div>
    </section>}
    <div className="editor-stage">
      <section className="preview-panel">
        <div className="video-frame">
          {previewUrl ? <video key={previewUrl} ref={videoRef} src={previewUrl} muted={muted} onLoadedMetadata={(event) => {
            const targetSeconds = useEditorStore.getState().playheadMs / 1_000;
            if (Number.isFinite(targetSeconds) && targetSeconds > 0 && targetSeconds < event.currentTarget.duration) {
              event.currentTarget.currentTime = targetSeconds;
            }
          }} onTimeUpdate={(event) => setPlayhead(event.currentTarget.currentTime * 1000)} onPlay={() => !playing && togglePlaying()} onPause={() => playing && togglePlaying()} /> : <div className="video-placeholder">{previewFailed ? <><strong>预览准备失败</strong><Button size="small" onClick={() => retryPreview()}>重试</Button></> : previewEnabled && previewPending ? "正在准备中文合成预览…" : "完成媒体准备后可预览本地视频"}</div>}
          {previewMedia && <div className={`preview-source-badge ${previewMedia.dubbed ? "dubbed" : "source"}`}><Waveform size={14} weight="bold" />{previewMedia.dubbed ? "中文合成预览" : project?.progress && project.progress >= 80 ? "原始代理 · 正在后台更新中文预览" : "原始代理 · 等待中文配音"}</div>}
          <div className="subtitle-overlay"><span>{current?.sourceText}</span><strong>{current?.subtitleZh}</strong></div>
        </div>
        <div className="playback-bar"><PlaybackTime durationMs={project?.durationMs ?? 0} videoRef={videoRef} /><div className="transport"><Tooltip title="上一片段"><Button type="text" shape="circle" icon={<BackIcon />} aria-label="上一片段" onClick={() => { const index = segments.findIndex((segment) => segment.id === selectedId); if (index > 0) selectSegment(segments[index - 1].id); }} /></Tooltip><Tooltip title={playing ? "暂停" : "播放"}><Button className="play-button" type="text" shape="circle" icon={playing ? <PauseIcon /> : <PlayIcon />} aria-label={playing ? "暂停" : "播放"} onClick={togglePlayback} /></Tooltip><Tooltip title="下一片段"><Button type="text" shape="circle" icon={<ForwardIcon />} aria-label="下一片段" onClick={() => { const index = segments.findIndex((segment) => segment.id === selectedId); if (index < segments.length - 1) selectSegment(segments[index + 1].id); }} /></Tooltip></div><PlaybackOptions muted={muted} videoRef={videoRef} /></div>
      </section>
      <Timeline />
      <Inspector onRegenerate={regenerateSegment} onSmartFit={(segmentId) => onFitWarnings([segmentId])} regenerating={regeneratingSegmentId === selectedId} onPreviewVoice={desktopBridge.isDesktop() ? previewVoice : undefined} onRunDirector={desktopBridge.isDesktop() ? runDirector : undefined} onProjectVoiceChange={changeProjectVoice} voices={voices} syncMode={project?.ttsSyncMode ?? "strict"} syncBlockSize={currentSyncUnit.length} syncBlockDurationMs={currentSyncUnit.length ? currentSyncUnit.at(-1)!.endMs - currentSyncUnit[0].startMs : 0} onSyncModeChange={changeSyncMode} />
    </div>
    <footer className="editor-statusbar"><span>项目帧率&nbsp; 30 fps</span><span>音频采样率&nbsp; 48 kHz</span><span>片段总数&nbsp; {segments.length}</span><span>已处理&nbsp; {segments.filter((segment) => segment.status === "ready").length}</span><span>待处理&nbsp; {segments.filter((segment) => segment.status !== "ready").length}</span><em>存储空间&nbsp; 本地 1.23 TB 可用 <i /></em></footer>
  </div>;
});

const PlaybackTime = memo(function PlaybackTime({ durationMs, videoRef }: { durationMs: number; videoRef: React.RefObject<HTMLVideoElement | null> }) {
  const playheadMs = useEditorStore((state) => state.playheadMs);
  useEffect(() => {
    const video = videoRef.current;
    if (video && Math.abs(video.currentTime * 1000 - playheadMs) > 600) video.currentTime = playheadMs / 1000;
  }, [playheadMs, videoRef]);
  return <span className="current-time">{formatTimecode(playheadMs)} <em>/ {formatTimecode(durationMs)}</em></span>;
});

const PlaybackOptions = memo(function PlaybackOptions({ muted, videoRef }: { muted: boolean; videoRef: React.RefObject<HTMLVideoElement | null> }) {
  const toggleMuted = useEditorStore((state) => state.toggleMuted);
  const [rate, setRate] = useState(1);
  const cycleRate = () => {
    const rates = [1, 1.25, 1.5, 0.75];
    const next = rates[(rates.indexOf(rate) + 1) % rates.length];
    setRate(next);
    if (videoRef.current) videoRef.current.playbackRate = next;
  };
  return <div className="playback-options"><Tooltip title="切换预览速度"><Button type="text" size="small" onClick={cycleRate}>{rate === 1 ? "1.0x" : `${rate}x`}</Button></Tooltip><Tooltip title={muted ? "取消静音" : "静音"}><Button type="text" shape="circle" icon={muted ? <MuteIcon /> : <SpeakerIcon />} aria-label={muted ? "取消静音" : "静音"} onClick={toggleMuted} /></Tooltip><Tooltip title="全屏"><Button type="text" shape="circle" icon={<FullscreenIcon />} aria-label="全屏" onClick={() => videoRef.current?.requestFullscreen()} /></Tooltip></div>;
});
