import { useMemo, useState } from "react";
import { Alert, Button, Form, Input, Modal, Popconfirm, Select, Tooltip, message } from "antd";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Cloud, Key, Microphone, PencilSimple, Plus, SpeakerHigh, Trash } from "@phosphor-icons/react";
import { antdIcon } from "../ui/icons";
import { desktopBridge } from "../bridge";
import type { ProviderProfile } from "../domain";

const PlusIcon = antdIcon(Plus);
const EditIcon = antdIcon(PencilSimple);
const TrashIcon = antdIcon(Trash);

const presets = {
  deepseek: { name: "DeepSeek", baseUrl: "https://api.deepseek.com", model: "deepseek-chat" },
  bailian: { name: "阿里百炼（中国内地）", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1", model: "qwen-plus" },
  custom: { name: "自定义 OpenAI 兼容接口", baseUrl: "", model: "" },
} as const;

type ProviderForm = { preset: keyof typeof presets; name: string; baseUrl: string; model: string; secret?: string };
type ProviderCard = { id: string; name: string; type: string; model: string; connected: boolean; local: boolean; profile?: ProviderProfile };

const parseConfig = (value: string): { model?: string; baseUrl?: string } => {
  try { return JSON.parse(value); } catch { return {}; }
};

export function ProvidersPage() {
  const [testing, setTesting] = useState<string | null>(null);
  const [verified, setVerified] = useState<Record<string, boolean>>({});
  const [addOpen, setAddOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [form] = Form.useForm<ProviderForm>();
  const queryClient = useQueryClient();
  const { data: savedProviders = [] } = useQuery({ queryKey: ["providers"], queryFn: desktopBridge.listProviders });
  const visibleProviders = useMemo<ProviderCard[]>(() => [
    ...savedProviders.map((provider) => ({
      id: provider.id,
      name: provider.name,
      type: provider.kind === "translation" ? "翻译模型" : provider.kind,
      model: parseConfig(provider.publicConfigJson).model ?? "已配置",
      connected: verified[provider.id] ?? Boolean(provider.credentialRef),
      local: false,
      profile: provider,
    })),
    { id: "system", name: "macOS 系统语音", type: "中文语音", model: "普通话 · Tingting", connected: true, local: true },
  ], [savedProviders, verified]);

  const choosePreset = (preset: keyof typeof presets) => form.setFieldsValue({ preset, ...presets[preset], secret: undefined });
  const openCreate = () => { setEditingId(null); setAddOpen(true); form.resetFields(); choosePreset("deepseek"); };
  const openEdit = (profile: ProviderProfile) => {
    const config = parseConfig(profile.publicConfigJson);
    const preset = profile.id === "deepseek" ? "deepseek" : profile.id === "bailian" ? "bailian" : "custom";
    setEditingId(profile.id); setAddOpen(true);
    form.setFieldsValue({ preset, name: profile.name, baseUrl: config.baseUrl ?? "", model: config.model ?? "", secret: undefined });
  };
  const test = async (provider: ProviderCard) => {
    if (provider.local) { message.success("macOS 系统语音可用"); return; }
    setTesting(provider.id);
    try {
      if (!desktopBridge.isDesktop()) { await new Promise((resolve) => window.setTimeout(resolve, 500)); message.success("浏览器原型：连接测试已模拟完成"); }
      else {
        const result = await desktopBridge.testProvider(provider.id);
        if (result) message.success(`${result.message} · ${result.latencyMs}ms${result.availableModels ? ` · ${result.availableModels} 个模型` : ""}`);
      }
      setVerified((current) => ({ ...current, [provider.id]: true }));
    } catch (error) { setVerified((current) => ({ ...current, [provider.id]: false })); message.error(String(error)); }
    finally { setTesting(null); }
  };
  const save = async () => {
    const values = await form.validateFields();
    const id = editingId ?? values.preset;
    await desktopBridge.saveProvider({ id, kind: "translation", name: values.name, publicConfigJson: JSON.stringify({ model: values.model, baseUrl: values.baseUrl }), secret: values.secret });
    await queryClient.invalidateQueries({ queryKey: ["providers"] });
    setVerified((current) => ({ ...current, [id]: false }));
    setAddOpen(false); form.resetFields(); message.success(values.secret ? "API Key 已写入 macOS Keychain" : "服务商配置已保存");
  };
  const removeProvider = async (id: string) => { await desktopBridge.deleteProvider(id); await queryClient.invalidateQueries({ queryKey: ["providers"] }); };
  const removeAll = async () => { for (const provider of savedProviders) await desktopBridge.deleteProvider(provider.id); await queryClient.invalidateQueries({ queryKey: ["providers"] }); message.success("已删除全部服务商凭据"); };

  return <div className="page"><section className="page-header"><div><span className="eyebrow">用户自备 API Key</span><h1>服务商</h1><p>应用直接连接你选择的平台，凭据存入 macOS Keychain，不经过开发者服务器。</p></div><Button type="primary" size="large" icon={<PlusIcon />} onClick={openCreate}>添加服务商</Button></section>
    <section className="provider-hero"><div className="hero-icon"><Key size={24} /></div><div><strong>在这里填写 DeepSeek 或阿里百炼 API Key</strong><p>点击“添加服务商”选择预设并粘贴 Key。SQLite 只保存凭据引用，完整 Key 只进入 macOS Keychain。</p></div><Popconfirm title="删除全部凭据？" description="此操作会同步移除 macOS Keychain 中的凭据。" okText="删除" cancelText="取消" onConfirm={removeAll}><Button danger disabled={savedProviders.length === 0}>删除全部凭据</Button></Popconfirm></section>
    <div className="section-title"><h2>翻译与语音</h2><span>{visibleProviders.filter((provider) => provider.connected).length} 个已配置</span></div>
    <section className="provider-grid">{visibleProviders.map((provider) => <article className="provider-card" key={provider.id}><header><div className={`provider-logo ${provider.local ? "local" : "cloud"}`}>{provider.type === "中文语音" ? <SpeakerHigh /> : <Cloud />}</div><div><h3>{provider.name}</h3><p>{provider.type}</p></div><span className={`connection-chip ${provider.connected ? "connected" : ""}`}>{provider.connected ? <Check /> : null}{provider.connected ? (verified[provider.id] ? "已验证" : "已配置") : "未配置"}</span></header><div className="provider-model"><span>当前模型 / 声音</span><strong>{provider.model}</strong></div><footer><Button loading={testing === provider.id} onClick={() => test(provider)}>{testing === provider.id ? "正在测试" : "测试连接"}</Button><div>{provider.profile && <Tooltip title="编辑"><Button type="text" icon={<EditIcon />} aria-label="编辑" onClick={() => openEdit(provider.profile!)} /></Tooltip>}{!provider.local && <Popconfirm title="删除这个服务商？" okText="删除" cancelText="取消" onConfirm={() => removeProvider(provider.id)}><Tooltip title="删除"><Button type="text" danger icon={<TrashIcon />} aria-label="删除" /></Tooltip></Popconfirm>}</div></footer></article>)}</section>
    <div className="data-scope"><Microphone size={19} /><div><strong>当前中文配音使用 macOS 系统语音</strong><p>无需 Key，配音文案也不会离开这台 Mac；在线 TTS 会在后续里程碑接入。</p></div></div>
    <Modal open={addOpen} onCancel={() => setAddOpen(false)} onOk={save} title={editingId ? "编辑服务商" : "添加翻译服务"} okText="安全保存" cancelText="取消" destroyOnHidden><Form form={form} layout="vertical"><Form.Item name="preset" label="服务商预设" rules={[{ required: true }]}><Select onChange={choosePreset} disabled={Boolean(editingId)} options={[{ value: "deepseek", label: "DeepSeek" }, { value: "bailian", label: "阿里百炼（中国内地）" }, { value: "custom", label: "自定义 OpenAI 兼容接口" }]} /></Form.Item><Form.Item name="name" label="显示名称" rules={[{ required: true, message: "请输入名称" }]}><Input /></Form.Item><Form.Item name="baseUrl" label="Base URL" rules={[{ required: true, type: "url", message: "请输入有效的 HTTPS 地址" }]}><Input /></Form.Item><Form.Item name="model" label="模型" rules={[{ required: true, message: "请输入模型名称" }]}><Input /></Form.Item><Form.Item name="secret" label={editingId ? "替换 API Key（留空则保持原 Key）" : "API Key"} rules={editingId ? [] : [{ required: true, message: "请输入 API Key" }]}><Input.Password autoComplete="new-password" placeholder="只写入 macOS Keychain" /></Form.Item><Alert type="info" showIcon message="保存后请点击卡片上的“测试连接”确认 Key 和网络均可用。" /></Form></Modal>
  </div>;
}
