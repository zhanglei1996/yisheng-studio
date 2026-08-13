import { memo, useEffect, useMemo, useState } from "react";
import {
  CaretDown,
  CheckCircle,
  Info,
  MagicWand,
  Microphone,
  Pause,
  Play,
  ShieldCheck,
  Sparkle,
  WarningCircle,
} from "@phosphor-icons/react";
import { Button, Input, Segmented, Select, Slider, Switch, Tooltip } from "antd";
import type { InlineNode, ScriptDocument, Segment, TtsStyle, TtsStyleId, TtsVoice } from "../../domain";
import { antdIcon } from "../../ui/icons";

const PlayIcon = antdIcon(Play);
const MagicIcon = antdIcon(MagicWand);
const InfoIcon = antdIcon(Info, 14);

const fallbackVoices: TtsVoice[] = [
  { id: "iflytek-xiaoxi", providerId: "iflytek", providerName: "讯飞超拟人", name: "小溪", locale: "zh-CN", gender: "female", traits: ["自然", "清晰"], available: false },
  { id: "aliyun-zhixia", providerId: "aliyun", providerName: "阿里百炼", name: "知琪", locale: "zh-CN", gender: "female", traits: ["亲和", "讲解"], available: false },
  { id: "system-tingting", providerId: "system", providerName: "macOS 系统语音", name: "Tingting", locale: "zh-CN", gender: "female", traits: ["本地", "免费"], available: true },
];

const styles: TtsStyle[] = [
  { id: "auto", label: "自动", description: "跟随语义与原声韵律" },
  { id: "professional", label: "专业讲解", description: "克制、清晰" },
  { id: "conversational", label: "自然口语", description: "轻松、有呼吸感" },
  { id: "documentary", label: "沉稳纪录", description: "低起伏、叙事感" },
  { id: "upbeat", label: "轻快分享", description: "更明亮活泼" },
  { id: "emphasis", label: "重点强调", description: "突出关键信息" },
];

export interface VoiceSynthesisEstimate {
  durationSeconds: number;
  costCny: number;
  requiresConfirmation?: boolean;
}

export interface VoiceInspectorProps {
  segment: Segment;
  voices?: TtsVoice[];
  estimate?: VoiceSynthesisEstimate | null;
  updateSegment: (id: string, patch: Partial<Segment>, recordHistory?: boolean) => void;
  onPreviewVoice?: (segmentId: string) => Promise<void>;
  onRunDirector?: (segmentId: string) => Promise<void>;
  onProjectVoiceChange?: (voice: TtsVoice) => Promise<void> | void;
  onRegenerate: (segmentId: string) => Promise<void>;
  syncMode?: "strict" | "balanced" | "narration" | "semantic";
  syncBlockSize?: number;
  syncBlockDurationMs?: number;
  onSyncModeChange?: (mode: "strict" | "balanced" | "narration" | "semantic") => Promise<void> | void;
}

const protectedTerm = /^(AI|API|RAG|LLM|GPT|token)$/i;
const emphasisPhrase = /(先?检索相关上下文|可靠性至关重要|至关重要|重点|关键)/;

function scriptFromText(text: string): ScriptDocument {
  const chunks = text.split(/(AI|API|RAG|LLM|GPT|token|先?检索相关上下文|可靠性至关重要|至关重要|[，；。！？])/gi).filter(Boolean);
  const nodes: InlineNode[] = [];
  chunks.forEach((chunk) => {
    if (chunk === "，" || chunk === "；") {
      nodes.push({ type: "text", text: chunk, origin: "translation" });
      nodes.push({ type: "pause", durationMs: 280, origin: "auto" });
      return;
    }
    const trimmed = chunk.trim();
    if (protectedTerm.test(trimmed)) {
      nodes.push({ type: "protected", text: chunk, kind: "term", canonical: trimmed, origin: "translation" });
      return;
    }
    nodes.push({
      type: "text",
      text: chunk,
      emphasis: emphasisPhrase.test(trimmed) ? 2 : undefined,
      delivery: /^(其实|通常|那么|所以)/.test(trimmed) ? "natural" : undefined,
      origin: emphasisPhrase.test(trimmed) || /^(其实|通常|那么|所以)/.test(trimmed) ? "auto" : "translation",
    });
  });
  return { version: 1, blocks: [{ type: "paragraph", children: nodes }] };
}

