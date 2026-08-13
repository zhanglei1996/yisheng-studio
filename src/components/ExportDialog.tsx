import { useEffect, useState } from "react";
import { Alert, Button, Input, Segmented } from "antd";
import { CheckCircle, Export, FileAudio, FileText, FileVideo, FolderOpen, FolderSimple } from "@phosphor-icons/react";
import { useQuery } from "@tanstack/react-query";
import { desktopBridge } from "../bridge";
import { message } from "antd";
import { AppModal } from "./AppModal";

const DEFAULT_OUTPUT = "~/Movies/译声工坊";

async function chooseDirectory(current: string) {
  if (!(window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__) return current;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({ directory: true, multiple: false, defaultPath: current });
  return typeof selected === "string" ? selected : current;
}

export function ExportDialog({ open, onClose, onResolveIssues, projectId }: { open: boolean; onClose: () => void; onResolveIssues?: (kind: "failed" | "timing") => void; projectId: string | null }) {
  const [subtitle, setSubtitle] = useState("中文字幕");
  const [exportPreset, setExportPreset] = useState("balanced");
  const [outputDirectory, setOutputDirectory] = useState(DEFAULT_OUTPUT);
  const [exporting, setExporting] = useState(false);
  const [done, setDone] = useState(false);
  const [resultDirectory, setResultDirectory] = useState<string | null>(null);
  const [warningAcknowledged, setWarningAcknowledged] = useState(false);
  const { data: jobs = [] } = useQuery({ queryKey: ["jobs"], queryFn: desktopBridge.listJobs, enabled: open });
  const { data: projects = [] } = useQuery({ queryKey: ["projects"], queryFn: desktopBridge.listProjects, enabled: open });
  const { data: preflight, isLoading: checking } = useQuery({ queryKey: ["export-preflight", projectId], queryFn: () => desktopBridge.getExportPreflight(projectId!), enabled: open && Boolean(projectId) });
  const project = projects.find((item) => item.id === projectId);
  const durationSeconds = (project?.durationMs ?? 0) / 1000;
  const presetBitrate = exportPreset === "share" ? 1_800_000 : exportPreset === "high" ? 5_000_000 : 2_800_000;
  const estimatedBytes = durationSeconds * ((presetBitrate + 192_000) / 8 + 48_000 * 2);
  const estimatedOutput = estimatedBytes > 0 ? `约 ${Math.max(1, Math.round(estimatedBytes / 1024 / 1024))} MiB` : "将在开始前计算";

  useEffect(() => {
    if (!open) window.setTimeout(() => { setDone(false); setWarningAcknowledged(false); }, 160);
    if (open && desktopBridge.isDesktop()) {
      import("@tauri-apps/api/path").then(({ homeDir }) => homeDir()).then((home) => setOutputDirectory(`${home}/Movies/译声工坊`)).catch(() => undefined);
    }
  }, [open]);

  const start = async () => {
    if (!projectId) { message.warning("请先打开要导出的项目"); return; }
    const job = jobs.find((item) => item.projectId === projectId);
    if (!job) { message.warning("项目没有可用任务"); return; }
    setExporting(true);
    try {
      const mode = subtitle === "不烧录" ? "none" : subtitle === "中英双语" ? "bilingual" : "chinese";
      const result = await desktopBridge.startExport(projectId, job.id, outputDirectory, mode, exportPreset);
      setResultDirectory(result?.directory ?? outputDirectory); setDone(true);
    } catch (error) { message.error(String(error)); }
    finally { setExporting(false); }
  };

  const browse = async () => setOutputDirectory(await chooseDirectory(outputDirectory));

  return (
    <AppModal
      open={open}
      onCancel={onClose}
      width={620}
      title="导出视频"
      footer={done
        ? <Button type="primary" onClick={onClose}>完成</Button>
        : <><Button onClick={onClose}>取消</Button><Button type="primary" icon={<Export />} loading={exporting || checking} disabled={Boolean(preflight && (!preflight.canExport || (preflight.warningCount > 0 && !warningAcknowledged)))} onClick={start}>{exporting ? "正在准备导出" : preflight?.warningCount && warningAcknowledged ? "仍然导出" : "开始导出"}</Button></>}
      className="export-modal"
    >
      {done ? (
        <div className="export-success antd-success">
          <CheckCircle size={52} weight="fill" />
          <h3>导出完成</h3>
          <p>视频、配音同步字幕、忠实翻译字幕和中文音轨已保存。</p>
          <Button icon={<FolderOpen size={16} />} onClick={() => resultDirectory && desktopBridge.revealInFinder(resultDirectory).catch(() => message.info(resultDirectory))}>查看保存位置</Button>
        </div>
      ) : (
        <div className="export-form">
          {preflight && !preflight.canExport && <Alert type="error" showIcon title={`${preflight.blockingCount} 个发布问题阻止导出`} description={<div className="preflight-description"><span>{preflight.message}</span><Button danger onClick={() => onResolveIssues?.("failed")}>定位并修复</Button></div>} />}
          {preflight && preflight.canExport && preflight.warningCount > 0 && <Alert type="warning" showIcon title={`${preflight.warningCount} 个发布提醒待确认`} description={<div className="preflight-description"><span>建议先处理字幕闪现、阅读速度或时长问题；确认后仍可导出。</span><div><Button onClick={() => onResolveIssues?.("timing")}>返回自动修复</Button><Button type={warningAcknowledged ? "primary" : "default"} onClick={() => setWarningAcknowledged(true)}>{warningAcknowledged ? "已确认风险" : "仍然导出"}</Button></div></div>} />}
          {preflight && preflight.canExport && preflight.warningCount === 0 && <Alert type="success" showIcon title="项目已通过发布检查" description="配音版本、非语音事件、字幕可读性与时间线当前均可安全导出。" />}
          {preflight?.checks.length ? <div className="publish-check-list" aria-label="发布检查详情">{preflight.checks.map((check, index) => <div className={`publish-check ${check.severity}`} key={`${check.code}-${index}`}><i /><span><strong>{check.message}</strong>{check.sourceRange && <small>{`${(check.sourceRange[0] / 1000).toFixed(1)}s – ${(check.sourceRange[1] / 1000).toFixed(1)}s`}</small>}{check.suggestedAction && <small>{check.suggestedAction}</small>}</span></div>)}</div> : null}
          <label className="export-field-label" htmlFor="export-directory">输出目录</label>
          <Input
            id="export-directory"
            size="large"
            value={outputDirectory}
            readOnly
            prefix={<FolderSimple size={16} />}
            suffix={<Button type="text" size="small" icon={<FolderOpen size={16} />} aria-label="选择输出目录" title="选择输出目录" onClick={browse} />}
            className="output-directory-input"
          />

          <div className="export-field-label">视频字幕（按当前配音时间同步）</div>
          <Segmented block options={["不烧录", "中文字幕", "中英双语"]} value={subtitle} onChange={setSubtitle} />

          <div className="export-field-label">发布预设</div>
          <Segmented block options={[{ label: "便于分享", value: "share" }, { label: "平衡", value: "balanced" }, { label: "高画质", value: "high" }]} value={exportPreset} onChange={setExportPreset} />

          <div className="export-package refined">
            <h3>默认导出包</h3>
            {[
              [FileVideo, "中文配音视频", "MP4 · H.264 VideoToolbox · AAC · 1080p"],
              [FileText, "字幕文件", "配音同步 SRT · 忠实翻译 SRT · 英文 SRT · 双语 ASS"],
              [FileAudio, "独立中文音轨", "WAV · 48 kHz"],
            ].map(([Icon, title, description]) => {
              const Component = Icon as typeof FileVideo;
              return <div key={String(title)}><Component /><span><strong>{String(title)}</strong><small>{String(description)}</small></span><CheckCircle size={17} weight="fill" /></div>;
            })}
          </div>
          <div className="export-note">预计输出 {estimatedOutput} · 当前预设约 {(presetBitrate / 1_000_000).toFixed(1)} Mbps · 原始文件不会被修改</div>
        </div>
      )}
    </AppModal>
  );
}
