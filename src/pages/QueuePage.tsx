import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Button, Popconfirm, Progress, Tooltip, message } from "antd";
import { ArrowCounterClockwise, CheckCircle, Clock, Pause, Play, WarningCircle, X } from "@phosphor-icons/react";
import { jobs as fixtureJobs } from "../fixtures";
import { desktopBridge } from "../bridge";
import type { Job, PersistedJob } from "../domain";
import { antdIcon } from "../ui/icons";

const PauseIcon = antdIcon(Pause);
const PlayIcon = antdIcon(Play);
const RetryIcon = antdIcon(ArrowCounterClockwise);
const CloseIcon = antdIcon(X);

const stageLabels: Record<string, string> = {
  media_check: "媒体检查",
  proxy: "生成预览代理",
  audio_extract: "音频提取",
  asr: "本地语音识别",
  translation: "上下文翻译",
  tts: "中文配音",
  export: "导出",
};

const toFixtureShape = (job: PersistedJob, projects: Record<string, string>): Job => ({
  id: job.id,
  project: projects[job.projectId] ?? "本地视频项目",
  stage: stageLabels[job.stage] ?? job.stage,
  progress: job.progress,
  status: job.status,
  eta: job.status === "paused" ? "已保存检查点" : job.checkpoint ? `检查点 ${job.checkpoint}` : "等待调度",
});

