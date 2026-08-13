import { Button } from "antd";
import { HardDrive, ShieldCheck, Sparkle } from "@phosphor-icons/react";
import { AppModal } from "./AppModal";

export function OnboardingDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  return <AppModal open={open} onCancel={onClose} width={620} className="onboarding-modal" title="使用说明" footer={<Button type="primary" onClick={onClose}>知道了</Button>}>
    <div className="onboarding-brand"><span><Sparkle weight="fill" /></span><h3>译声工坊</h3><p>原始视频和中间文件保存在你的 Mac；需要在线服务时，只发送对应阶段所需的文本。</p></div>
    <div className="privacy-features"><div><ShieldCheck /><span><strong>清楚的数据边界</strong><small>翻译只发送字幕文本，在线 TTS 只发送中文文案。</small></span></div><div><HardDrive /><span><strong>本地组件随应用管理</strong><small>设置页会显示当前组件状态；缺失组件需要通过正式安装包补齐。</small></span></div></div>
    <div className="runtime-recommend"><div><strong>推荐识别模型</strong><span>Whisper small.en · 平衡质量与速度 · 466 MB</span></div><span className="neutral-chip">在设置中查看状态</span></div>
  </AppModal>;
}
