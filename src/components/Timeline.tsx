import { memo, useMemo, useRef, useState } from "react";
import { useShallow } from "zustand/shallow";
import { Button, Slider, Tooltip } from "antd";
import { ArrowsMerge, CursorClick, Eye, LockSimple, Magnet, Minus, Plus, Scissors, SpeakerHigh } from "@phosphor-icons/react";
import { formatTimecode } from "../domain";
import { useEditorStore } from "../store";
import { WaveformCanvas } from "./WaveformCanvas";
import { antdIcon } from "../ui/icons";

const SelectIcon = antdIcon(CursorClick);
const SplitIcon = antdIcon(Scissors);
const MergeIcon = antdIcon(ArrowsMerge);
const MagnetIcon = antdIcon(Magnet);

const BASE_VIEW_DURATION = 21000;

export const Timeline = memo(function Timeline() {
  const trackRef = useRef<HTMLDivElement>(null);
  const [snapping, setSnapping] = useState(true);
  const { segments, selectedId, selectSegment, setPlayhead, zoom, setZoom, updateSegment, splitSelected, mergeNext } = useEditorStore(useShallow((state) => ({
    segments: state.segments, selectedId: state.selectedId, selectSegment: state.selectSegment, setPlayhead: state.setPlayhead,
    zoom: state.zoom, setZoom: state.setZoom, updateSegment: state.updateSegment, splitSelected: state.splitSelected, mergeNext: state.mergeNext,
  })));
  const duration = BASE_VIEW_DURATION / zoom;
  const selected = segments.find((segment) => segment.id === selectedId);
  const viewStart = Math.max(0, (selected?.startMs ?? segments[0]?.startMs ?? 0) - duration * 0.28);
  const end = viewStart + duration;
  const ticks = useMemo(() => Array.from({ length: 8 }, (_, index) => viewStart + index * duration / 7), [duration, viewStart]);
  const position = (ms: number) => ((ms - viewStart) / duration) * 100;
  const visible = segments.filter((segment) => segment.endMs >= viewStart && segment.startMs <= end);

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
      <div className="tool-group"><Tooltip title="选择工具"><Button className="tool-button active" icon={<SelectIcon />} aria-label="选择工具" /></Tooltip><Tooltip title="拆分片段"><Button className="tool-button" icon={<SplitIcon />} aria-label="拆分片段" onClick={splitSelected} /></Tooltip><Tooltip title="合并下一片段"><Button className="tool-button" icon={<MergeIcon />} aria-label="合并下一片段" onClick={mergeNext} /></Tooltip><Tooltip title="吸附"><Button className={`tool-button ${snapping ? "active-soft" : ""}`} icon={<MagnetIcon />} aria-label="吸附" aria-pressed={snapping} onClick={() => setSnapping(!snapping)} /></Tooltip></div>
      <TimelineTimecode />
      <div className="zoom-control"><span>缩放</span><Minus size={14} /><Slider aria-label="时间轴缩放" min={0.7} max={2.4} step={0.1} value={zoom} onChange={setZoom} tooltip={{ open: false }} /><Plus size={14} /></div>
    </div>
    <div className="timeline-body">
      <div className="track-labels">
        <div className="ruler-label" />
        {[{ title: "原始音频", sub: "英语 · 48kHz" }, { title: "背景声音", sub: "BGM · -18.0 LUFS" }, { title: "中文配音", sub: "女声 · 自然风格" }, { title: "双语字幕", sub: `${segments.length} 个片段` }].map((track) => <div className="track-label" key={track.title}><Eye /><span><strong>{track.title}</strong><small>{track.sub}</small></span><SpeakerHigh className="track-action" /><LockSimple className="track-action" /></div>)}
      </div>
      <div className="track-content" ref={trackRef} onPointerDown={(event) => {
        const rect = event.currentTarget.getBoundingClientRect();
        setPlayhead(viewStart + ((event.clientX - rect.left) / rect.width) * duration);
      }}>
        <div className="time-ruler">{ticks.map((tick) => <span key={tick} style={{ left: `${position(tick)}%` }}><i />{formatTimecode(tick, true)}</span>)}</div>
        <div className="audio-track original"><WaveformCanvas seed={11} /></div>
        <div className="audio-track background"><WaveformCanvas seed={29} color="#3b8b62" /></div>
        <div className="segment-track dub-track">{visible.map((segment) => <button key={segment.id} className={`timeline-segment dub ${selectedId === segment.id ? "selected" : ""} ${segment.status}`} style={{ left: `${position(segment.startMs)}%`, width: `${Math.max(1.3, position(segment.endMs) - position(segment.startMs))}%` }} onClick={(event) => { event.stopPropagation(); selectSegment(segment.id); }}><span>{segment.spokenZh}</span>{segment.overflowMs && <em>! 时长超出 {(segment.overflowMs / 1000).toFixed(1)} 秒</em>}<i className="resize-handle start" onPointerDown={(event) => moveBoundary(segment.id, "start", event)} /><i className="resize-handle end" onPointerDown={(event) => moveBoundary(segment.id, "end", event)} /></button>)}</div>
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