export function QueuePage({ onOpenProject }: { onOpenProject: (projectId: string) => void }) {
  const queryClient = useQueryClient();
  const [browserJobs, setBrowserJobs] = useState(fixtureJobs);
  const { data: persistedJobs = [], isFetching } = useQuery({ queryKey: ["jobs"], queryFn: desktopBridge.listJobs, refetchInterval: desktopBridge.isDesktop() ? 3000 : false });
  const { data: projects = [] } = useQuery({ queryKey: ["projects"], queryFn: desktopBridge.listProjects });
  const projectNames = useMemo(() => Object.fromEntries(projects.map((project) => [project.id, project.name])), [projects]);
  const jobs = desktopBridge.isDesktop() ? persistedJobs.map((job) => toFixtureShape(job, projectNames)) : browserJobs;

  useEffect(() => {
    if (!desktopBridge.isDesktop()) return;
    let unlisten: () => void = () => undefined;
    desktopBridge.onJobState((job) => {
      queryClient.setQueryData<PersistedJob[]>(["jobs"], (current = []) => current.some((item) => item.id === job.id) ? current.map((item) => item.id === job.id ? job : item) : [...current, job]);
      queryClient.invalidateQueries({ queryKey: ["projects"] });
      if (["tts", "export"].includes(job.stage)) queryClient.invalidateQueries({ queryKey: ["segments", job.projectId] });
    }).then((dispose) => { unlisten = dispose; });
    return () => unlisten();
  }, [queryClient]);

  const act = async (job: Job, action: "toggle" | "retry" | "cancel") => {
    if (!desktopBridge.isDesktop()) {
      setBrowserJobs((current) => action === "cancel" ? current.filter((item) => item.id !== job.id) : current.map((item) => item.id === job.id ? { ...item, status: action === "retry" ? "queued" : item.status === "running" ? "paused" : "running", stage: item.status === "running" ? "已暂停" : item.stage } : item));
      return;
    }
    try {
      if (action === "cancel") await desktopBridge.cancelJob(job.id);
      else if (action === "retry") {
        await desktopBridge.retryJob(job.id);
        await runPersistedStage(persistedJobs.find((item) => item.id === job.id));
      }
      else if (job.status === "running") await desktopBridge.pauseJob(job.id);
      else await runPersistedStage(persistedJobs.find((item) => item.id === job.id));
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["jobs"] }),
        queryClient.invalidateQueries({ queryKey: ["segments", persistedJobs.find((item) => item.id === job.id)?.projectId] }),
      ]);
    } catch (error) {
      message.error(String(error));
    }
  };

  const runPersistedStage = async (persisted: PersistedJob | undefined) => {
    if (!persisted) return;
    if (["media_check", "audio_extract", "proxy"].includes(persisted.stage)) await desktopBridge.prepareMedia(persisted.projectId, persisted.id);
    else if (persisted.stage === "asr") await desktopBridge.runAsr(persisted.projectId, persisted.id);
    else if (["glossary", "translation"].includes(persisted.stage)) await desktopBridge.runTranslation(persisted.projectId, persisted.id);
    else if (persisted.stage === "tts") await desktopBridge.runTts(persisted.projectId, persisted.id);
    else if (persisted.stage === "export") { onOpenProject(persisted.projectId); message.info("项目已准备好，请点击编辑器右上角“导出”"); }
    else await desktopBridge.resumeJob(persisted.id);
  };

  const running = jobs.filter((job) => job.status === "running").length;
  const queued = jobs.filter((job) => ["queued", "paused"].includes(job.status)).length;
  const waiting = jobs.filter((job) => job.status === "waiting_user").length;
  const completed = jobs.filter((job) => job.status === "succeeded").length;

  return <div className="page"><section className="page-header"><div><span className="eyebrow">本地任务调度</span><h1>任务队列</h1><p>同一时间运行一个重型项目，任务状态和检查点会持久化到 SQLite。</p></div><div className="top-status panel-status"><span className="status-dot success" />{isFetching ? "正在同步" : "本地 Worker 正常"}</div></section>
    <section className="queue-summary"><div><span>正在运行</span><strong>{running}</strong></div><div><span>等待处理</span><strong>{queued}</strong></div><div><span>等待你确认</span><strong className="warning-text">{waiting}</strong></div><div><span>已完成</span><strong>{completed}</strong></div></section>
    <section className="data-panel job-list">{jobs.map((job, index) => <article className="job-row" key={job.id}><div className={`job-state-icon ${job.status}`}>{job.status === "running" ? <Play weight="fill" /> : job.status === "waiting_user" ? <WarningCircle /> : job.status === "failed" ? <ArrowCounterClockwise /> : <Clock />}</div><div className="job-main"><header><div><strong>{job.project}</strong><span>{job.stage}</span></div><em>#{index + 1}</em></header><Progress percent={job.progress} showInfo={false} size="small" status={job.status === "failed" ? "exception" : job.status === "running" ? "active" : "normal"} /><footer><span>{job.progress}%</span><span>{job.eta}</span></footer></div><div className="job-actions">{job.status === "failed" ? <Tooltip title="重试"><Button type="text" icon={<RetryIcon />} aria-label="重试" onClick={() => act(job, "retry")} /></Tooltip> : !["succeeded", "cancelled"].includes(job.status) && <Tooltip title={job.status === "running" ? "暂停" : job.status === "waiting_user" ? "确认并继续" : "继续"}><Button type="text" icon={job.status === "running" ? <PauseIcon /> : <PlayIcon />} aria-label={job.status === "running" ? "暂停" : "继续"} onClick={() => act(job, "toggle")} /></Tooltip>}<Popconfirm title="取消这个任务？" okText="取消任务" cancelText="返回" onConfirm={() => act(job, "cancel")}><Tooltip title="取消任务"><Button type="text" danger icon={<CloseIcon />} aria-label="取消任务" disabled={["succeeded", "cancelled"].includes(job.status)} /></Tooltip></Popconfirm></div></article>)}</section>
    {jobs.length === 0 && <div className="queue-empty"><CheckCircle size={28} /><strong>队列为空</strong><span>新建项目后，任务会出现在这里。</span></div>}
    <div className="privacy-note"><CheckCircle size={18} />队列、检查点和中间文件仅保存在这台 Mac；重启后运行中任务会恢复为已暂停。</div>
  </div>;
}