function displayName(voice: TtsVoice) {
  return `${voice.providerName} · ${voice.name}`;
}

type ScriptMark = "text" | "emphasis" | "conversational" | "protected";

const scriptMarkLabels: Record<ScriptMark, string> = {
  text: "普通",
  emphasis: "强调",
  conversational: "口语化",
  protected: "保护",
};

function scriptMark(node: Exclude<InlineNode, { type: "pause" }>): ScriptMark {
  if (node.type === "protected") return "protected";
  if (node.delivery === "natural" || node.delivery === "casual") return "conversational";
  if (node.emphasis) return "emphasis";
  return "text";
}

function nextScriptMark(mark: ScriptMark): ScriptMark {
  if (mark === "text") return "emphasis";
  if (mark === "emphasis") return "conversational";
  if (mark === "conversational") return "protected";
  return "text";
}

function applyNextScriptMark(node: Exclude<InlineNode, { type: "pause" }>): InlineNode {
  const nextMark = nextScriptMark(scriptMark(node));
  if (nextMark === "protected") {
    return {
      type: "protected",
      text: node.text,
      kind: /\d/.test(node.text) ? "number" : "term",
      canonical: node.text.trim() || node.text,
      origin: "manual",
    };
  }
  if (nextMark === "emphasis") return { type: "text", text: node.text, emphasis: 2, origin: "manual" };
  if (nextMark === "conversational") return { type: "text", text: node.text, delivery: "natural", origin: "manual" };
  return { type: "text", text: node.text, origin: "manual" };
}

function styleCapabilityNote(voice?: TtsVoice) {
  if (!voice) return "当前未选择音色；风格将保存为译声工坊导演编排。";
  const provider = `${voice.providerId} ${voice.providerName}`.toLowerCase();
  if (provider.includes("system") || provider.includes("macos")) {
    return "系统语音不支持原生风格指令；选择后请点击“重新编排”，由译声工坊调整断句、强调和停顿。";
  }
  if (provider.includes("iflytek") || provider.includes("讯飞")) {
    return "讯飞超拟人当前未开放原生风格指令；译声工坊会把导演结果转成可听见的断句、强调与 [pNNN] 停顿。";
  }
  if (provider.includes("aliyun") || provider.includes("bailian") || provider.includes("阿里") || provider.includes("百炼")) {
    if (/_v\d\b/i.test(voice.id) || /^long/i.test(voice.id)) {
      return "CosyVoice 的原生指令按模型与音色能力启用；其他音色仍会执行导演的断句、强调与停顿。";
    }
    return "百炼的原生风格能力由模型决定；当前音色数据未声明模型，先按译声工坊导演编排，仅 Qwen Instruct 会同时原生执行。";
  }
  return "当前音色未声明原生风格能力；选择后请点击“重新编排”，由译声工坊导演实现。";
}

