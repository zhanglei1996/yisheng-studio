import { useMemo, useState } from "react";
import { Alert, Button, Form, Input, Popconfirm, Select, Tooltip, message } from "antd";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Check,
  Cloud,
  Key,
  Microphone,
  PencilSimple,
  Plus,
  ShieldCheck,
  SpeakerHigh,
  Trash,
  WarningCircle,
} from "@phosphor-icons/react";
import { antdIcon } from "../ui/icons";
import { desktopBridge } from "../bridge";
import type { ProviderProfile } from "../domain";
import { AppModal } from "../components/AppModal";
import "./ProvidersPage.css";

const PlusIcon = antdIcon(Plus);
const EditIcon = antdIcon(PencilSimple);
const TrashIcon = antdIcon(Trash);

const presetDefaults = {
  deepseek: {
    name: "DeepSeek",
    baseUrl: "https://api.deepseek.com",
    model: "deepseek-chat",
  },
  bailian: {
    name: "阿里百炼翻译（中国内地）",
    baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    model: "qwen-plus",
  },
  custom: {
    name: "自定义 OpenAI 兼容接口",
    baseUrl: "",
    model: "",
  },
  bailian_tts: {
    name: "阿里百炼 TTS",
    baseUrl: "https://dashscope.aliyuncs.com/api/v1",
    region: "cn-beijing",
    model: "qwen3-tts-instruct-flash",
    voice: "Cherry",
  },
  iflytek_tts: {
    name: "讯飞超拟人语音",
    baseUrl: "wss://cbm01.cn-huabei-1.xf-yun.com/v1/private/mcd9m97e6",
    model: "super-human-tts",
    authMode: "api_password",
    voice: "",
  },
} as const;

type ProviderPreset = keyof typeof presetDefaults;
type AuthMode = "api_password" | "api_key_secret";
type TestState = "idle" | "testing" | "success" | "error" | "desktop_only";

type ProviderForm = {
  preset: ProviderPreset;
  name: string;
  baseUrl: string;
  model: string;
  secret?: string;
  region?: "cn-beijing" | "ap-southeast-1";
  voice?: string;
  appId?: string;
  authMode?: AuthMode;
  apiPassword?: string;
  apiKey?: string;
  apiSecret?: string;
};

type ProviderPublicConfig = {
  vendor?: string;
  baseUrl?: string;
  model?: string;
  region?: string;
  voice?: string;
  appId?: string;
  authMode?: AuthMode;
  dataScope?: "text_only";
};

type ProviderCard = {
  id: string;
  name: string;
  type: string;
  model: string;
  voice?: string;
  configured: boolean;
  local: boolean;
  profile?: ProviderProfile;
};

const providerIds: Record<ProviderPreset, string> = {
  deepseek: "deepseek",
  bailian: "bailian",
  custom: "custom",
  bailian_tts: "bailian-tts",
  iflytek_tts: "iflytek-super-tts",
};

const bailianRegionUrls = {
  "cn-beijing": "https://dashscope.aliyuncs.com/api/v1",
  "ap-southeast-1": "https://dashscope-intl.aliyuncs.com/api/v1",
} as const;

const presetOptions = [
  {
    label: "翻译服务",
    options: [
      { value: "deepseek", label: "DeepSeek" },
      { value: "bailian", label: "阿里百炼翻译" },
      { value: "custom", label: "自定义 OpenAI 兼容接口" },
    ],
  },
  {
    label: "在线中文语音",
    options: [
      { value: "bailian_tts", label: "阿里百炼 TTS（Qwen / CosyVoice）" },
      { value: "iflytek_tts", label: "讯飞超拟人语音" },
    ],
  },
];

const bailianModelOptions = [
  { value: "qwen3-tts-instruct-flash", label: "Qwen3-TTS Instruct Flash（默认）" },
  { value: "cosyvoice-v3-flash", label: "CosyVoice v3 Flash" },
  { value: "cosyvoice-v3-plus", label: "CosyVoice v3 Plus" },
];

