import { memo, useEffect, useMemo, useRef, useState } from "react";
import { useShallow } from "zustand/shallow";
import { Button, Slider, Tooltip, message } from "antd";
import { ArrowsMerge, CaretLeft, CaretRight, Eye, MagicWand, Magnet, Minus, Plus, Scissors } from "@phosphor-icons/react";
import { formatTimecode } from "../domain";
import { useEditorStore } from "../store";
import { WaveformCanvas } from "./WaveformCanvas";
import { antdIcon } from "../ui/icons";
import { desktopBridge } from "../bridge";
import { useQueryClient } from "@tanstack/react-query";

const SplitIcon = antdIcon(Scissors);
const MergeIcon = antdIcon(ArrowsMerge);
const MagnetIcon = antdIcon(Magnet);

const BASE_VIEW_DURATION = 21000;

export const Timeline = memo(function Timeline({ audioMode = "duck" }: { audioMode?: string }) {
  const queryClient = useQueryClient();
  const trackRef = useRef<HTMLDivElement>(null);
  const [snapping, setSnapping] = useState(true);
  const [panMs, setPanMs] = useState(0);
  const { segments, loadedProjectId, selectedId, selectSegment, setPlayhead, zoom, setZoom, updateSegment, splitSelected, mergeNext, localization, setLocalization } = useEditorStore(useShallow((state) => ({
    segments: state.segments, selectedId: state.selectedId, selectSegment: state.selectSegment, setPlayhead: state.setPlayhead,
    loadedProjectId: state.loadedProjectId, zoom: state.zoom, setZoom: state.setZoom, updateSegment: state.updateSegment, splitSelected: state.splitSelected, mergeNext: state.mergeNext, localization: state.localization, setLocalization: state.setLocalization,
  })));
  const [analyzing, setAnalyzing] = useState(false);
  const duration = BASE_VIEW_DURATION / zoom;
  const selectedIndex = segments.findIndex((segment) => segment.id === selectedId);
  const selected = segments[selectedIndex];
  const nextSegment = segments[selectedIndex + 1];
  const splitDisabledReason = !selected
    ? "请先选择一个片段"
    : selected.locked
      ? "请先解锁当前片段的时间边界"
      : selected.endMs - selected.startMs < 600
        ? "片段短于 0.6 秒，无法继续拆分"
        : null;
  const mergeDisabledReason = !selected
    ? "请先选择一个片段"
    : !nextSegment
      ? "已经是最后一个片段"
      : selected.locked || nextSegment.locked
        ? "请先解锁相邻片段的时间边界"
        : null;
  const projectEnd = segments.at(-1)?.endMs ?? duration;
  const baseViewStart = (selected?.startMs ?? segments[0]?.startMs ?? 0) - duration * 0.28;
  const viewStart = Math.min(Math.max(0, projectEnd - duration), Math.max(0, baseViewStart + panMs));
  const end = viewStart + duration;
  const ticks = useMemo(() => Array.from({ length: 8 }, (_, index) => viewStart + index * duration / 7), [duration, viewStart]);
  const position = (ms: number) => ((ms - viewStart) / duration) * 100;
  const visible = segments.filter((segment) => segment.endMs >= viewStart && segment.startMs <= end);
  useEffect(() => setPanMs(0), [selectedId]);
  const panTimeline = (delta: number) => setPanMs((current) => current + delta);

  const analyzeSync = async () => {
    if (!loadedProjectId) return;
    setAnalyzing(true);
    try {
      const analysis = await desktopBridge.analyzeLocalization(loadedProjectId, true);
      setLocalization(analysis);
      message.success(analysis?.timelineEdits.length ? `发现 ${analysis.timelineEdits.length} 个可优化区间` : "音画节奏已经较紧凑");
    } catch (error) {
      message.error(`同步分析失败：${String(error)}`);
    } finally {
      setAnalyzing(false);
    }
  };

  const toggleTimelineEdit = async (editId: string, accepted: boolean) => {
    if (!loadedProjectId) return;
    try {
      const analysis = await desktopBridge.acceptTimelineEdit(loadedProjectId, editId, accepted);
      setLocalization(analysis);
      const prepared = await desktopBridge.preparePreviewMedia(loadedProjectId);
      if (prepared) queryClient.setQueriesData({ queryKey: ["preview-media", loadedProjectId] }, prepared);
      message.success(accepted ? "已加入导出时间线，可再次点击撤销" : "已从导出时间线移除");
    } catch (error) {
      message.error(String(error));
    }
  };

  const moveBoundary = (id: string, side: "start" | "end", event: React.PointerEvent) => {
    event.preventDefault();
    event.stopPropagation();
    const target = trackRef.current;
    if (!target) return;
    const selectedIndex = segments.findIndex((segment) => segment.id === id);
    const segment = segments[selectedIndex];
    if (!segment || segment.locked) return;
    const rect = target.getBoundingClientRect();
    const snapPoints = snapping ? segments.flatMap((item) => [item.startMs, item.endMs]).sort((left, right) => left - right) : [];
    const nearestSnapPoint = (raw: number) => {
      if (!snapPoints.length) return raw;
      let low = 0;
      let high = snapPoints.length;
      while (low < high) {
        const middle = (low + high) >>> 1;
        if (snapPoints[middle] < raw) low = middle + 1; else high = middle;
      }
      const before = snapPoints[Math.max(0, low - 1)];
      const after = snapPoints[Math.min(snapPoints.length - 1, low)];
      const nearest = Math.abs(before - raw) <= Math.abs(after - raw) ? before : after;
      return Math.abs(nearest - raw) <= 80 ? nearest : raw;
    };
    let frame = 0;
    let recordedHistory = false;
    let latestEvent: PointerEvent | null = null;
    const applyLatest = () => {
      frame = 0;
      if (!latestEvent) return;
      const raw = viewStart + ((latestEvent.clientX - rect.left) / rect.width) * duration;
      const nearest = nearestSnapPoint(raw);
      const currentSegment = useEditorStore.getState().segments.find((item) => item.id === id);
      if (!currentSegment) return;
      if (side === "start") {
        const previousEnd = segments[selectedIndex - 1]?.endMs ?? 0;
        const startMs = Math.round(Math.min(segment.endMs - 300, Math.max(previousEnd, nearest)));
        if (startMs === currentSegment.startMs) return;
        updateSegment(id, { startMs }, !recordedHistory);
      } else {
        const nextStart = segments[selectedIndex + 1]?.startMs ?? end;
        const endMs = Math.round(Math.max(segment.startMs + 300, Math.min(nextStart, nearest)));
        if (endMs === currentSegment.endMs) return;
        updateSegment(id, { endMs }, !recordedHistory);
      }
      recordedHistory = true;
    };
    const onMove = (moveEvent: PointerEvent) => {
      latestEvent = moveEvent;
      if (!frame) frame = window.requestAnimationFrame(applyLatest);
    };
    const onUp = () => {
      if (frame) {
        window.cancelAnimationFrame(frame);
        applyLatest();
      }
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
  };

  return <section className="timeline-panel">
    <div className="timeline-toolbar">
      <div className="tool-group">
        <Tooltip title={splitDisabledReason ?? "从片段中点拆分"}><span><Button className="tool-button" icon={<SplitIcon />} aria-label="拆分片段" disabled={Boolean(splitDisabledReason)} onClick={splitSelected} /></span></Tooltip>
        <Tooltip title={mergeDisabledReason ?? "与下一个片段合并"}><span><Button className="tool-button" icon={<MergeIcon />} aria-label="合并下一片段" disabled={Boolean(mergeDisabledReason)} onClick={mergeNext} /></span></Tooltip>
        <Tooltip title={snapping ? "关闭片段边界吸附" : "开启片段边界吸附"}><Button className={`tool-button ${snapping ? "active-soft" : ""}`} icon={<MagnetIcon />} aria-label="片段边界吸附" aria-pressed={snapping} onClick={() => setSnapping(!snapping)} /></Tooltip>
      </div>
      <Tooltip title="分析停顿、静止画面与同步锚点"><Button className="sync-analysis-button" size="small" icon={<MagicWand />} loading={analyzing} onClick={analyzeSync}>优化音画同步</Button></Tooltip>
      <TimelineTimecode />
      <div className="timeline-pan"><Tooltip title="向左移动时间轴"><Button type="text" size="small" icon={<CaretLeft />} aria-label="向左移动时间轴" onClick={() => panTimeline(-duration * 0.7)} /></Tooltip><span>左右滑动</span><Tooltip title="向右移动时间轴"><Button type="text" size="small" icon={<CaretRight />} aria-label="向右移动时间轴" onClick={() => panTimeline(duration * 0.7)} /></Tooltip></div>
      <div className="zoom-control"><span>缩放</span><Minus size={14} /><Slider aria-label="时间轴缩放" min={0.7} max={2.4} step={0.1} value={zoom} onChange={setZoom} tooltip={{ open: false }} /><Plus size={14} /></div>
    </div>
    <div className="timeline-body">
      <div className="track-labels">
        <div className="ruler-label" />
        {[{ title: "场景与画面", sub: localization ? `${localization.scenes.length} 场景 · ${localization.anchors.length} 锚点` : "等待智能同步分析" }, { title: "背景声音", sub: audioMode === "separate" ? "去人声背景轨 · 本地分离" : audioMode === "mute" ? "不保留原声" : "完整原声动态避让" }, { title: "中文配音", sub: "场景级连贯旁白" }, { title: "双语字幕", sub: `${segments.length} 个片段` }].map((track) => <div className="track-label" key={track.title}><Eye /><span><strong>{track.title}</strong><small>{track.sub}</small></span></div>)}
      </div>
      <div className="track-content" ref={trackRef} onWheel={(event) => {
        const delta = Math.abs(event.deltaX) > 0 ? event.deltaX : event.shiftKey ? event.deltaY : 0;
        if (!delta) return;
        event.preventDefault();
        panTimeline(delta * duration / 900);
      }} onPointerDown={(event) => {
        const rect = event.currentTarget.getBoundingClientRect();
        setPlayhead(viewStart + ((event.clientX - rect.left) / rect.width) * duration);
      }}>
        <div className="time-ruler">{ticks.map((tick) => <span key={tick} style={{ left: `${position(tick)}%` }}><i />{formatTimecode(tick, true)}</span>)}</div>
        <div className="audio-track original localization-track"><WaveformCanvas seed={11} />{localization?.scenes.filter((scene) => scene.sourceEndMs >= viewStart && scene.sourceStartMs <= end).map((scene) => <span className="scene-range" key={scene.id} style={{ left: `${position(scene.sourceStartMs)}%`, width: `${Math.max(1, position(scene.sourceEndMs) - position(scene.sourceStartMs))}%` }} title={scene.spokenZh}>场景 {scene.ordinal + 1}</span>)}{localization?.anchors.filter((anchor) => anchor.sourceTimeMs >= viewStart && anchor.sourceTimeMs <= end).map((anchor) => <i className={`sync-anchor ${anchor.priority}`} key={anchor.id} style={{ left: `${position(anchor.sourceTimeMs)}%` }} title={`${anchor.priority === "exact" ? "精确" : "邻近"}锚点 · ${anchor.phrase}`} />)}{localization?.timelineEdits.filter((edit) => edit.sourceEndMs >= viewStart && edit.sourceStartMs <= end).map((edit) => <button type="button" className={`timeline-edit ${edit.operation} ${edit.accepted ? "accepted" : "suggested"}`} key={edit.id} style={{ left: `${position(edit.sourceStartMs)}%`, width: `${Math.max(1, position(edit.sourceEndMs) - position(edit.sourceStartMs))}%` }} title={`${edit.reason} · ${edit.accepted ? "点击撤销" : "点击采用"}`} aria-pressed={edit.accepted} onPointerDown={(event) => event.stopPropagation()} onClick={(event) => { event.stopPropagation(); void toggleTimelineEdit(edit.id, !edit.accepted); }}>{edit.operation === "cut" ? "裁剪" : `${edit.rate?.toFixed(1)}×`}</button>)}</div>
        <div className="audio-track background"><WaveformCanvas seed={29} color="#3b8b62" /></div>
        <div className="segment-track dub-track">{visible.map((segment) => {
          const durationPending = segment.status === "warning" && !segment.ttsDurationMs;
          const warningLabel = durationPending ? "时长待适配" : segment.overflowMs ? `时长超出 ${(segment.overflowMs / 1000).toFixed(1)} 秒` : "";
          return <button key={segment.id} data-segment-id={segment.id} aria-label={`${segment.spokenZh}${warningLabel ? `，${warningLabel}` : ""}`} className={`timeline-segment dub ${selectedId === segment.id ? "selected" : ""} ${segment.status}`} style={{ left: `${position(segment.startMs)}%`, width: `${Math.max(1.3, position(segment.endMs) - position(segment.startMs))}%` }} onClick={(event) => { event.stopPropagation(); selectSegment(segment.id); }}><span>{segment.spokenZh}</span>{segment.overflowMs && <em><strong aria-hidden="true">!</strong> {warningLabel}</em>}<i className="resize-handle start" onPointerDown={(event) => moveBoundary(segment.id, "start", event)} /><i className="resize-handle end" onPointerDown={(event) => moveBoundary(segment.id, "end", event)} /></button>;
        })}</div>
        <div className="segment-track subtitle-track">{visible.map((segment) => <button key={segment.id} className={`timeline-segment subtitle ${selectedId === segment.id ? "selected" : ""}`} style={{ left: `${position(segment.startMs)}%`, width: `${Math.max(1.3, position(segment.endMs) - position(segment.startMs))}%` }} onClick={(event) => { event.stopPropagation(); selectSegment(segment.id); }}><span>{segment.sourceText}</span><strong>{segment.subtitleZh}</strong><small>{formatTimecode(segment.startMs, true)} – {formatTimecode(segment.endMs, true)}</small></button>)}</div>
        <TimelinePlayhead viewStart={viewStart} duration={duration} />
      </div>
    </div>
  </section>;
});

const TimelineTimecode = memo(function TimelineTimecode() {
  const playheadMs = useEditorStore((state) => state.playheadMs);
  return <div className="timeline-timecode">{formatTimecode(playheadMs)}</div>;
});

const TimelinePlayhead = memo(function TimelinePlayhead({ viewStart, duration }: { viewStart: number; duration: number }) {
  const playheadMs = useEditorStore((state) => state.playheadMs);
  return <div className="playhead" style={{ left: `${((playheadMs - viewStart) / duration) * 100}%` }}><i /></div>;
});
