import { Button } from "antd";
import { useQuery } from "@tanstack/react-query";
import { CheckCircle, HardDrive, Info, ShieldCheck, Warning } from "@phosphor-icons/react";
import { desktopBridge } from "../bridge";

const fixtureRuntimes = [
  { name: "FFmpeg 媒体组件", version: "8.0.1 · arm64", size: "92 MB", status: "installed" },
  { name: "whisper.cpp", version: "1.7.6 · Metal", size: "18 MB", status: "installed" },
  { name: "Whisper small.en", version: "平衡模型", size: "466 MB", status: "installed" },
];

export function SettingsPage({ onOnboarding }: { onOnboarding: () => void }) {
  const { data: runtimeCatalog = [] } = useQuery({ queryKey: ["runtime-catalog"], queryFn: desktopBridge.listRuntimes });
  const runtimes = runtimeCatalog.length ? runtimeCatalog.map((runtime) => ({
    name: runtime.name,
    version: `${runtime.version} · ${runtime.architecture}`,
    size: runtime.sizeBytes ? `${Math.round(runtime.sizeBytes / 1024 / 1024)} MB` : "系统组件",
    status: runtime.installed ? "installed" : "available",
  })) : fixtureRuntimes;

  return <div className="page settings-page">
    <section className="page-header">
      <h1>设置</h1>
      <Button onClick={onOnboarding}>查看使用说明</Button>
    </section>
    <div className="settings-content">
      <section className="settings-section">
        <div className="section-title"><div><h2>组件与模型</h2><p>显示这台 Mac 当前可用的本地组件；组件安装暂由正式安装包管理。</p></div><span className="success-chip"><CheckCircle />基础组件状态</span></div>
        <div className="runtime-list">{runtimes.map((runtime) => <div className="runtime-row" key={runtime.name}>
          <div className="runtime-icon"><HardDrive /></div>
          <div><strong>{runtime.name}</strong><span>{runtime.version} · {runtime.size}</span></div>
          <span className={runtime.status === "installed" ? "installed" : "runtime-unavailable"}>{runtime.status === "installed" ? <><CheckCircle />已安装</> : "未安装"}</span>
        </div>)}</div>
      </section>
      <section className="settings-section privacy-settings">
        <div className="section-title"><div><h2>隐私与数据边界</h2><p>应用不采集视频、字幕正文、生成音频或行为数据。</p></div><span className="success-chip"><ShieldCheck />遥测已关闭</span></div>
        <div className="setting-row"><div><strong>本地优先</strong><span>原始视频与中间产物留在本机；在线服务的数据范围会在操作前明确说明。</span></div><span className="neutral-chip">固定关闭遥测</span></div>
      </section>
      <div className="license-warning"><Warning /><div><strong>发布门禁</strong><p>HT-Demucs 模型及推理依赖完成许可证审计前，不进入正式分发包。</p></div><Info /></div>
    </div>
  </div>;
}
