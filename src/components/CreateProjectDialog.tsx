import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Alert, Button, Checkbox, Input, Radio, Select, Steps, message } from "antd";
import { FileVideo, FolderOpen, Lightning, ShieldCheck, SlidersHorizontal } from "@phosphor-icons/react";
import { antdIcon } from "../ui/icons";
import { desktopBridge } from "../bridge";
import type { MediaProbe } from "../domain";
import { AppModal } from "./AppModal";

const steps = ["导入视频", "生成方式", "服务配置", "确认开始"];
const FolderIcon = antdIcon(FolderOpen);

export interface ProjectCreationOptions {
  probe: MediaProbe;
  workflowMode: "quick" | "review";
  audioMode: "duck" | "mute" | "separate";
  translationProviderId: string | null;
  ttsProviderId: string;
  ttsVoiceId: string | null;
  projectName: string;
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
  const [audio, setAudio] = useState<"duck" | "mute" | "separate">("separate");
  const [rights, setRights] = useState(false);
  const [probe, setProbe] = useState<MediaProbe | null>(null);
  const [probing, setProbing] = useState(false);
  const [translationProviderId, setTranslationProviderId] = useState<string | null>(null);
  const [ttsProviderId, setTtsProviderId] = useState<string | null>(null);
  const [ttsVoiceId, setTtsVoiceId] = useState<string | null>(null);
  const [projectName, setProjectName] = useState("");
  const [dragging, setDragging] = useState(false);
  const probingRef = useRef(false);
  const { data: providers = [] } = useQuery({ queryKey: ["providers"], queryFn: desktopBridge.listProviders, enabled: open });
  const translationProviders = useMemo(() => providers.filter((provider) => provider.kind === "translation" || provider.kind === "翻译模型"), [providers]);
  const cloudTtsProviders = useMemo(() => providers.filter((provider) => provider.kind === "cloud_tts" && Boolean(provider.secretBundleRef || provider.credentialRef)), [providers]);
  const { data: ttsCatalog } = useQuery({
    queryKey: ["tts-catalog", ttsProviderId],
    queryFn: () => desktopBridge.listTtsCatalog(ttsProviderId!),
    enabled: open && Boolean(ttsProviderId),
  });

  useEffect(() => {
    if (!open) return;
    setStep(0); setMode("quick"); setAudio("separate"); setRights(false); setProbe(null); setTtsProviderId(null); setTtsVoiceId(null); setProjectName("");
  }, [open]);
  useEffect(() => {
    if (!translationProviderId && translationProviders[0]) setTranslationProviderId(translationProviders[0].id);
  }, [translationProviderId, translationProviders]);
  useEffect(() => {
    if (!ttsProviderId) setTtsProviderId(cloudTtsProviders[0]?.id ?? "system");
  }, [cloudTtsProviders, ttsProviderId]);
  useEffect(() => {
    const firstVoice = ttsCatalog?.voices.find((voice) => voice.available)?.id;
    if (firstVoice && !ttsCatalog?.voices.some((voice) => voice.id === ttsVoiceId)) setTtsVoiceId(firstVoice);
  }, [ttsCatalog, ttsVoiceId]);

