import { useEffect, useMemo, useRef } from "react";
import { useQuery } from "@tanstack/react-query";
import { convertFileSrc } from "@tauri-apps/api/core";
import { Button, Tooltip, message } from "antd";
import { ArrowCounterClockwise, ArrowClockwise, CornersOut, Export, Pause, Play, SkipBack, SkipForward, SpeakerHigh, SpeakerSlash, Waveform } from "@phosphor-icons/react";
import { formatTimecode } from "../domain";
import { useEditorStore } from "../store";
import { Inspector } from "./Inspector";
import { Timeline } from "./Timeline";
import { antdIcon } from "../ui/icons";
import { desktopBridge } from "../bridge";

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

export function EditorPage({ onExport, onRegenerate, projectId }: { onExport: () => void; onRegenerate: (segmentId: string) => Promise<void>; projectId: string | null }) {
  const { segments, selectedId, selectSegment, playheadMs, setPlayhead, playing, togglePlaying, muted, toggleMuted, undo, redo, history, future, loadSegments } = useEditorStore();
  const { data: projectSegments = [] } = useQuery({ queryKey: ["segments", projectId], queryFn: () => desktopBridge.listSegments(projectId!), enabled: Boolean(projectId) && desktopBridge.isDesktop() });
  const { data: projects = [] } = useQuery({ queryKey: ["projects"], queryFn: desktopBridge.listProjects });
  const project = projects.find((item) => item.id === projectId);
  const previewEnabled = Boolean(projectId && project?.artifactDir) && desktopBridge.isDesktop();
  const { data: previewMedia, isPending: previewPending, isError: previewFailed, refetch: retryPreview } = useQuery({
    queryKey: ["preview-media", projectId, project?.updatedAt],
    queryFn: () => desktopBridge.resolvePreviewMedia(projectId!),
    enabled: previewEnabled,
    retry: 1,
  });
  const videoRef = useRef<HTMLVideoElement>(null);
  const previewUrl = useMemo(() => previewMedia?.path && desktopBridge.isDesktop() ? `${convertFileSrc(previewMedia.path)}?revision=${encodeURIComponent(project?.updatedAt ?? "current")}` : null, [previewMedia?.path, project?.updatedAt]);
  const loadedProject = useRef<string | null>(null);
  const syncingFromBackend = useRef(false);
  const persistedSignature = useRef("");
  const signatureOf = (items: Array<{ id: string; startMs: number; endMs: number; sourceText: string; subtitleZh: string; spokenZh: string; linked: boolean; status: string }>) => JSON.stringify(items.map((segment) => [segment.id, segment.startMs, segment.endMs, segment.sourceText, segment.subtitleZh, segment.spokenZh, segment.linked, segment.status]));
  useEffect(() => {
    if (projectId && projectSegments.length) {
      const nextSignature = signatureOf(projectSegments);
      if (loadedProject.current === projectId && nextSignature === persistedSignature.current) return;
      syncingFromBackend.current = true;
      persistedSignature.current = nextSignature;
      loadSegments(projectSegments.map((segment) => ({ ...segment, status: ["ready", "processing", "warning", "stale"].includes(segment.status) ? segment.status as "ready" | "processing" | "warning" | "stale" : "ready", voice: "Tingting · 系统语音", speed: 1 })));
      loadedProject.current = projectId;
    }
  }, [projectId, projectSegments, loadSegments]);
  useEffect(() => {
    if (!projectId || loadedProject.current !== projectId || !desktopBridge.isDesktop() || !segments.length) return;
    const payload = segments.map((segment, ordinal) => ({
      id: segment.id, projectId, ordinal, startMs: Math.round(segment.startMs), endMs: Math.round(segment.endMs), sourceText: segment.sourceText,
      subtitleZh: segment.subtitleZh, spokenZh: segment.spokenZh, linked: segment.linked, status: segment.status,
    }));
    const currentSignature = signatureOf(payload);
    if (syncingFromBackend.current || currentSignature === persistedSignature.current) {
      syncingFromBackend.current = false;
      return;
    }
    const timer = window.setTimeout(() => desktopBridge.saveSegments(projectId, payload).then(() => {
      persistedSignature.current = currentSignature;
    }).catch((error) => message.error(String(error))), 500);
    return () => window.clearTimeout(timer);
  }, [projectId, segments]);
  const current = segments.find((segment) => segment.id === selectedId) ?? segments[0];
  useEffect(() => {
    const video = videoRef.current;
    if (video && Math.abs(video.currentTime * 1000 - playheadMs) > 600) video.currentTime = playheadMs / 1000;
  }, [playheadMs]);
  useEffect(() => {
    const video = videoRef.current;
    const segment = segments.find((item) => item.id === selectedId);
    if (video && segment) video.currentTime = segment.startMs / 1000;
  }, [selectedId]);
  const togglePlayback = async () => {
    const video = videoRef.current;
    if (!video) { togglePlaying(); return; }
    if (video.paused) await video.play(); else video.pause();
  };

  return <div className="editor-page">
    <div className="editor-actionbar">
      <div className="project-context"><strong>{project?.name ?? "Building Reliable AI Agents"}</strong><span>{project?.workflowMode === "review" ? "先校对模式" : "快速生成模式"}</span></div>
      <div className="undo-group"><Button type="text" disabled={!history.length} icon={<UndoIcon />} onClick={undo}>撤销</Button><Button type="text" disabled={!future.length} icon={<RedoIcon />} onClick={redo}>重做</Button></div>
      <div className="editor-spacer" />
      <span className="project-state"><i />本地处理 · 已暂停等待校对</span>
      <Button type="primary" size="small" icon={<ExportIcon />} onClick={onExport}>导出</Button>
    </div>
    <div className="editor-upper">
      <section className="preview-panel">
        <div className="video-frame">
          {previewUrl ? <video key={previewUrl} ref={videoRef} src={previewUrl} muted={muted} onTimeUpdate={(event) => setPlayhead(event.currentTarget.currentTime * 1000)} onPlay={() => !playing && togglePlaying()} onPause={() => playing && togglePlaying()} /> : <div className="video-placeholder">{previewFailed ? <><strong>预览准备失败</strong><Button size="small" onClick={() => retryPreview()}>重试</Button></> : previewEnabled && previewPending ? "正在准备中文合成预览…" : "完成媒体准备后可预览本地视频"}</div>}
          {previewMedia && <div className={`preview-source-badge ${previewMedia.dubbed ? "dubbed" : "source"}`}><Waveform size={14} weight="bold" />{previewMedia.dubbed ? "中文合成预览" : "原始代理 · 等待中文配音"}</div>}
          <div className="subtitle-overlay"><span>{current?.sourceText}</span><strong>{current?.subtitleZh}</strong></div>
        </div>
        <div className="playback-bar"><span className="current-time">{formatTimecode(playheadMs)} <em>/ {formatTimecode(project?.durationMs ?? 0)}</em></span><div className="transport"><Tooltip title="上一片段"><Button type="text" shape="circle" icon={<BackIcon />} aria-label="上一片段" onClick={() => { const index = segments.findIndex((segment) => segment.id === selectedId); if (index > 0) selectSegment(segments[index - 1].id); }} /></Tooltip><Tooltip title={playing ? "暂停" : "播放"}><Button className="play-button" type="text" shape="circle" icon={playing ? <PauseIcon /> : <PlayIcon />} aria-label={playing ? "暂停" : "播放"} onClick={togglePlayback} /></Tooltip><Tooltip title="下一片段"><Button type="text" shape="circle" icon={<ForwardIcon />} aria-label="下一片段" onClick={() => { const index = segments.findIndex((segment) => segment.id === selectedId); if (index < segments.length - 1) selectSegment(segments[index + 1].id); }} /></Tooltip></div><div className="playback-options"><Button type="text" size="small">1.0x</Button><Tooltip title={muted ? "取消静音" : "静音"}><Button type="text" shape="circle" icon={muted ? <MuteIcon /> : <SpeakerIcon />} aria-label={muted ? "取消静音" : "静音"} onClick={toggleMuted} /></Tooltip><Tooltip title="全屏"><Button type="text" shape="circle" icon={<FullscreenIcon />} aria-label="全屏" onClick={() => videoRef.current?.requestFullscreen()} /></Tooltip></div></div>
      </section>
      <Inspector onRegenerate={async (segmentId) => {
        if (projectId && desktopBridge.isDesktop()) {
          const payload = segments.map((segment, ordinal) => ({
            id: segment.id, projectId, ordinal, startMs: Math.round(segment.startMs), endMs: Math.round(segment.endMs), sourceText: segment.sourceText,
            subtitleZh: segment.subtitleZh, spokenZh: segment.spokenZh, linked: segment.linked, status: segment.status,
          }));
          await desktopBridge.saveSegments(projectId, payload);
          persistedSignature.current = signatureOf(payload);
        }
        await onRegenerate(segmentId);
      }} />
    </div>
    <Timeline />
    <footer className="editor-statusbar"><span>项目帧率&nbsp; 30 fps</span><span>音频采样率&nbsp; 48 kHz</span><span>片段总数&nbsp; {segments.length}</span><span>已处理&nbsp; {segments.filter((segment) => segment.status === "ready").length}</span><span>待处理&nbsp; {segments.filter((segment) => segment.status !== "ready").length}</span><em>存储空间&nbsp; 本地 1.23 TB 可用 <i /></em></footer>
  </div>;
}
