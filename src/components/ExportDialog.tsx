import { useEffect, useState } from "react";
import { Button, Input, Modal, Segmented } from "antd";
import { CheckCircleFilled, FolderOpenOutlined, FolderOutlined } from "@ant-design/icons";
import { Export, FileAudio, FileText, FileVideo } from "@phosphor-icons/react";
import { useQuery } from "@tanstack/react-query";
import { desktopBridge } from "../bridge";
import { message } from "antd";

const DEFAULT_OUTPUT = "~/Movies/译声工坊";

async function chooseDirectory(current: string) {
  if (!(window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__) return current;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({ directory: true, multiple: false, defaultPath: current });
  return typeof selected === "string" ? selected : current;
}

export function ExportDialog({ open, onClose, projectId }: { open: boolean; onClose: () => void; projectId: string | null }) {
  const [subtitle, setSubtitle] = useState("中文字幕");
  const [outputDirectory, setOutputDirectory] = useState(DEFAULT_OUTPUT);
  const [exporting, setExporting] = useState(false);
  const [done, setDone] = useState(false);
  const [resultDirectory, setResultDirectory] = useState<string | null>(null);
  const { data: jobs = [] } = useQuery({ queryKey: ["jobs"], queryFn: desktopBridge.listJobs, enabled: open });

  useEffect(() => {
    if (!open) window.setTimeout(() => setDone(false), 160);
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
      const result = await desktopBridge.startExport(projectId, job.id, outputDirectory, mode);
      setResultDirectory(result?.directory ?? outputDirectory); setDone(true);
    } catch (error) { message.error(String(error)); }
    finally { setExporting(false); }
  };

  const browse = async () => setOutputDirectory(await chooseDirectory(outputDirectory));

  return (
    <Modal
      open={open}
      onCancel={onClose}
      width={620}
      centered
      destroyOnHidden
      title={<div className="modal-heading"><span className="eyebrow">本地导出</span><strong>导出中文版本</strong></div>}
      footer={done
        ? <Button type="primary" onClick={onClose}>完成</Button>
        : <><Button onClick={onClose}>取消</Button><Button type="primary" icon={<Export />} loading={exporting} onClick={start}>{exporting ? "正在准备导出" : "开始导出"}</Button></>}
      className="export-modal"
    >
      {done ? (
        <div className="export-success antd-success">
          <CheckCircleFilled />
          <h3>导出完成</h3>
          <p>视频、三种字幕和中文音轨已保存。</p>
          <Button icon={<FolderOpenOutlined />} onClick={() => resultDirectory && desktopBridge.revealInFinder(resultDirectory).catch(() => message.info(resultDirectory))}>查看保存位置</Button>
        </div>
      ) : (
        <div className="export-form">
          <label className="export-field-label" htmlFor="export-directory">输出目录</label>
          <Input
            id="export-directory"
            size="large"
            value={outputDirectory}
            readOnly
            prefix={<FolderOutlined />}
            suffix={<Button type="text" size="small" icon={<FolderOpenOutlined />} aria-label="选择输出目录" title="选择输出目录" onClick={browse} />}
            className="output-directory-input"
          />

          <div className="export-field-label">视频字幕</div>
          <Segmented block options={["不烧录", "中文字幕", "中英双语"]} value={subtitle} onChange={setSubtitle} />

          <div className="export-package refined">
            <h3>默认导出包</h3>
            {[
              [FileVideo, "中文配音视频", "MP4 · H.264 VideoToolbox · AAC · 1080p"],
              [FileText, "字幕文件", "中文 SRT · 英文 SRT · 双语 ASS"],
              [FileAudio, "独立中文音轨", "WAV · 48 kHz"],
            ].map(([Icon, title, description]) => {
              const Component = Icon as typeof FileVideo;
              return <div key={String(title)}><Component /><span><strong>{String(title)}</strong><small>{String(description)}</small></span><CheckCircleFilled /></div>;
            })}
          </div>
          <div className="export-note">预计输出 1.36 GB · 不放大源视频 · 原始文件不会被修改</div>
        </div>
      )}
    </Modal>
  );
}