  const loadPath = async (path: string) => {
    if (probingRef.current) return;
    if (!/\.(mp4|mov|mkv|m4v|webm)$/i.test(path)) {
      message.warning("仅支持 MP4、MOV、MKV、M4V 或 WebM 视频");
      return;
    }
    probingRef.current = true;
    setProbing(true);
    try {
      const nextProbe = await desktopBridge.probeMedia(path);
      setProbe(nextProbe);
      setProjectName(nextProbe.fileName.replace(/\.[^.]+$/, ""));
    } catch (error) {
      message.error(String(error));
    } finally {
      setProbing(false);
      probingRef.current = false;
    }
  };
  const chooseFile = async () => {
    if (!desktopBridge.isDesktop()) { setProbe(browserDemo); setProjectName(browserDemo.fileName.replace(/\.[^.]+$/, "")); return; }
    const path = await desktopBridge.selectVideo();
    if (path) await loadPath(path);
  };
  useEffect(() => {
    if (!open || step !== 0 || !desktopBridge.isDesktop()) return;
    let dispose: () => void = () => undefined;
    let cancelled = false;
    import("@tauri-apps/api/webview").then(({ getCurrentWebview }) => getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "enter" || event.payload.type === "over") setDragging(true);
      if (event.payload.type === "leave") setDragging(false);
      if (event.payload.type === "drop") {
        setDragging(false);
        if (event.payload.paths.length !== 1) {
          message.warning("请一次只拖入一个视频文件");
          return;
        }
        void loadPath(event.payload.paths[0]);
      }
    })).then((unlisten) => { if (cancelled) unlisten(); else dispose = unlisten; }).catch((error) => message.error(`无法启用文件拖放：${String(error)}`));
    return () => { cancelled = true; dispose(); };
  }, [open, step]);
  const next = async () => {
    if (step < steps.length - 1) { setStep(step + 1); return; }
    if (probe) await onComplete({ probe, workflowMode: mode, audioMode: audio, translationProviderId, ttsProviderId: ttsProviderId ?? "system", ttsVoiceId, projectName: projectName.trim() || probe.fileName.replace(/\.[^.]+$/, "") });
  };
  const providerName = translationProviders.find((provider) => provider.id === translationProviderId)?.name ?? "尚未配置";
  const ttsProviderName = ttsProviderId === "system" ? "macOS 系统语音（本地）" : cloudTtsProviders.find((provider) => provider.id === ttsProviderId)?.name ?? "尚未配置";
  const voiceName = ttsCatalog?.voices.find((voice) => voice.id === ttsVoiceId)?.name ?? (ttsProviderId === "system" ? "Tingting" : "加载中");

  return <AppModal open={open} onCancel={onClose} width={680} className="project-modal" title="新建项目" footer={<><Button onClick={step === 0 ? onClose : () => setStep(step - 1)}>{step === 0 ? "取消" : "上一步"}</Button><Button type="primary" disabled={step === 0 && (!rights || !probe)} onClick={next}>{step === steps.length - 1 ? "创建并开始" : "继续"}</Button></>}><Steps current={step} size="small" responsive={false} items={steps.map((title) => ({ title }))} />
    <div className="modal-content">
      {step === 0 && <><Button className={`file-drop ${dragging ? "dragging" : ""}`} loading={probing} onClick={chooseFile} onDragEnter={(event) => { event.preventDefault(); setDragging(true); }} onDragOver={(event) => { event.preventDefault(); setDragging(true); }} onDragLeave={(event) => { event.preventDefault(); setDragging(false); }} onDrop={(event) => { event.preventDefault(); setDragging(false); const file = event.dataTransfer.files[0] as File & { path?: string }; if (file?.path) void loadPath(file.path); else if (file) { const demo = { ...browserDemo, fileName: file.name, fileSize: file.size }; setProbe(demo); setProjectName(file.name.replace(/\.[^.]+$/, "")); } }}><FileVideo size={32} /><strong>{dragging ? "松开以导入视频" : probe?.fileName ?? "拖入视频或点击选择"}</strong><span>{probe ? `${probe.width} × ${probe.height} · ${formatDuration(probe.durationMs)} · ${formatBytes(probe.fileSize)}` : "MP4、MOV、MKV、M4V 或 WebM"}</span><em><FolderIcon />{probe ? "重新选择" : "选择文件"}</em></Button><div className="check-row"><Checkbox checked={rights} onChange={(event) => setRights(event.target.checked)}><span><strong>我确认拥有处理此视频的权利</strong><small>生成结果默认用于个人学习，不用于未经授权的公开传播。</small></span></Checkbox></div></>}
      {step === 1 && <><Radio.Group className="choice-grid" value={mode} onChange={(event) => setMode(event.target.value)}><Radio.Button value="quick"><Lightning size={24} /><strong>自动生成（推荐）</strong><span>识别、翻译、口播导演和配音一次完成；失败会暂停，时长提醒会进入编辑器集中处理。</span></Radio.Button><Radio.Button value="review"><SlidersHorizontal size={24} /><strong>先校对口播稿</strong><span>翻译完成后打开编辑器确认，确认前不开始云端配音。</span></Radio.Button></Radio.Group><h3 className="form-title">原声处理</h3><Radio.Group className="radio-stack" value={audio} onChange={(event) => setAudio(event.target.value)}><Radio value="separate"><span><strong>安全模式 · 只替换人声（推荐）</strong><small>本地分离并全程使用去人声背景轨；保留音乐、点击与环境声，不恢复完整原声</small></span></Radio>{[["duck", "压低原声", "保留环境感，但原视频中的英文仍可能听见"], ["mute", "静音原声", "完全移除原始音轨，只保留中文配音"]].map(([id, title, desc]) => <Radio key={id} value={id}><span><strong>{title}</strong><small>{desc}</small></span></Radio>)}</Radio.Group>{audio === "separate" && <Alert type="info" showIcon title="安全模式不会降级回混原声" description="首次处理会在本机下载分离模型。若组件、模型或分离结果不可用，任务会停止并阻止导出，避免无说话区间突然出现英文。" />}</>}
      {step === 2 && <div className="form-stack"><label><span>翻译服务</span><Select value={translationProviderId} placeholder="先在服务商页面配置" options={translationProviders.map((provider) => ({ value: provider.id, label: provider.name }))} onChange={setTranslationProviderId} /></label>{translationProviders.length === 0 && <Alert type="warning" showIcon title="还没有翻译服务" description="请先到左侧“服务商”添加翻译服务。" />}<label><span>配音引擎</span><Select value={ttsProviderId} options={[...cloudTtsProviders.map((provider) => ({ value: provider.id, label: `${provider.name} · 高级语音` })), { value: "system", label: "macOS 系统语音 · 本地免费" }]} onChange={(value) => { setTtsProviderId(value); setTtsVoiceId(null); }} /></label><label><span>项目默认声音</span><Select value={ttsVoiceId} loading={!ttsCatalog} placeholder="正在读取可用声音" options={(ttsCatalog?.voices ?? []).filter((voice) => voice.available).map((voice) => ({ value: voice.id, label: `${voice.name}${voice.traits.length ? ` · ${voice.traits.slice(0, 2).join(" / ")}` : ""}` }))} onChange={setTtsVoiceId} /></label><label><span>语音识别模型</span><Select value="small.en" options={[{ value: "small.en", label: "small.en · 平衡（推荐）" }]} /></label><div className="data-scope compact"><ShieldCheck /><div><strong>数据发送范围</strong><p>原视频、识别音频{audio === "separate" ? "和人声分离" : ""}留在本地；字幕文本发送给翻译服务{ttsProviderId === "system" ? "，配音也在本地完成。" : `；确认后的中文口播稿发送给 ${ttsProviderName}。`}</p></div></div></div>}
      {step === 3 && probe && <div className="review-summary"><label className="project-name-field"><span>项目名称</span><Input value={projectName} maxLength={120} placeholder="输入便于识别的项目名称" onChange={(event) => setProjectName(event.target.value)} /></label><div><span>源文件</span><strong>{probe.fileName}</strong></div><div><span>媒体</span><strong>{probe.width}×{probe.height} · {formatDuration(probe.durationMs)}</strong></div><div><span>处理模式</span><strong>{mode === "quick" ? "自动生成 · 无需中途确认" : "先校对口播稿"}</strong></div><div><span>翻译</span><strong>{providerName}</strong></div><div><span>配音</span><strong>{ttsProviderName} · {voiceName}</strong></div><div><span>原声</span><strong>{audio === "duck" ? "压低原声" : audio === "mute" ? "静音原声" : "安全模式 · 只替换人声"}</strong></div></div>}
    </div></AppModal>;
}

const formatDuration = (milliseconds: number) => {
  const total = Math.round(milliseconds / 1000);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  return `${hours ? `${hours}:` : ""}${String(minutes).padStart(hours ? 2 : 1, "0")}:${String(seconds).padStart(2, "0")}`;
};

const formatBytes = (bytes: number) => bytes >= 1024 ** 3 ? `${(bytes / 1024 ** 3).toFixed(1)} GB` : `${Math.round(bytes / 1024 ** 2)} MB`;
