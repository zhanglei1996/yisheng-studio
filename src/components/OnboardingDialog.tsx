import { useState } from "react";
import { Button, Modal } from "antd";
import { CheckCircle, DownloadSimple, HardDrive, ShieldCheck, Sparkle } from "@phosphor-icons/react";
import { antdIcon } from "../ui/icons";

const DownloadIcon = antdIcon(DownloadSimple);

export function OnboardingDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [downloading, setDownloading] = useState(false);
  const [installed, setInstalled] = useState(false);
  const install = () => { setDownloading(true); window.setTimeout(() => { setDownloading(false); setInstalled(true); }, 1300); };
  return <Modal open={open} onCancel={onClose} width={620} centered destroyOnHidden className="onboarding-modal" title={<div className="modal-heading"><span className="eyebrow">欢迎使用</span><strong>让视频中文化留在本地</strong></div>} footer={<><Button onClick={onClose}>暂时跳过</Button><Button type="primary" onClick={onClose}>进入项目库</Button></>}><div className="onboarding-brand"><span><Sparkle weight="fill" /></span><h3>译声工坊</h3><p>原始视频和中间文件保存在你的 Mac；需要在线服务时，只发送对应阶段所需的文本。</p></div><div className="privacy-features"><div><ShieldCheck /><span><strong>清楚的数据边界</strong><small>翻译只发送字幕文本，在线 TTS 只发送中文文案。</small></span></div><div><HardDrive /><span><strong>按需安装本地组件</strong><small>组件按 Mac 架构下载，并验证哈希与签名。</small></span></div></div><div className="runtime-recommend"><div><strong>推荐识别模型</strong><span>Whisper small.en · 平衡质量与速度 · 466 MB</span></div>{installed ? <span className="success-chip"><CheckCircle />已安装</span> : <Button type="primary" icon={<DownloadIcon />} loading={downloading} onClick={install}>{downloading ? "下载中 68%" : "下载模型"}</Button>}</div></Modal>;
}
