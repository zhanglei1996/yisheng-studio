import { ArrowCounterClockwise, ClockCounterClockwise, Copy, Link, LinkBreak, LockSimple, MagicWand, Microphone, Play, SpeakerHigh } from "@phosphor-icons/react";
import { Button, Input, Select, Segmented, Slider, Tooltip } from "antd";
import { formatTimecode } from "../domain";
import { glossaryTerms } from "../fixtures";
import { useEditorStore } from "../store";
import { antdIcon } from "../ui/icons";

const LockIcon = antdIcon(LockSimple);
const CopyIcon = antdIcon(Copy);
const LinkIcon = antdIcon(Link);
const UnlinkIcon = antdIcon(LinkBreak);
const MagicIcon = antdIcon(MagicWand);
const RestoreIcon = antdIcon(ArrowCounterClockwise);
const PlayIcon = antdIcon(Play);

export function Inspector({ onRegenerate }: { onRegenerate: (segmentId: string) => Promise<void> }) {
  const { segments, selectedId, inspectorTab, setInspectorTab, updateSegment } = useEditorStore();
  const segment = segments.find((item) => item.id === selectedId) ?? segments[0];
  if (!segment) return null;
  const regenerate = () => onRegenerate(segment.id);
  return <aside className="inspector">
    <div className="inspector-tabs"><Segmented block options={[{ label: "文本", value: "text" }, { label: "声音", value: "voice" }, { label: "对齐", value: "align" }]} value={inspectorTab} onChange={(value) => setInspectorTab(value as "text" | "voice" | "align")} /></div>
    <div className="inspector-content">
      {inspectorTab === "text" && <>
        <div className="inspector-title"><div><span>片段文本</span><small>{formatTimecode(segment.startMs)} – {formatTimecode(segment.endMs)}</small></div><Tooltip title={segment.locked ? "解锁片段" : "锁定片段"}><Button type="text" icon={<LockIcon />} aria-label={segment.locked ? "解锁片段" : "锁定片段"} onClick={() => updateSegment(segment.id, { locked: !segment.locked })} /></Tooltip></div>
        <label className="text-block"><span>源语言（英语）<Tooltip title="复制原文"><Button type="text" size="small" icon={<CopyIcon />} aria-label="复制原文" /></Tooltip></span><Input.TextArea autoSize={{ minRows: 2, maxRows: 5 }} value={segment.sourceText} onChange={(event) => updateSegment(segment.id, { sourceText: event.target.value })} /></label>
        <label className="text-block"><span>字幕译文（中文）<em>{segment.linked ? "与配音联动" : "独立字幕"}</em></span><Input.TextArea showCount maxLength={500} autoSize={{ minRows: 2, maxRows: 5 }} value={segment.subtitleZh} onChange={(event) => updateSegment(segment.id, { subtitleZh: event.target.value, spokenZh: segment.linked ? event.target.value : segment.spokenZh })} /></label>
        <div className="field-heading"><span>配音文案</span><Button type="link" size="small" icon={segment.linked ? <LinkIcon /> : <UnlinkIcon />} onClick={() => updateSegment(segment.id, { linked: !segment.linked, spokenZh: !segment.linked ? segment.subtitleZh : segment.spokenZh })}>{segment.linked ? "已联动" : "独立文案"}</Button></div>
        <label className="text-block"><Input.TextArea autoSize={{ minRows: 2, maxRows: 5 }} value={segment.spokenZh} onChange={(event) => updateSegment(segment.id, { spokenZh: event.target.value })} /><div className="text-actions"><Button type="link" size="small" icon={<MagicIcon />}>AI 精简</Button><Button type="link" size="small" icon={<RestoreIcon />}>恢复译文</Button></div></label>
        <div className="section-heading"><span>本片段术语</span><span className="count-badge">3</span></div>
        <div className="term-mini-table">{glossaryTerms.slice(0, 3).map((term) => <div key={term.id}><strong>{term.source}</strong><span>{term.target}</span><em>{term.policy === "keep" ? "保留" : "固定"}</em></div>)}</div>
        <label className="note-field"><span>上下文备注</span><Input placeholder="添加仅自己可见的备注" /></label>
      </>}
      {inspectorTab === "voice" && <>
        <div className="inspector-title"><div><span>中文声音</span><small>项目默认声音，可为当前片段覆盖</small></div><Microphone /></div>
        <label className="form-field"><span>服务商</span><Select defaultValue="macOS 系统语音" options={["macOS 系统语音", "讯飞开放平台", "腾讯云 TTS"].map((value) => ({ value, label: value }))} /></label>
        <label className="form-field"><span>声音</span><Select value={segment.voice} onChange={(voice) => updateSegment(segment.id, { voice })} options={["普通话 · 女声 · 自然", "普通话 · 男声 · 沉稳", "Tingting · 系统语音"].map((value) => ({ value, label: value }))} /></label>
        <label className="slider-field"><span><strong>语速</strong><em>{segment.speed.toFixed(2)}x</em></span><Slider min={0.8} max={1.2} step={0.01} value={segment.speed} onChange={(speed) => updateSegment(segment.id, { speed })} /></label>
        <Button className="voice-preview" icon={<PlayIcon />}><span><strong>试听当前片段</strong><small>{segment.spokenZh}</small></span></Button>
        <div className="data-scope compact"><SpeakerHigh /><div><strong>本地系统语音</strong><p>配音文案不会离开这台 Mac。</p></div></div>
      </>}
      {inspectorTab === "align" && <>
        <div className="inspector-title"><div><span>时长与对齐</span><small>音频不会被截断或侵入下一片段</small></div><ClockCounterClockwise /></div>
        <div className="alignment-meter"><header><span>目标时长</span><strong>{((segment.endMs - segment.startMs) / 1000).toFixed(2)} 秒</strong></header><div className="alignment-bars"><span style={{ width: "78%" }}>目标窗口</span><i style={{ width: segment.overflowMs ? "92%" : "74%" }} /></div><footer><span>合成音频 {segment.overflowMs ? "5.84" : "4.82"} 秒</span><em className={segment.overflowMs ? "warning-text" : "success-text"}>{segment.overflowMs ? `超出 ${(segment.overflowMs / 1000).toFixed(1)} 秒` : "适配良好"}</em></footer></div>
        <div className="alignment-rules"><div><span>首尾静音</span><strong>自动裁剪</strong></div><div><span>不变调加速</span><strong>最高 1.15x</strong></div><div><span>文案压缩</span><strong>最多重试 2 次</strong></div><div><span>最大自动语速</span><strong>1.20x</strong></div></div>
        {segment.overflowMs && <div className="warning-card"><strong>这个片段需要处理</strong><p>建议精简 4～7 个汉字，或将结束边界向后移动。</p><Button type="link" size="small" icon={<MagicIcon />} onClick={regenerate}>重新配音并校验</Button></div>}
      </>}
    </div>
    <footer className="inspector-footer"><Button icon={<MagicIcon />} onClick={regenerate}>重新生成本片段</Button></footer>
  </aside>;
}