export const VoiceInspector = memo(function VoiceInspector({ segment, voices = fallbackVoices, estimate, updateSegment, onPreviewVoice, onRunDirector, onProjectVoiceChange, onRegenerate, syncMode = "strict", syncBlockSize = 1, syncBlockDurationMs, onSyncModeChange }: VoiceInspectorProps) {
  const [documentTab, setDocumentTab] = useState<"subtitle" | "script">("script");
  const [editing, setEditing] = useState(false);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [previewing, setPreviewing] = useState(false);
  const [generating, setGenerating] = useState(false);
  const [directing, setDirecting] = useState(false);
  const [changingVoice, setChangingVoice] = useState(false);
  const [changingSyncMode, setChangingSyncMode] = useState(false);
  const [textDraft, setTextDraft] = useState(segment.spokenZh);
  const script = useMemo(() => segment.scriptDocument ?? scriptFromText(segment.spokenZh), [segment.scriptDocument, segment.spokenZh]);
  const selectedVoice = voices.find((voice) => voice.id === segment.voice || voice.name === segment.voice || displayName(voice) === segment.voice)
    ?? voices.find((voice) => voice.available)
    ?? voices[0];
  const directorEnabled = segment.directorEnabled ?? true;
  const selectedStyle = segment.ttsStyle ?? "auto";
  const cloudVoice = selectedVoice?.providerId !== "system";
  const nodes = script.blocks[0]?.children ?? [];
  const capabilityNote = styleCapabilityNote(selectedVoice);
  const previewDisabledReason = !selectedVoice?.available
    ? "请先在设置中配置该服务商的 API Key"
    : !onPreviewVoice
      ? "试听能力正在接入，配置完成后即可使用"
      : undefined;

  const updateScript = (next: ScriptDocument, spokenZh = segment.spokenZh) => updateSegment(segment.id, { scriptDocument: next, spokenZh });
  const changeNode = (nodeIndex: number) => {
    const nextNodes: InlineNode[] = nodes.map((item, index): InlineNode => {
      if (index !== nodeIndex) return item;
      if (item.type === "pause") return { ...item, durationMs: item.durationMs >= 400 ? 160 : item.durationMs + 120, origin: "manual" };
      return applyNextScriptMark(item);
    });
    updateScript({ version: 1, blocks: [{ type: "paragraph", children: nextNodes }] });
  };
  const addPause = () => updateScript({
    version: 1,
    blocks: [{ type: "paragraph", children: [...nodes, { type: "pause", durationMs: 280, origin: "manual" }] }],
  });
  const changeText = (spokenZh: string) => updateScript(scriptFromText(spokenZh), spokenZh);
  const beginTextEditing = () => {
    setTextDraft(segment.spokenZh);
    setEditing(true);
  };
  const commitTextEdit = (draft = textDraft) => {
    if (draft !== segment.spokenZh) changeText(draft);
    setEditing(false);
  };
  const preview = async () => {
    if (!onPreviewVoice || previewDisabledReason) return;
    setPreviewing(true);
    try { await onPreviewVoice(segment.id); } finally { setPreviewing(false); }
  };
  const regenerate = async () => {
    setGenerating(true);
    try { await onRegenerate(segment.id); } finally { setGenerating(false); }
  };
  const runDirector = async () => {
    if (!onRunDirector) return;
    setDirecting(true);
    try { await onRunDirector(segment.id); } finally { setDirecting(false); }
  };

  useEffect(() => {
    setEditing(false);
    setTextDraft(segment.spokenZh);
  }, [segment.id]);

  useEffect(() => {
    if (!editing) setTextDraft(segment.spokenZh);
  }, [editing, segment.spokenZh]);

  return <>
    <div className="inspector-content voice-inspector-content">
      {segment.ttsStatus === "failed" && <section className="voice-failure-card" role="alert">
        <WarningCircle size={16} />
        <div><strong>本片段配音失败</strong><p>{segment.ttsErrorMessage ?? "请检查服务商凭据、音色权限或网络后重试。"}</p></div>
      </section>}
      <section className="voice-project-head">
        <span className="voice-avatar"><Microphone size={18} weight="fill" /></span>
        <div><strong>{selectedVoice ? displayName(selectedVoice) : "未选择声音"}</strong><small>{selectedVoice?.traits.join(" · ") || "用于首次整片生成和后续批量重生成"}</small></div>
        <span className="project-voice-badge"><CheckCircle size={12} weight="fill" />项目默认声音</span>
        <Select
          aria-label="切换项目默认声音"
          popupMatchSelectWidth={260}
          value={selectedVoice?.id}
          loading={changingVoice}
          disabled={changingVoice}
          onChange={async (voiceId) => {
            const voice = voices.find((candidate) => candidate.id === voiceId);
            if (voice && onProjectVoiceChange) {
              setChangingVoice(true);
              try { await onProjectVoiceChange(voice); } finally { setChangingVoice(false); }
            } else updateSegment(segment.id, { voice: voiceId });
          }}
          options={voices.map((voice) => ({ value: voice.id, label: `${displayName(voice)}${voice.available ? "" : " · 待配置"}` }))}
        />
      </section>

      <section className="voice-sync-section">
        <header><strong>配音连续性</strong><span className={syncMode === "semantic" ? "recommended" : ""}>{syncMode === "semantic" ? "实验推荐" : syncMode === "balanced" ? "稳定" : "精确卡点"}</span></header>
        <Segmented
          block
          value={syncMode}
          disabled={changingSyncMode || selectedVoice?.providerId === "system"}
          options={[{ label: "语义旁白", value: "semantic" }, { label: "平衡模式", value: "balanced" }, { label: "严格同步", value: "strict" }]}
          onChange={async (value) => {
            if (!onSyncModeChange) return;
            setChangingSyncMode(true);
            try { await onSyncModeChange(value as "strict" | "balanced" | "narration" | "semantic"); } finally { setChangingSyncMode(false); }
          }}
        />
        <p>{selectedVoice?.providerId === "system" ? "系统语音暂只支持逐片段同步；选择云端语音后可启用语义旁白。" : syncMode === "semantic" ? `百炼先按 30–60 秒场景理解并改写，再由${selectedVoice?.providerName ?? "当前语音服务"}把当前 ${syncBlockSize} 条字幕组织成约 ${((syncBlockDurationMs ?? 0) / 1000).toFixed(1)} 秒的画面锚点。` : syncMode === "narration" ? `当前片段属于 ${syncBlockSize} 条字幕的旧版旁白章节。` : syncMode === "balanced" ? `连续合成当前语音块的 ${syncBlockSize} 条字幕（约 ${((syncBlockDurationMs ?? 0) / 1000).toFixed(1)} 秒），减少每句重新起范的片段感。` : "每条字幕独立合成，卡点更精确，但句间可能出现明显的状态重置。"}</p>
        {syncMode !== "strict" && <small>{syncMode === "semantic" ? "每个 5–15 秒锚点连续合成并独立校时；句意可以重组，音频不会跨过画面锚点长期漂移。" : syncMode === "narration" ? "旧版章节模式仅保留兼容，不建议继续使用。" : "编辑任意一句后，只需重新生成它所在的语音块；字幕时间码不会改变。"}</small>}
      </section>

      <section className="voice-script-card">
        <header>
          <Segmented
            size="small"
            options={[{ label: "字幕译文", value: "subtitle" }, { label: "口播稿", value: "script" }]}
            value={documentTab}
            onChange={(value) => setDocumentTab(value as "subtitle" | "script")}
          />
          <span className="protected-status"><ShieldCheck size={13} weight="fill" />术语与数字已保护</span>
        </header>
        {documentTab === "subtitle" ? <Input.TextArea
          className="voice-script-textarea"
          value={segment.subtitleZh}
          autoSize={{ minRows: 4, maxRows: 8 }}
          onChange={(event) => updateSegment(segment.id, { subtitleZh: event.target.value })}
        /> : editing ? <Input.TextArea
          className="voice-script-textarea"
          value={textDraft}
          autoFocus
          autoSize={{ minRows: 4, maxRows: 8 }}
          onBlur={(event) => commitTextEdit(event.currentTarget.value)}
          onChange={(event) => setTextDraft(event.target.value)}
        /> : <div className="annotated-script" role="group" aria-label="结构化口播稿">
          {nodes.map((node, index) => {
            if (node.type === "pause") {
              const nextDuration = node.durationMs >= 400 ? 160 : node.durationMs + 120;
              return <Tooltip key={`pause-${index}`} title={`当前 ${node.durationMs}ms；点击调整为 ${nextDuration}ms`}>
                <button type="button" className="inline-mark pause" aria-label={`停顿 ${node.durationMs}毫秒，点击调整为 ${nextDuration}毫秒`} onClick={() => changeNode(index)}><Pause size={11} weight="fill" />停顿 {node.durationMs}ms</button>
              </Tooltip>;
            }
            const mark = scriptMark(node);
            const nextMark = nextScriptMark(mark);
            return <Tooltip key={`node-${index}`} title={`当前：${scriptMarkLabels[mark]}；点击切换为${scriptMarkLabels[nextMark]}`}>
              <button type="button" className={`script-token ${mark}`} aria-label={`${node.text}，当前${scriptMarkLabels[mark]}，点击切换为${scriptMarkLabels[nextMark]}`} onClick={() => changeNode(index)}>
                {node.text}<span>{mark === "text" ? "" : scriptMarkLabels[mark]}</span>
              </button>
            </Tooltip>;
          })}
        </div>}
        <footer>
          <span>{nodes.some((node) => node.origin === "manual") ? "已由你调整演绎标记" : "自动导演已根据语义生成"}</span>
          <div><Button type="text" size="small" icon={<Pause size={13} />} onClick={addPause}>加停顿</Button><Button type="link" size="small" onClick={beginTextEditing}>编辑文字</Button></div>
        </footer>
      </section>

      <section className="performance-section">
        <header><strong>自动导演</strong><span><Button type="link" size="small" loading={directing} disabled={!onRunDirector || !directorEnabled} onClick={runDirector}>重新编排</Button><Switch aria-label="自动导演" size="small" checked={directorEnabled} onChange={(checked) => updateSegment(segment.id, { directorEnabled: checked })} /></span></header>
        <p className="performance-summary">{nodes.filter((node) => node.type !== "pause" && (scriptMark(node) === "emphasis" || scriptMark(node) === "conversational")).length} 处演绎标记 · {nodes.filter((node) => node.type === "pause").length} 处停顿；可直接点击上方口播稿中的标记调整。</p>
      </section>

      <section className="style-section">
        <span className="voice-section-label">导演风格</span>
        <div className="style-options">{styles.map((style) => <Tooltip key={style.id} title={style.description}>
          <button type="button" aria-pressed={selectedStyle === style.id} className={selectedStyle === style.id ? "selected" : ""} onClick={() => updateSegment(segment.id, { ttsStyle: style.id as TtsStyleId })}>{style.id === "auto" && <Sparkle size={12} weight="fill" />}{style.label}</button>
        </Tooltip>)}</div>
        <div className="style-capability-note"><InfoIcon />{capabilityNote}</div>
      </section>

      <section className={`voice-advanced ${advancedOpen ? "open" : ""}`}>
        <button className="advanced-trigger" onClick={() => setAdvancedOpen((open) => !open)}><span><CaretDown size={14} />高级参数</span><CaretDown size={14} /></button>
        {advancedOpen && <div className="advanced-body">
          <label><span>语速 <em>{segment.speed.toFixed(2)}×</em></span><Slider min={0.8} max={1.08} step={0.01} value={segment.speed} onChange={(speed) => updateSegment(segment.id, { speed })} /></label>
          <div><span>最大时长修正</span><strong>1.08×</strong></div>
        </div>}
      </section>
    </div>

    <footer className="inspector-footer voice-inspector-footer">
      <div className="voice-action-row">
        <Tooltip title={previewDisabledReason}><span><Button disabled={Boolean(previewDisabledReason)} loading={previewing} icon={<PlayIcon />} onClick={preview}>试听口播稿</Button></span></Tooltip>
        <Button type="primary" loading={generating || segment.status === "processing"} disabled={!selectedVoice?.available} icon={<MagicIcon />} onClick={regenerate}>{syncMode === "semantic" ? `重新生成当前语义场景 · ${syncBlockSize} 条` : syncMode === "narration" ? `重新生成当前旁白章节 · ${syncBlockSize} 条` : syncMode === "balanced" ? `重新生成当前语音块 · ${syncBlockSize} 条` : "重新生成本片段"}</Button>
      </div>
      <div className="voice-estimate">
        {estimate ? <>预计 {estimate.durationSeconds} 秒 · ¥{estimate.costCny.toFixed(2)}</> : selectedVoice?.providerId === "system" ? "预计 18 秒 · 本地合成免费" : selectedVoice?.available ? "使用已保存凭据直接生成，不会再次要求输入密钥" : "请先在服务商页完成配置"}
      </div>
      <div className="voice-data-note"><InfoIcon />{cloudVoice ? syncMode === "semantic" ? `仅向阿里百炼发送场景文字和字幕用于改写，并向${selectedVoice?.providerName ?? "当前语音服务"}发送中文口播稿与合成参数；不上传原视频或原声` : syncMode === "narration" ? "仅向阿里百炼发送当前旁白章节的中文口播稿与合成参数，不上传原视频或原声" : syncMode === "balanced" ? "仅发送当前语音块的中文口播稿与合成参数，不上传原视频或原声" : "仅发送本片段中文口播稿与合成参数，不上传原视频或原声" : "使用本地系统语音，口播稿不会离开这台 Mac"}</div>
    </footer>
  </>;
});
