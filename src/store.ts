import { create } from "zustand";
import type { LocalizationAnalysis, Segment } from "./domain";
import { initialSegments } from "./fixtures";

type InspectorTab = "text" | "voice" | "align";

interface EditorState {
  segments: Segment[];
  loadedProjectId: string | null;
  loadedSignature: string;
  selectedId: string;
  inspectorTab: InspectorTab;
  playheadMs: number;
  playing: boolean;
  zoom: number;
  muted: boolean;
  localization: LocalizationAnalysis | null;
  history: Segment[][];
  future: Segment[][];
  hydrateProject: (projectId: string, signature: string, segments: Segment[]) => boolean;
  selectSegment: (id: string) => void;
  setInspectorTab: (tab: InspectorTab) => void;
  setPlayhead: (ms: number) => void;
  togglePlaying: () => void;
  toggleMuted: () => void;
  setZoom: (zoom: number) => void;
  setLocalization: (analysis: LocalizationAnalysis | null) => void;
  updateSegment: (id: string, patch: Partial<Segment>, recordHistory?: boolean) => void;
  setProjectVoice: (voice: string) => void;
  splitSelected: () => void;
  mergeNext: () => void;
  regenerateSelected: () => void;
  undo: () => void;
  redo: () => void;
}

const withSnapshot = (state: EditorState, segments: Segment[]) => ({
  segments,
  history: [...state.history.slice(-29), state.segments],
  future: [],
});

const newSegmentId = () => crypto.randomUUID();

const plainScript = (text: string): NonNullable<Segment["scriptDocument"]> => ({
  version: 1,
  blocks: [{ type: "paragraph", children: [{ type: "text", text, origin: "manual" }] }],
});

const mergeScripts = (current: Segment, next: Segment): NonNullable<Segment["scriptDocument"]> => {
  const first = current.scriptDocument?.blocks[0]?.children ?? plainScript(current.spokenZh).blocks[0].children;
  const second = next.scriptDocument?.blocks[0]?.children ?? plainScript(next.spokenZh).blocks[0].children;
  return {
    version: 1,
    blocks: [{
      type: "paragraph",
      children: [
        ...first,
        { type: "pause", durationMs: 280, origin: "auto" },
        ...second,
      ],
    }],
  };
};

export const useEditorStore = create<EditorState>((set, get) => ({
  segments: initialSegments,
  loadedProjectId: null,
  loadedSignature: "",
  selectedId: "seg-02",
  inspectorTab: "text",
  playheadMs: 312340,
  playing: false,
  zoom: 1,
  muted: false,
  localization: null,
  history: [],
  future: [],
  hydrateProject: (projectId, signature, segments) => {
    const current = get();
    if (current.loadedProjectId === projectId && current.loadedSignature === signature) return false;
    const preserveSession = current.loadedProjectId === projectId;
    set({
      segments,
      loadedProjectId: projectId,
      loadedSignature: signature,
      selectedId: preserveSession && segments.some((segment) => segment.id === current.selectedId) ? current.selectedId : segments[0]?.id ?? "",
      playheadMs: preserveSession ? current.playheadMs : segments[0]?.startMs ?? 0,
      history: preserveSession ? current.history : [],
      future: preserveSession ? current.future : [],
      localization: preserveSession ? current.localization : null,
    });
    return true;
  },
  selectSegment: (id) => set({ selectedId: id, playheadMs: get().segments.find((segment) => segment.id === id)?.startMs ?? get().playheadMs }),
  setInspectorTab: (inspectorTab) => set({ inspectorTab }),
  setPlayhead: (playheadMs) => set({ playheadMs }),
  togglePlaying: () => set((state) => ({ playing: !state.playing })),
  toggleMuted: () => set((state) => ({ muted: !state.muted })),
  setZoom: (zoom) => set({ zoom: Math.min(2.4, Math.max(0.7, zoom)) }),
  setLocalization: (localization) => set({ localization }),
  updateSegment: (id, patch, recordHistory = true) => set((state) => {
    const current = state.segments.find((segment) => segment.id === id);
    if (!current || Object.entries(patch).every(([key, value]) => current[key as keyof Segment] === value)) return state;
    const segments = state.segments.map((segment) => segment.id === id ? { ...segment, ...patch, status: patch.status ?? "stale" } : segment);
    return recordHistory ? withSnapshot(state, segments) : { segments, future: [] };
  }),
  setProjectVoice: (voice) => set((state) => withSnapshot(state, state.segments.map((segment) => ({ ...segment, voice, status: "stale" as const })))),
  splitSelected: () => set((state) => {
    const index = state.segments.findIndex((segment) => segment.id === state.selectedId);
    const current = state.segments[index];
    if (!current || current.locked || current.endMs - current.startMs < 600) return state;
    const midpoint = Math.round((current.startMs + current.endMs) / 2);
    // Without word-level alignment, automatically splitting the structured
    // document would silently move protected terms. Keep the first draft and
    // create an explicit empty second script for the user to distribute.
    const first = { ...current, id: newSegmentId(), endMs: midpoint, status: "stale" as const };
    const second = { ...current, id: newSegmentId(), startMs: midpoint, spokenZh: "", scriptDocument: plainScript(""), linked: false, status: "stale" as const };
    const segments = [...state.segments.slice(0, index), first, second, ...state.segments.slice(index + 1)];
    return { ...withSnapshot(state, segments), selectedId: first.id };
  }),
  mergeNext: () => set((state) => {
    const index = state.segments.findIndex((segment) => segment.id === state.selectedId);
    const current = state.segments[index];
    const next = state.segments[index + 1];
    if (!current || !next || current.locked || next.locked) return state;
    const merged: Segment = {
      ...current,
      id: newSegmentId(),
      endMs: next.endMs,
      sourceText: `${current.sourceText} ${next.sourceText}`,
      subtitleZh: `${current.subtitleZh}${next.subtitleZh}`,
      spokenZh: `${current.spokenZh}${next.spokenZh}`,
      scriptDocument: mergeScripts(current, next),
      voice: current.voice === next.voice ? current.voice : "system-tingting",
      ttsStyle: current.ttsStyle === next.ttsStyle ? current.ttsStyle : "auto",
      status: "stale",
      overflowMs: undefined,
    };
    const segments = [...state.segments.slice(0, index), merged, ...state.segments.slice(index + 2)];
    return { ...withSnapshot(state, segments), selectedId: merged.id };
  }),
  regenerateSelected: () => {
    const id = get().selectedId;
    set((state) => ({ segments: state.segments.map((segment) => segment.id === id ? { ...segment, status: "processing", overflowMs: undefined } : segment) }));
    window.setTimeout(() => set((state) => ({ segments: state.segments.map((segment) => segment.id === id ? { ...segment, status: "ready", overflowMs: undefined } : segment) })), 1100);
  },
  undo: () => set((state) => {
    const previous = state.history.at(-1);
    if (!previous) return state;
    return { segments: previous, history: state.history.slice(0, -1), future: [state.segments, ...state.future] };
  }),
  redo: () => set((state) => {
    const next = state.future[0];
    if (!next) return state;
    return { segments: next, history: [...state.history, state.segments], future: state.future.slice(1) };
  }),
}));
