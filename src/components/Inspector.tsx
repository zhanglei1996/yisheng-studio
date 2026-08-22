import { memo } from "react";
import { useShallow } from "zustand/shallow";
import { ArrowCounterClockwise, CaretLeft, CaretRight, ClockCounterClockwise, Copy, Link, LinkBreak, LockSimple, LockSimpleOpen, MagicWand } from "@phosphor-icons/react";
import { Button, Input, Segmented, Tooltip } from "antd";
import { formatTimecode, type TtsVoice } from "../domain";
import { useEditorStore } from "../store";
import { antdIcon } from "../ui/icons";
import { VoiceInspector, type VoiceSynthesisEstimate } from "./voice/VoiceInspector";

const LockIcon = antdIcon(LockSimple);
const UnlockIcon = antdIcon(LockSimpleOpen);
const PreviousIcon = antdIcon(CaretLeft);
const NextIcon = antdIcon(CaretRight);
const CopyIcon = antdIcon(Copy);
const LinkIcon = antdIcon(Link);
const UnlinkIcon = antdIcon(LinkBreak);
const MagicIcon = antdIcon(MagicWand);
const RestoreIcon = antdIcon(ArrowCounterClockwise);

export interface InspectorProps {
  onRegenerate: (segmentId: string) => Promise<void>;
  onSmartFit?: (segmentId: string) => Promise<unknown>;
  regenerating?: boolean;
  onPreviewVoice?: (segmentId: string) => Promise<void>;
  onRunDirector?: (segmentId: string) => Promise<void>;
  voices?: TtsVoice[];
  voiceEstimate?: VoiceSynthesisEstimate | null;
  onProjectVoiceChange?: (voice: TtsVoice) => Promise<void> | void;
  syncMode?: "strict" | "balanced" | "narration" | "semantic";
  syncBlockSize?: number;
  syncBlockDurationMs?: number;
  onSyncModeChange?: (mode: "strict" | "balanced" | "narration" | "semantic") => Promise<void> | void;
}

