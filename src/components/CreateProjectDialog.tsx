import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Alert, Button, Checkbox, Modal, Radio, Select, Steps, message } from "antd";
import { FileVideo, FolderOpen, Lightning, ShieldCheck, SlidersHorizontal } from "@phosphor-icons/react";
import { antdIcon } from "../ui/icons";
import { desktopBridge } from "../bridge";
import type { MediaProbe } from "../domain";

const steps = ["导入视频", "生成方式", "服务配置", "确认开始"];
const FolderIcon = antdIcon(FolderOpen);

export interface ProjectCreationOptions {
  probe: MediaProbe;
  workflowMode: "quick" | "review";
  audioMode: "duck" | "mute" | "separate";
  translationProviderId: string | null;
}

const browserDemo: MediaProbe = {
  sourcePath: "/demo/Building Reliable AI Agents.mp4",
  fingerprint: "browser-demo",
  fileName: "Building Reliable AI Agents.mp4",
  fileSize: 1_800_000_000,
  durationMs: 1_458_000,
  width: 1920,
  height: 1080,
  videoCodec: "h264",
  audioCodec: "aac",
  audioSampleRate: 48_000,
};

export function CreateProjectDialog({ open, onClose, onComplete }: { open: boolean; onClose: () => void; onComplete: (options: ProjectCreationOptions) => void | Promise<void> }) {
  const [step, setStep] = useState(0);
  const [mode, setMode] = useState<"quick" | "review">("quick");
  const [audio, setAudio] = useState<"duck" | "mute" | "separate">("duck");
  const [rights, setRights] = useState(false);
  const [probe, setProbe] = useState<MediaProbe | null>(null);
  const [probing, setProbing] = useState(false);
  const [translationProviderId, setTranslationProviderId] = useState<string | null>(null);
  const { data: providers = [] } = useQuery({ queryKey: ["providers"], queryFn: desktopBridge.listProviders, enabled: open });
  const translationProviders = useMemo(() => providers.filter((provider) => provider.kind === "translation" || provider.kind === "翻译模型"), [providers]);

  useEffect(() => {
    if (!open) return;
    setStep(0); setMode("quick"); setAudio("duck"); setRights(false); setProbe(null);
  }, [open]);
  useEffect(() => {
    if (!translationProviderId && translationProviders[0]) setTranslationProviderId(translationProviders[0].id);
  }, [translationProviderId, translationProviders]);

  const chooseFile = async () => {
    try {
      if (!desktopBridge.isDesktop()) { setProbe(browserDemo); return; }
      const path = await desktopBridge.selectVideo();
      if (!path) return;
      setProbing(true);
      setProbe(await desktopBridge.probeMedia(path));
    } catch (error) {
      message.error(String(error));
    } finally {
      setProbing(false);
    }
  };
  const next = async () => {
    if (step < steps.length - 1) { setStep(step + 1); return; }
    if (probe) await onComplete({ probe, workflowMode: mode, audioMode: audio, translationProviderId });
  };
  const providerName = translationProviders.find((provider) => provider.id === translationProviderId)?.name ?? "尚未配置";

  return <Modal open={open} onCancel={onClose} width={680} centered destroyOnHidden className="project-modal" title={<div className="modal-heading"><span className="eyebrow">新建项目</span><strong>{steps[step]}</strong></div>} footer={<><Button onClick={step === 0 ? onClose : () => setStep(step - 1)}>{step === 0 ? "取消" : "上一步"}</Button><Button type="primary" disabled={step === 0 && (!rights || !probe)} onClick={next}>{step === steps.length - 1 ? "创建并开始" : "继续"}</Button></>}><Steps current={step} size="small" items={steps.map((title) => ({ title }))} />
    <div className="modal-content">
      {step === 0 && <><Button className="file-drop" loading={probing} onClick={chooseFile}><FileVideo size={35} /><strong>{probe?.fileName ?? "选择本地英文视频"}</strong><span>{probe ? `${probe.width} × ${probe.height} · ${formatDuration(probe.durationMs)} · ${formatBytes(probe.fileSize)}` : "支持 MP4、MOV、MKV、M4V 和 WebM"}</span><em><FolderIcon />{probe ? "重新选择" : "选择文件"}</em></Button><div className="check-row"><Checkbox checked={rights} onChange={(event) => setRights(event.target.checked)}><span><strong>我确认拥有处理此视频的权利</strong><small>生成结果默认用于个人学习，不用于未经授权的公开传播。</small></span></Checkbox></div></>}
      {step === 1 && <><Radio.Group className="choice-grid" value={mode} onChange={(event) => setMode(event.target.value)}><Radio.Button value="quick"><Lightning size={24} /><strong>快速生成</strong><span>术语确认后自动完成翻译、配音与对齐。</span></Radio.Button><Radio.Button value="review"><SlidersHorizontal size={24} /><strong>先校对</strong><span>翻译完成后暂停，确认文本再批量配音。</span></Radio.Button></Radio.Group><h3 className="form-title">原声处理</h3><Radio.Group className="radio-stack" value={audio} onChange={(event) => setAudio(event.target.value)}>{[["duck", "压低原声", "保留环境感，原声降低至 -24 dB"], ["mute", "静音原声", "完全移除原始音轨"], ["separate", "高质量人声分离", "分离人声与背景，首次使用需下载模型"]].map(([id, title, desc]) => <Radio key={id} value={id}><span><strong>{title}</strong><small>{desc}</small></span></Radio>)}</Radio.Group></>}
      {step === 2 && <div className="form-stack"><label><span>翻译服务</span><Select value={translationProviderId} placeholder="先在服务商页面配置" options={translationProviders.map((provider) => ({ value: provider.id, label: provider.name }))} onChange={setTranslationProviderId} /></label>{translationProviders.length === 0 && <Alert type="warning" showIcon message="还没有翻译服务" description="项目仍可创建并完成媒体准备；到左侧“服务商”添加 DeepSeek 或阿里百炼后，再继续识别和翻译。" />}<label><span>中文语音</span><Select value="system" options={[{ value: "system", label: "macOS 系统语音 · Tingting（本地）" }]} /></label><label><span>语音识别模型</span><Select value="small.en" options={[{ value: "small.en", label: "small.en · 平衡（推荐）" }]} /></label><div className="data-scope compact"><ShieldCheck /><div><strong>数据发送范围</strong><p>原始视频和识别音频保留在本地；字幕文本仅发送给所选翻译服务。</p></div></div></div>}
      {step === 3 && probe && <div className="review-summary"><div><span>视频</span><strong>{probe.fileName}</strong></div><div><span>媒体</span><strong>{probe.width}×{probe.height} · {formatDuration(probe.durationMs)}</strong></div><div><span>处理模式</span><strong>{mode === "quick" ? "快速生成" : "先校对"}</strong></div><div><span>翻译</span><strong>{providerName}</strong></div><div><span>中文语音</span><strong>macOS 系统语音 · Tingting</strong></div><div><span>原声</span><strong>{audio === "duck" ? "压低原声" : audio === "mute" ? "静音原声" : "高质量人声分离"}</strong></div></div>}
    </div></Modal>;
}

const formatDuration = (milliseconds: number) => {
  const total = Math.round(milliseconds / 1000);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  return `${hours ? `${hours}:` : ""}${String(minutes).padStart(hours ? 2 : 1, "0")}:${String(seconds).padStart(2, "0")}`;
};

const formatBytes = (bytes: number) => bytes >= 1024 ** 3 ? `${(bytes / 1024 ** 3).toFixed(1)} GB` : `${Math.round(bytes / 1024 ** 2)} MB`;
