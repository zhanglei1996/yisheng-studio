import { useState } from "react";
import { Button, Progress, Segmented, Switch } from "antd";
import { useQuery } from "@tanstack/react-query";
import { CheckCircle, DownloadSimple, FolderOpen, HardDrive, Info, ShieldCheck, Trash, Warning } from "@phosphor-icons/react";
import { antdIcon } from "../ui/icons";
import { desktopBridge } from "../bridge";

const DownloadIcon = antdIcon(DownloadSimple);
const FolderIcon = antdIcon(FolderOpen);
const TrashIcon = antdIcon(Trash);

const fixtureRuntimes = [
  { name: "FFmpeg 媒体组件", version: "8.0.1 · arm64", size: "92 MB", status: "installed" },
  { name: "whisper.cpp", version: "1.7.6 · Metal", size: "18 MB", status: "installed" },
  { name: "Whisper small.en", version: "平衡模型", size: "466 MB", status: "installed" },
  { name: "高质量人声分离", version: "HT-Demucs · CoreML", size: "316 MB", status: "available" },
];

export function SettingsPage({ onOnboarding }: { onOnboarding: () => void }) {
  const [downloads, setDownloads] = useState<string[]>([]);
  const { data: runtimeCatalog = [] } = useQuery({ queryKey: ["runtime-catalog"], queryFn: desktopBridge.listRuntimes });
  const runtimes = runtimeCatalog.length ? runtimeCatalog.map((runtime) => ({
    name: runtime.name,
    version: `${runtime.version} · ${runtime.architecture}`,
    size: runtime.sizeBytes ? `${Math.round(runtime.sizeBytes / 1024 / 1024)} MB` : "系统组件",
    status: runtime.installed ? "installed" : "available",
  })) : fixtureRuntimes;
  const startDownload = (name: string) => { setDownloads([...downloads, name]); window.setTimeout(() => setDownloads((items) => items.filter((item) => item !== name)), 1400); };
  return <div className="page settings-page"><section className="page-header"><div><span className="eyebrow">本地运行时</span><h1>设置</h1><p>管理模型、缓存、隐私和应用更新。</p></div><Button onClick={onOnboarding}>重新运行首次引导</Button></section>
    <div className="settings-layout"><nav className="settings-nav"><Segmented vertical options={["组件与模型", "存储与缓存", "隐私与诊断", "应用更新"]} defaultValue="组件与模型" /></nav><div className="settings-content">
      <section className="settings-section"><div className="section-title"><div><h2>组件与模型</h2><p>组件会按当前 Mac 架构下载并校验签名。</p></div><span className="success-chip"><CheckCircle />基础组件可用</span></div><div className="runtime-list">{runtimes.map((runtime) => <div className="runtime-row" key={runtime.name}><div className="runtime-icon"><HardDrive /></div><div><strong>{runtime.name}</strong><span>{runtime.version} · {runtime.size}</span></div>{runtime.status === "installed" ? <span className="installed"><CheckCircle />已安装</span> : <Button icon={<DownloadIcon />} loading={downloads.includes(runtime.name)} onClick={() => startDownload(runtime.name)}>{downloads.includes(runtime.name) ? "下载中" : "下载"}</Button>}</div>)}</div></section>
      <section className="settings-section"><div className="section-title"><div><h2>存储与缓存</h2><p>项目引用原始文件，中间产物由应用管理。</p></div></div><Progress percent={42} showInfo={false} /><div className="storage-labels"><span>应用缓存 12.6 GB</span><em>本地可用 1.23 TB</em></div><div className="button-row"><Button icon={<FolderIcon />}>打开缓存目录</Button><Button danger icon={<TrashIcon />}>清理未使用缓存</Button></div></section>
      <section className="settings-section privacy-settings"><div className="section-title"><div><h2>隐私与诊断</h2><p>默认不采集视频、字幕正文、生成音频或行为数据。</p></div><span className="success-chip"><ShieldCheck />遥测已关闭</span></div><div className="setting-row"><div><strong>匿名使用统计</strong><span>保持关闭；V1 不提供后台遥测。</span></div><Switch aria-label="匿名使用统计" defaultChecked={false} /></div><div className="setting-row"><div><strong>生成脱敏诊断包</strong><span>排除凭据、正文、音频、完整路径和文件名。</span></div><Button>生成诊断包</Button></div></section>
      <div className="license-warning"><Warning /><div><strong>发布门禁</strong><p>HT-Demucs 模型及推理依赖完成许可证审计前，不进入正式分发包。</p></div><Info /></div>
    </div></div>
  </div>;
}