const defaultVoiceForBailianModel = (model: string) => {
  if (model === "cosyvoice-v3-plus") return "longanhuan";
  if (model.startsWith("cosyvoice")) return "longanhuan_v3";
  return "Cherry";
};

const parseConfig = (value: string): ProviderPublicConfig => {
  try {
    return JSON.parse(value) as ProviderPublicConfig;
  } catch {
    return {};
  }
};

const presetForProfile = (profile: ProviderProfile): ProviderPreset => {
  const config = parseConfig(profile.publicConfigJson);
  if (profile.id === "bailian-tts" || config.vendor === "bailian_tts") return "bailian_tts";
  if (profile.id === "iflytek-super-tts" || config.vendor === "iflytek") return "iflytek_tts";
  if (profile.id === "deepseek") return "deepseek";
  if (profile.id === "bailian") return "bailian";
  return "custom";
};

const providerTypeLabel = (kind: string) => {
  if (kind === "translation") return "翻译模型";
  if (kind === "cloud_tts") return "在线中文语音";
  return kind;
};

const statusLabel = (card: ProviderCard, state: TestState) => {
  if (card.local) return "系统可用";
  if (state === "testing") return "测试中";
  if (state === "success") return "已验证";
  if (state === "error") return "测试失败";
  if (state === "desktop_only") return "需在 App 测试";
  return card.configured ? "已配置" : "未配置";
};