export const Inspector = memo(function Inspector({ onRegenerate, onSmartFit, regenerating, onPreviewVoice, onRunDirector, onProjectVoiceChange, voices, voiceEstimate, syncMode = "strict", syncBlockSize = 1, syncBlockDurationMs, onSyncModeChange }: InspectorProps) {
  const { segments, selectedId, inspectorTab, selectSegment, setInspectorTab, updateSegment } = useEditorStore(useShallow((state) => ({
    segments: state.segments, selectedId: state.selectedId, inspectorTab: state.inspectorTab, selectSegment: state.selectSegment, setInspectorTab: state.setInspectorTab, updateSegment: state.updateSegment,
  })));
  const segment = segments.find((item) => item.id === selectedId) ?? segments[0];
  if (!segment) return null;
  const selectedIndex = Math.max(0, segments.findIndex((item) => item.id === segment.id));
  const segmentState = segment.ttsState === "failed"
    ? { label: "配音失败", tone: "danger" }
    : segment.status === "warning"
      ? { label: "时长提醒", tone: "warning" }
      : segment.status === "stale"
        ? { label: "等待重新生成", tone: "pending" }
        : { label: "已就绪", tone: "success" };
  const durationPending = segment.status === "warning" && !segment.ttsDurationMs;
  const regenerate = () => onRegenerate(segment.id);
  const copySource = async () => {
    try {
      await navigator.clipboard.writeText(segment.sourceText);
    } catch {
      const input = document.createElement("textarea");
      input.value = segment.sourceText;
      input.style.position = "fixed";
      input.style.opacity = "0";
      document.body.appendChild(input);
      input.select();
      document.execCommand("copy");
      input.remove();
    }
  };
  const restoreTranslation = () => updateSegment(segment.id, {
    spokenZh: segment.subtitleZh,
    linked: true,
    scriptDocument: undefined,
  });
  return <aside className="inspector">
    <div className="inspector-tabs"><Segmented block options={[{ label: "文本", value: "text" }, { label: "声音", value: "voice" }, { label: "对齐", value: "align" }]} value={inspectorTab} onChange={(value) => setInspectorTab(value as "text" | "voice" | "align")} /></div>
    <div className="inspector-contextbar">
      <div><strong>片段 {selectedIndex + 1}</strong><span>/ {segments.length}</span><em className={`segment-state ${segmentState.tone}`}>{segmentState.label}</em></div>
      <div className="segment-navigator">
        <Tooltip title="上一片段（↑）"><Button type="text" size="small" icon={<PreviousIcon />} aria-label="上一片段" disabled={selectedIndex === 0} onClick={() => selectSegment(segments[selectedIndex - 1].id)} /></Tooltip>
        <Tooltip title="下一片段（↓）"><Button type="text" size="small" icon={<NextIcon />} aria-label="下一片段" disabled={selectedIndex >= segments.length - 1} onClick={() => selectSegment(segments[selectedIndex + 1].id)} /></Tooltip>
      </div>
    </div>
    {inspectorTab !== "voice" && <div className="inspector-content">
      {inspectorTab === "text" && <>
        <div className="inspector-title"><div><span>片段文本</span><small>{formatTimecode(segment.startMs)} – {formatTimecode(segment.endMs)}</small></div><Tooltip title={segment.locked ? "解锁时间边界" : "锁定时间边界"}><Button type="text" icon={segment.locked ? <LockIcon /> : <UnlockIcon />} aria-label={segment.locked ? "解锁时间边界" : "锁定时间边界"} aria-pressed={segment.locked} onClick={() => updateSegment(segment.id, { locked: !segment.locked })} /></Tooltip></div>
        <label className="text-block"><span>源语言（英语）<Tooltip title="复制原文"><Button type="text" size="small" icon={<CopyIcon />} aria-label="复制原文" onClick={() => void copySource()} /></Tooltip></span><Input.TextArea autoSize={{ minRows: 2, maxRows: 5 }} value={segment.sourceText} onChange={(event) => updateSegment(segment.id, { sourceText: event.target.value })} /></label>
        <label className="text-block"><span>字幕译文（中文）<em>{segment.linked ? "与配音联动" : "独立字幕"}</em></span><Input.TextArea showCount maxLength={500} autoSize={{ minRows: 2, maxRows: 5 }} value={segment.subtitleZh} onChange={(event) => updateSegment(segment.id, { subtitleZh: event.target.value, spokenZh: segment.linked ? event.target.value : segment.spokenZh })} /></label>
        <div className="field-heading"><span>配音文案</span><Button type="link" size="small" icon={segment.linked ? <LinkIcon /> : <UnlinkIcon />} onClick={() => updateSegment(segment.id, { linked: !segment.linked, spokenZh: !segment.linked ? segment.subtitleZh : segment.spokenZh })}>{segment.linked ? "已联动" : "独立文案"}</Button></div>
        <label className="text-block"><Input.TextArea autoSize={{ minRows: 2, maxRows: 5 }} value={segment.spokenZh} onChange={(event) => updateSegment(segment.id, { spokenZh: event.target.value, linked: false })} /><div className="text-actions">{segment.overflowMs && onSmartFit && <Button type="link" size="small" icon={<MagicIcon />} loading={regenerating} onClick={() => onSmartFit(segment.id)}>智能缩短</Button>}<Button type="link" size="small" icon={<RestoreIcon />} disabled={segment.linked && segment.spokenZh === segment.subtitleZh} onClick={restoreTranslation}>恢复译文</Button></div></label>
      </>}
      {inspectorTab === "align" && <>
        <div className="inspector-title"><div><span>时长与对齐</span><small>音频不会被截断或侵入下一片段</small></div><ClockCounterClockwise /></div>
        <div className="alignment-meter"><header><span>目标时长</span><strong>{((segment.endMs - segment.startMs) / 1000).toFixed(2)} 秒</strong></header><div className="alignment-bars"><span style={{ width: "78%" }}>目标窗口</span><i style={{ width: segment.overflowMs ? "92%" : "74%" }} /></div><footer><span>{durationPending ? "最新合成仍未适配成功" : `合成音频 ${((segment.ttsDurationMs ?? (segment.endMs - segment.startMs + (segment.overflowMs ?? 0))) / 1000).toFixed(2)} 秒`}</span><em className={segment.overflowMs ? "warning-text" : "success-text"}>{durationPending ? "需要继续处理" : segment.overflowMs ? `超出 ${(segment.overflowMs / 1000).toFixed(1)} 秒` : "适配良好"}</em></footer></div>
        <div className="alignment-rules"><div><span>首尾静音</span><strong>自动裁剪</strong></div><div><span>常规不变调加速</span><strong>最高 1.08x</strong></div><div><span>文案压缩</span><strong>最多重试 2 次</strong></div><div><span>短片段兜底</span><strong>最高 1.25x</strong></div></div>
        {segment.overflowMs && <div className="warning-card"><strong>{durationPending ? "这个片段仍未通过时长校验" : `这个片段超出目标 ${(segment.overflowMs / 1000).toFixed(1)} 秒`}</strong><p>推荐先智能缩短口播稿；字幕译文保持不变。如果语义不适合压缩，可编辑口播稿或拖动时间轴边界。</p><div className="warning-actions"><Button type="primary" icon={<MagicIcon />} loading={regenerating} onClick={() => onSmartFit?.(segment.id)}>智能缩短并重生成</Button><Button onClick={() => setInspectorTab("text")}>编辑口播稿</Button><Button onClick={() => document.querySelector<HTMLElement>(`.timeline-segment[data-segment-id='${segment.id}']`)?.focus()}>调整片段边界</Button></div></div>}
      </>}
    </div>}
    {inspectorTab === "voice" && <VoiceInspector segment={segment} voices={voices} estimate={voiceEstimate} updateSegment={updateSegment} onPreviewVoice={onPreviewVoice} onRunDirector={onRunDirector} onProjectVoiceChange={onProjectVoiceChange} onRegenerate={onRegenerate} syncMode={syncMode} syncBlockSize={syncBlockSize} syncBlockDurationMs={syncBlockDurationMs} onSyncModeChange={onSyncModeChange} />}
    {inspectorTab !== "voice" && <footer className="inspector-footer"><span>{segment.overflowMs ? "本片段仍需处理" : "本片段时长正常"}</span><Button icon={<MagicIcon />} loading={regenerating} onClick={regenerate}>重新生成本片段</Button></footer>}
  </aside>;
});