export function ProvidersPage() {
  const [testStates, setTestStates] = useState<Record<string, TestState>>({});
  const [addOpen, setAddOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingCredentialReady, setEditingCredentialReady] = useState(false);
  const [editingAuthMode, setEditingAuthMode] = useState<AuthMode | null>(null);
  const [editingAppId, setEditingAppId] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [form] = Form.useForm<ProviderForm>();
  const queryClient = useQueryClient();
  const selectedPreset = Form.useWatch("preset", form) ?? "deepseek";
  const selectedAuthMode = Form.useWatch("authMode", form) ?? "api_password";
  const { data: savedProviders = [] } = useQuery({
    queryKey: ["providers"],
    queryFn: desktopBridge.listProviders,
  });

  const visibleProviders = useMemo<ProviderCard[]>(
    () => [
      ...savedProviders.map((provider) => {
        const config = parseConfig(provider.publicConfigJson);
        return {
          id: provider.id,
          name: provider.name,
          type: providerTypeLabel(provider.kind),
          model: config.model ?? "已配置",
          voice: config.voice,
          configured: Boolean(provider.secretBundleRef || provider.credentialRef),
          local: false,
          profile: provider,
        };
      }),
      {
        id: "system",
        name: "macOS 系统语音",
        type: "本地中文语音",
        model: "普通话",
        voice: "Tingting",
        configured: true,
        local: true,
      },
    ],
    [savedProviders],
  );

  const choosePreset = (preset: ProviderPreset) => {
    form.resetFields([
      "name",
      "baseUrl",
      "model",
      "secret",
      "region",
      "voice",
      "appId",
      "authMode",
      "apiPassword",
      "apiKey",
      "apiSecret",
    ]);
    form.setFieldsValue({ preset, ...presetDefaults[preset] });
  };

  const openCreate = () => {
    setEditingId(null);
    setEditingCredentialReady(false);
    setEditingAuthMode(null);
    setEditingAppId(null);
    setAddOpen(true);
    form.resetFields();
    choosePreset("deepseek");
  };

  const openEdit = (profile: ProviderProfile) => {
    const config = parseConfig(profile.publicConfigJson);
    const preset = presetForProfile(profile);
    setEditingId(profile.id);
    setEditingCredentialReady(Boolean(profile.secretBundleRef || profile.credentialRef));
    setEditingAuthMode(config.authMode ?? null);
    setEditingAppId(config.appId?.trim() ?? null);
    setAddOpen(true);
    form.resetFields();
    form.setFieldsValue({
      preset,
      name: profile.name,
      baseUrl: config.baseUrl ?? presetDefaults[preset].baseUrl,
      model: config.model ?? presetDefaults[preset].model,
      region: config.region as ProviderForm["region"],
      voice: config.voice,
      appId: config.appId,
      authMode: config.authMode ?? "api_password",
    });
  };

  const test = async (provider: ProviderCard) => {
    if (provider.local) {
      setTestStates((current) => ({ ...current, [provider.id]: "success" }));
      message.success("macOS 系统语音可用，合成全程在本机完成");
      return;
    }
    if (!desktopBridge.isDesktop()) {
      setTestStates((current) => ({ ...current, [provider.id]: "desktop_only" }));
      message.info("连接测试需在 macOS App 内运行；浏览器不会读取 Keychain 凭据");
      return;
    }

    setTestStates((current) => ({ ...current, [provider.id]: "testing" }));
    try {
      const result = await desktopBridge.testProvider(provider.id);
      if (!result?.ok) throw new Error("服务商未返回可用状态");
      setTestStates((current) => ({ ...current, [provider.id]: "success" }));
      message.success(
        `${result.message} · ${result.latencyMs}ms${result.availableModels ? ` · ${result.availableModels} 个模型` : ""}`,
      );
    } catch (error) {
      setTestStates((current) => ({ ...current, [provider.id]: "error" }));
      message.error(String(error));
    }
  };

  const save = async () => {
    setSaving(true);
    try {
      const values = await form.validateFields();
      const id = editingId ?? providerIds[values.preset];
      const isTranslation = ["deepseek", "bailian", "custom"].includes(values.preset);
      let publicConfig: ProviderPublicConfig;
      let secret = values.secret?.trim() || undefined;

      if (values.preset === "bailian_tts") {
        const region = values.region ?? "cn-beijing";
        publicConfig = {
          vendor: "bailian_tts",
          baseUrl: bailianRegionUrls[region],
          region,
          model: values.model,
          voice: values.voice,
          dataScope: "text_only",
        };
        if (secret) secret = JSON.stringify({ apiKey: secret });
      } else if (values.preset === "iflytek_tts") {
        const authMode = values.authMode ?? "api_password";
        publicConfig = {
          vendor: "iflytek",
          baseUrl: presetDefaults.iflytek_tts.baseUrl,
          model: presetDefaults.iflytek_tts.model,
          appId: values.appId?.trim(),
          authMode,
          voice: values.voice?.trim(),
          dataScope: "text_only",
        };
        if (authMode === "api_password" && values.apiPassword?.trim()) {
          secret = JSON.stringify({ authMode, appId: values.appId?.trim(), apiPassword: values.apiPassword.trim() });
        }
        if (authMode === "api_key_secret" && values.apiKey?.trim() && values.apiSecret?.trim()) {
          secret = JSON.stringify({
            authMode,
            appId: values.appId?.trim(),
            apiKey: values.apiKey.trim(),
            apiSecret: values.apiSecret.trim(),
          });
        }
      } else {
        publicConfig = {
          baseUrl: values.baseUrl?.trim(),
          model: values.model?.trim(),
        };
      }

      await desktopBridge.saveProvider({
        id,
        kind: isTranslation ? "translation" : "cloud_tts",
        name: values.name.trim(),
        publicConfigJson: JSON.stringify(publicConfig),
        secret,
        driver: values.preset === "bailian_tts" ? "aliyun_tts" : values.preset === "iflytek_tts" ? "iflytek_super_tts" : undefined,
      });
      await queryClient.invalidateQueries({ queryKey: ["providers"] });
      setTestStates((current) => ({ ...current, [id]: "idle" }));
      setAddOpen(false);
      form.resetFields();
      if (!desktopBridge.isDesktop()) {
        message.info("浏览器预览不会保存凭据，请在 macOS App 内完成配置");
      } else {
        message.success(secret ? "凭据已写入 macOS Keychain，不会在页面回显" : "公开配置已保存，原凭据保持不变");
      }
    } catch (error) {
      message.error(`保存服务商失败：${String(error)}`);
    } finally {
      setSaving(false);
    }
  };

  const removeProvider = async (id: string) => {
    try {
      await desktopBridge.deleteProvider(id);
      await queryClient.invalidateQueries({ queryKey: ["providers"] });
      setTestStates((current) => {
        const next = { ...current };
        delete next[id];
        return next;
      });
      message.success("服务商与 Keychain 凭据已删除");
    } catch (error) {
      message.error(`删除服务商失败：${String(error)}`);
    }
  };

  const removeAll = async () => {
    try {
      for (const provider of savedProviders) await desktopBridge.deleteProvider(provider.id);
      await queryClient.invalidateQueries({ queryKey: ["providers"] });
      message.success("所有自定义服务商已删除");
    } catch (error) {
      await queryClient.invalidateQueries({ queryKey: ["providers"] });
      message.error(`删除未全部完成：${String(error)}`);
    }
    setTestStates({});
    message.success("已删除全部云服务商凭据，本地系统语音不受影响");
  };

  const editingKeepsCredential = Boolean(editingId) && editingCredentialReady;
  const currentAppId = Form.useWatch("appId", form)?.trim() ?? "";
  const iflytekCredentialCanRemain = editingKeepsCredential
    && editingAuthMode === selectedAuthMode
    && editingAppId === currentAppId;
  const isTranslationForm = ["deepseek", "bailian", "custom"].includes(selectedPreset);

  return (
    <div className="page provider-page">
      <section className="page-header">
        <h1>服务商</h1>
        <Button type="primary" size="large" icon={<PlusIcon />} onClick={openCreate}>
          添加服务商
        </Button>
      </section>

      <section className="provider-hero">
        <div className="hero-icon"><Key size={24} /></div>
        <div>
          <strong>在线 TTS 只发送待合成的中文文案</strong>
          <p>不会上传原视频、原始音轨、字幕工程或其他本地文件；服务商只接收合成所需的文本和语音参数。</p>
        </div>
        <Popconfirm
          title="删除全部云服务商凭据？"
          description="此操作会同步移除 macOS Keychain 中的相关凭据。"
          okText="删除"
          cancelText="取消"
          onConfirm={removeAll}
        >
          <Button danger disabled={savedProviders.length === 0}>删除全部凭据</Button>
        </Popconfirm>
      </section>

      <div className="section-title">
        <h2>翻译与语音</h2>
        <span>{visibleProviders.filter((provider) => provider.configured).length} 个已配置</span>
      </div>

      <section className="provider-grid">
        {visibleProviders.map((provider) => {
          const state = testStates[provider.id] ?? "idle";
          const successful = provider.local || state === "success";
          return (
            <article className={`provider-card ${provider.type === "在线中文语音" ? "online-tts" : ""}`} key={provider.id}>
              <header>
                <div className={`provider-logo ${provider.local ? "local" : "cloud"}`}>
                  {provider.type.includes("语音") ? <SpeakerHigh /> : <Cloud />}
                </div>
                <div>
                  <h3>{provider.name}</h3>
                  <p>{provider.type}</p>
                </div>
                <span className={`connection-chip ${successful ? "connected" : ""} ${state}`}>
                  {successful ? <Check /> : state === "error" ? <WarningCircle /> : null}
                  {statusLabel(provider, state)}
                </span>
              </header>
              <div className="provider-model">
                <span>当前模型 / 声音</span>
                <strong>{provider.model}{provider.voice ? ` · ${provider.voice}` : ""}</strong>
              </div>
              {provider.type === "在线中文语音" && (
                <div className="provider-data-note"><ShieldCheck size={14} />仅发送待合成文本与语音参数</div>
              )}
              <footer>
                <Button loading={state === "testing"} onClick={() => test(provider)}>
                  {state === "testing" ? "正在测试" : "测试连接"}
                </Button>
                <div>
                  {provider.profile && (
                    <Tooltip title="编辑">
                      <Button
                        type="text"
                        icon={<EditIcon />}
                        aria-label={`编辑 ${provider.name}`}
                        onClick={() => openEdit(provider.profile!)}
                      />
                    </Tooltip>
                  )}
                  {!provider.local && (
                    <Popconfirm
                      title="删除这个服务商？"
                      description="同时删除 Keychain 中保存的凭据。"
                      okText="删除"
                      cancelText="取消"
                      onConfirm={() => removeProvider(provider.id)}
                    >
                      <Tooltip title="删除">
                        <Button type="text" danger icon={<TrashIcon />} aria-label={`删除 ${provider.name}`} />
                      </Tooltip>
                    </Popconfirm>
                  )}
                </div>
              </footer>
            </article>
          );
        })}
      </section>

      <div className="data-scope">
        <Microphone size={19} />
        <div>
          <strong>macOS 系统语音可作为你主动选择的本地方案</strong>
          <p>系统语音无需 Key，中文文案不会离开这台 Mac；云服务失败时不会静默切换音色。</p>
        </div>
      </div>

      <AppModal
        className="provider-modal"
        width={640}
        open={addOpen}
        onCancel={() => setAddOpen(false)}
        onOk={save}
        confirmLoading={saving}
        title={editingId ? "编辑服务商" : "添加服务商"}
        okText="安全保存"
        cancelText="取消"
        destroyOnHidden
      >
        <Form form={form} layout="vertical" requiredMark={false}>
          <Form.Item name="preset" label="服务商预设" rules={[{ required: true }]}>
            <Select
              options={presetOptions}
              onChange={(value: ProviderPreset) => choosePreset(value)}
              disabled={Boolean(editingId)}
            />
          </Form.Item>
          <Form.Item name="name" label="显示名称" rules={[{ required: true, message: "请输入名称" }]}>
            <Input />
          </Form.Item>

          {isTranslationForm && (
            <>
              <Form.Item
                name="baseUrl"
                label="Base URL"
                rules={[{ required: true, type: "url", message: "请输入有效的 HTTPS 地址" }]}
              >
                <Input />
              </Form.Item>
              <Form.Item name="model" label="模型" rules={[{ required: true, message: "请输入模型名称" }]}>
                <Input />
              </Form.Item>
              <Form.Item
                name="secret"
                label="API Key"
                extra={editingKeepsCredential ? "当前 Key 已安全保存且不会回显；留空则保持不变。" : undefined}
                rules={editingKeepsCredential ? [] : [{ required: true, message: "请输入 API Key" }]}
              >
                <Input.Password autoComplete="new-password" placeholder="只写入 macOS Keychain" />
              </Form.Item>
            </>
          )}

          {selectedPreset === "bailian_tts" && (
            <>
              <Form.Item name="region" label="地域" rules={[{ required: true }]}>
                <Select
                  options={[
                    { value: "cn-beijing", label: "中国内地（北京）" },
                    { value: "ap-southeast-1", label: "国际（新加坡）" },
                  ]}
                  onChange={(region: keyof typeof bailianRegionUrls) => {
                    form.setFieldValue("baseUrl", bailianRegionUrls[region]);
                    if (region === "ap-southeast-1" && String(form.getFieldValue("model") ?? "").startsWith("cosyvoice")) {
                      form.setFieldValue("model", "qwen3-tts-instruct-flash");
                      form.setFieldValue("voice", "Cherry");
                      message.info("CosyVoice HTTP 合成当前仅支持北京地域，已切换为 Qwen3-TTS");
                    }
                  }}
                />
              </Form.Item>
              <Form.Item name="baseUrl" hidden><Input /></Form.Item>
              <Form.Item name="model" label="语音模型" rules={[{ required: true }]}>
                <Select
                  options={bailianModelOptions}
                  onChange={(model: string) => {
                    if (model.startsWith("cosyvoice") && form.getFieldValue("region") === "ap-southeast-1") {
                      form.setFieldValue("region", "cn-beijing");
                      form.setFieldValue("baseUrl", bailianRegionUrls["cn-beijing"]);
                      message.info("CosyVoice HTTP 合成当前仅支持北京地域，已同步切换地域");
                    }
                    form.setFieldValue("voice", defaultVoiceForBailianModel(model));
                  }}
                />
              </Form.Item>
              <Form.Item
                name="voice"
                label="音色"
                rules={[{ required: true, message: "请输入与模型匹配的音色" }]}
                extra="Qwen3-TTS 默认 Cherry；CosyVoice v3 Flash 与 v3 Plus 的音色 ID 不同，已随模型自动匹配。"
              >
                <Input />
              </Form.Item>
              <Form.Item
                name="secret"
                label="DashScope API Key"
                extra={editingKeepsCredential ? "当前 Key 已安全保存且不会回显；留空则保持不变。" : undefined}
                rules={editingKeepsCredential ? [] : [{ required: true, message: "请输入 DashScope API Key" }]}
              >
                <Input.Password autoComplete="new-password" placeholder="只写入 macOS Keychain" />
              </Form.Item>
            </>
          )}

          {selectedPreset === "iflytek_tts" && (
            <>
              <Form.Item name="baseUrl" hidden><Input /></Form.Item>
              <Form.Item name="model" hidden><Input /></Form.Item>
              <Form.Item
                name="appId"
                label="AppID"
                extra={editingKeepsCredential && editingAppId !== currentAppId ? "AppID 已变更，请重新输入对应凭据。" : undefined}
                rules={[{ required: true, message: "请输入 AppID" }]}
              >
                <Input autoComplete="off" />
              </Form.Item>
              <Form.Item name="voice" label="音色（VCN）" rules={[{ required: true, message: "请输入已授权音色" }]}>
                <Input placeholder="例如：x6_lingxiaoyue_flow（以控制台授权为准）" />
              </Form.Item>
              <Form.Item name="authMode" label="鉴权方式" rules={[{ required: true }]}>
                <Select
                  options={[
                    { value: "api_password", label: "APIPassword" },
                    { value: "api_key_secret", label: "APIKey + APISecret" },
                  ]}
                  onChange={() => form.resetFields(["apiPassword", "apiKey", "apiSecret"])}
                />
              </Form.Item>
              {selectedAuthMode === "api_password" ? (
                <Form.Item
                  name="apiPassword"
                  label="APIPassword"
                  extra={iflytekCredentialCanRemain ? "当前凭据已安全保存且不会回显；留空则保持不变。" : undefined}
                  rules={iflytekCredentialCanRemain ? [] : [{ required: true, message: "请输入 APIPassword" }]}
                >
                  <Input.Password autoComplete="new-password" placeholder="只写入 macOS Keychain" />
                </Form.Item>
              ) : (
                <div className="provider-secret-grid">
                  <Form.Item
                    name="apiKey"
                    label="APIKey"
                    extra={iflytekCredentialCanRemain ? "留空则保持原凭据" : undefined}
                    dependencies={["apiSecret"]}
                    rules={[
                      {
                        validator: async (_, value?: string) => {
                          if (!iflytekCredentialCanRemain && !value?.trim()) throw new Error("请输入 APIKey");
                          if (value?.trim() && !form.getFieldValue("apiSecret")?.trim()) throw new Error("请同时输入 APISecret");
                        },
                      },
                    ]}
                  >
                    <Input.Password autoComplete="new-password" />
                  </Form.Item>
                  <Form.Item
                    name="apiSecret"
                    label="APISecret"
                    extra={iflytekCredentialCanRemain ? "留空则保持原凭据" : undefined}
                    dependencies={["apiKey"]}
                    rules={[
                      {
                        validator: async (_, value?: string) => {
                          if (!iflytekCredentialCanRemain && !value?.trim()) throw new Error("请输入 APISecret");
                          if (value?.trim() && !form.getFieldValue("apiKey")?.trim()) throw new Error("请同时输入 APIKey");
                        },
                      },
                    ]}
                  >
                    <Input.Password autoComplete="new-password" />
                  </Form.Item>
                </div>
              )}
            </>
          )}

          {!isTranslationForm && (
            <Alert
              className="provider-privacy-note"
              type="info"
              showIcon
              title="数据范围：只发送待合成文本"
              description="原视频、音轨、项目文件与完整字幕工程始终保留在本机。保存后请回到卡片执行连接测试。"
            />
          )}
          {isTranslationForm && (
            <Alert type="info" showIcon title="保存后请在卡片上执行连接测试，确认凭据、模型和网络均可用。" />
          )}
        </Form>
      </AppModal>
    </div>
  );
}
