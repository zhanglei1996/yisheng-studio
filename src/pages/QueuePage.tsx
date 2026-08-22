import { useEffect, useMemo, useState } from "react";
import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button, Popconfirm, Progress, Tooltip, message } from "antd";
import { ArrowCounterClockwise, CheckCircle, Clock, Pause, Play, Trash, WarningCircle, X } from "@phosphor-icons/react";
import { jobs as fixtureJobs } from "../fixtures";
import { desktopBridge } from "../bridge";
import type { Job, PersistedJob, Project, ProjectReadiness, ProviderProfile, TtsCatalog } from "../domain";
import { antdIcon } from "../ui/icons";
import { useWorkflowActions } from "../features/workflow/useWorkflowActions";

const PauseIcon = antdIcon(Pause);
const PlayIcon = antdIcon(Play);
const RetryIcon = antdIcon(ArrowCounterClockwise);
const CloseIcon = antdIcon(X);
const TrashIcon = antdIcon(Trash);

const isSafeSeparationFailure = (error?: string | null) => Boolean(error && (
  error.includes("本地人声分离失败")
  || error.includes("人声分离组件未安装")
  || error.includes("本地分离未生成")
));

const stageLabels: Record<string, string> = {
  media_check: "媒体检查",
  proxy: "生成预览代理",
  audio_extract: "音频提取",
  asr: "本地语音识别",
  translation: "上下文翻译",
  script_director: "口播导演",
  tts: "中文配音",
  export: "导出",
};

const toFixtureShape = (job: PersistedJob, projects: Record<string, Project>, providers: Record<string, ProviderProfile>, catalogs: Record<string, TtsCatalog>): Job => {
  const project = projects[job.projectId];
  const providerId = project?.ttsProviderId ?? "system";
  const providerName = providerId === "system" ? "macOS 系统语音" : providers[providerId]?.name ?? "未配置的语音服务";
  const model = providerModel(providers[providerId]);
  const voiceId = project?.ttsVoiceId ?? (providerId === "system" ? "Tingting" : "默认音色");
  const voiceName = catalogs[providerId]?.voices.find((voice) => voice.id === voiceId)?.name ?? voiceId;
  const syncLabel = project?.ttsSyncMode === "semantic" ? "语义旁白" : project?.ttsSyncMode === "narration" ? "连续旁白" : project?.ttsSyncMode === "balanced" ? "平衡连续" : "严格同步";
  const cacheMatch = job.checkpoint?.match(/\|cache:(\d+)\/(\d+)$/);
  const cacheLabel = cacheMatch ? Number(cacheMatch[2]) > 0 && cacheMatch[1] === cacheMatch[2] ? `缓存音轨 ${cacheMatch[1]}/${cacheMatch[2]} · 未重复合成` : `缓存复用 ${cacheMatch[1]}/${cacheMatch[2]}` : null;
  return {
  id: job.id,
  project: project?.name ?? "已删除的项目",
  stage: stageLabels[job.stage] ?? job.stage,
  progress: job.progress,
  status: job.status,
  eta: job.status === "paused" ? "已保存检查点" : job.checkpoint ? `检查点 ${job.checkpoint}` : "等待调度",
  errorMessage: job.errorMessage,
  checkpoint: job.checkpoint,
  projectId: job.projectId,
  synthesisLabel: [providerName, model, voiceName, syncLabel, cacheLabel].filter(Boolean).join(" · "),
  };
};

const readableFailure = (error?: string | null) => {
  if (!error) return null;
  if (isSafeSeparationFailure(error)) return "本地人声分离没有完成。你可以重试，或改用“静音原声”继续；导出视频不会残留英文人声。";
  if (error.includes("Unable to choose an output format")) return "音频临时文件格式异常。此问题已修复，请点击“重试失败片段”。";
  if (error.includes("钥匙串") || error.toLowerCase().includes("keychain")) return "无法读取已保存的服务商凭据。请解锁 macOS 钥匙串后直接重试，无需重新输入密钥。";
  if (error.includes("口播稿不能为空")) return "有片段缺少中文口播稿。请打开编辑器补充内容后重试。";
  return error.replace(/^(provider|media|validation|database) error:\s*/i, "").slice(0, 220);
};

const ttsCheckpointLabel = (checkpoint?: string | null) => {
  const synthesis = checkpoint?.match(/^tts:(chapter|scene)-(\d+)\/(\d+)/);
  if (synthesis) return `已完成 ${synthesis[2]}/${synthesis[3]} 个连续语音块；已落盘结果可断点复用`;
  const fitting = checkpoint?.match(/^tts:compress-(\d+)\/(\d+)/);
  return fitting ? `正在自动优化口播长度 ${fitting[1]}/${fitting[2]}；随后只重生成受影响语音块` : null;
};

const providerModel = (provider?: ProviderProfile) => {
  if (!provider) return null;
  try {
    const config = JSON.parse(provider.publicConfigJson) as { model?: string };
    return config.model || null;
  } catch { return null; }
};

export function QueuePage({ onOpenProject }: { onOpenProject: (projectId: string) => void }) {
  const queryClient = useQueryClient();
  const runWorkflowAction = useWorkflowActions(onOpenProject);
  const [browserJobs, setBrowserJobs] = useState(fixtureJobs);
  const { data: persistedJobs = [] } = useQuery({ queryKey: ["jobs"], queryFn: desktopBridge.listJobs, refetchInterval: desktopBridge.isDesktop() ? 3000 : false });
  const { data: projects = [] } = useQuery({ queryKey: ["projects"], queryFn: desktopBridge.listProjects });
  const readinessQueries = useQueries({ queries: projects.map((project) => ({ queryKey: ["readiness", project.id], queryFn: () => desktopBridge.getProjectReadiness(project.id), staleTime: 3000 })) });
  const readinessByProject = useMemo(() => Object.fromEntries(readinessQueries.flatMap((query, index) => query.data ? [[projects[index].id, query.data as ProjectReadiness]] : [])), [projects, readinessQueries]);
  const segmentQueries = useQueries({ queries: projects.map((project) => ({ queryKey: ["segments", project.id], queryFn: () => desktopBridge.listSegments(project.id), enabled: desktopBridge.isDesktop(), staleTime: 3000 })) });
  const issueCounts = useMemo(() => Object.fromEntries(segmentQueries.map((query, index) => { const segments = query.data ?? []; return [projects[index]?.id, { failed: segments.filter((segment) => segment.ttsState === "failed" && segment.status !== "warning").length, timing: segments.filter((segment) => segment.status === "warning").length }]; })), [projects, segmentQueries]);
  const { data: providers = [] } = useQuery({ queryKey: ["providers"], queryFn: desktopBridge.listProviders });
  const providerIds = useMemo(() => [...new Set(projects.map((project) => project.ttsProviderId ?? "system"))], [projects]);
  const catalogQueries = useQueries({ queries: providerIds.map((providerId) => ({ queryKey: ["tts-catalog", providerId], queryFn: () => desktopBridge.listTtsCatalog(providerId), staleTime: 60_000 })) });
  const projectMap = useMemo(() => Object.fromEntries(projects.map((project) => [project.id, project])), [projects]);
  const providerMap = useMemo(() => Object.fromEntries(providers.map((provider) => [provider.id, provider])), [providers]);
  const catalogMap = useMemo(() => Object.fromEntries(catalogQueries.flatMap((query, index) => query.data ? [[providerIds[index], query.data]] : [])), [catalogQueries, providerIds]);
  const jobs = desktopBridge.isDesktop() ? persistedJobs.map((job) => toFixtureShape(job, projectMap, providerMap, catalogMap)) : browserJobs;

  useEffect(() => {
    if (!desktopBridge.isDesktop()) return;
    let unlisten: () => void = () => undefined;
    desktopBridge.onJobState((job) => {
      queryClient.setQueryData<PersistedJob[]>(["jobs"], (current = []) => current.some((item) => item.id === job.id) ? current.map((item) => item.id === job.id ? job : item) : [...current, job]);
      queryClient.invalidateQueries({ queryKey: ["projects"] });
      queryClient.invalidateQueries({ queryKey: ["segments", job.projectId] });
      queryClient.invalidateQueries({ queryKey: ["readiness", job.projectId] });
    }).then((dispose) => { unlisten = dispose; });
    return () => unlisten();
  }, [queryClient]);

  const act = async (job: Job, action: "toggle" | "retry" | "cancel" | "mute-fallback") => {
    if (!desktopBridge.isDesktop()) {
      setBrowserJobs((current) => action === "cancel" ? current.filter((item) => item.id !== job.id) : current.map((item) => item.id === job.id ? { ...item, status: action === "retry" ? "queued" : item.status === "running" ? "paused" : "running", stage: item.status === "running" ? "已暂停" : item.stage } : item));
      return;
    }
    try {
      const persisted = persistedJobs.find((item) => item.id === job.id);
      if (!persisted) return;
      if (action === "cancel") await runWorkflowAction(persisted, "cancel");
      else if (action === "mute-fallback") {
        await desktopBridge.updateProjectAudioMode(persisted.projectId, "mute");
        message.info("已改用静音原声：会保留画面并移除全部英文原声，现在从断点继续");
        await runWorkflowAction(persisted, "retry");
      }
      else if (action === "retry") await runWorkflowAction(persisted, "retry");
      else if (job.status === "running") await runWorkflowAction(persisted, "pause");
      else await runWorkflowAction(persisted, "continue");
    } catch (error) {
      message.error(String(error));
    }
  };
  const remove = async (job: Job) => {
    try {
      await desktopBridge.deleteJob(job.id);
      await queryClient.invalidateQueries({ queryKey: ["jobs"] });
      message.success("任务记录已删除，项目和生成文件仍然保留");
    } catch (error) { message.error(String(error)); }
  };

  const running = jobs.filter((job) => job.status === "running").length;
  const queued = jobs.filter((job) => ["queued", "paused"].includes(job.status)).length;
  const waiting = jobs.filter((job) => job.status === "waiting_user").length;
  const completed = jobs.filter((job) => job.status === "succeeded").length;

  return <div className="page">
    <section className="page-header"><h1>任务队列</h1></section>
    <section className="queue-summary">
      <div><span>正在运行</span><strong>{running}</strong></div>
      <div><span>等待处理</span><strong>{queued}</strong></div>
      <div><span>需要处理</span><strong className="warning-text">{waiting}</strong></div>
      <div><span>已完成</span><strong>{completed}</strong></div>
    </section>
    <section className="data-panel job-list">{jobs.map((job, index) => {
      const failure = readableFailure(job.errorMessage);
      const separationFailure = isSafeSeparationFailure(job.errorMessage);
      const issues = job.projectId ? issueCounts[job.projectId] : undefined;
      const readiness = job.projectId ? readinessByProject[job.projectId] : undefined;
      const needsEditor = job.status === "waiting_user" && !failure;
      const ttsProgress = ttsCheckpointLabel(job.checkpoint);
      const hasTtsBlockers = Boolean(readiness?.blockingCount && job.stage === "中文配音");
      const actionLabel = hasTtsBlockers
        ? (job.status === "waiting_user" ? "重试未完成配音" : "继续生成中文配音")
        : readiness?.warningCount
          ? `处理 ${readiness.warningCount} 个时长问题`
          : needsEditor ? readiness?.nextAction ?? "打开处理" : "继续";
      const detail = ttsProgress ?? (readiness?.blockingCount
        ? `${readiness.blockingCount} 个配音问题会阻止导出`
        : readiness?.warningCount
          ? `${readiness.warningCount} 个时长问题可自动修复，也可知情导出`
          : issues?.failed
            ? `${issues.failed} 个配音失败会阻止导出`
            : issues?.timing
              ? `${issues.timing} 个时长问题可自动修复，也可知情导出`
              : failure ? "可安全重试，已完成片段不会重复生成" : readiness?.nextAction ?? job.eta);
      return <article className="job-row" key={job.id}>
        <div className={`job-state-icon ${job.status}`}>{job.status === "running" ? <Play weight="fill" /> : job.status === "waiting_user" ? <WarningCircle /> : job.status === "failed" ? <ArrowCounterClockwise /> : <Clock />}</div>
        <div className="job-main">
          <header><div><strong>{job.project}</strong><span>{job.stage}</span><span className="job-synthesis">配音：{job.synthesisLabel}</span></div><em>#{index + 1}</em></header>
          <Progress percent={readiness?.progress ?? job.progress} showInfo={false} size="small" status={job.status === "failed" ? "exception" : job.status === "running" ? "active" : "normal"} />
          {failure && <div className="job-error"><WarningCircle size={15} /><span><strong>中断原因</strong>{failure}</span></div>}
          <footer><span>{readiness?.progress ?? job.progress}%</span><span>{detail}</span></footer>
        </div>
        <div className="job-actions">
          {separationFailure && <Button onClick={() => act(job, "mute-fallback")}>改用静音原声并继续</Button>}
          {job.status === "failed" ? <Button icon={<RetryIcon />} onClick={() => act(job, "retry")}>重试</Button> : !["succeeded", "cancelled"].includes(job.status) && <Button icon={job.status === "running" ? <PauseIcon /> : <PlayIcon />} onClick={() => act(job, "toggle")}>{job.status === "running" ? "暂停" : actionLabel}</Button>}
          {!["succeeded", "cancelled"].includes(job.status) && <Popconfirm title="取消这个任务？" okText="取消任务" cancelText="返回" onConfirm={() => act(job, "cancel")}><Tooltip title="取消任务"><Button type="text" danger icon={<CloseIcon />} aria-label="取消任务" /></Tooltip></Popconfirm>}
          {job.status !== "running" && <Popconfirm title="删除这条任务记录？" description="项目、字幕和生成文件会保留。" okText="删除记录" cancelText="取消" onConfirm={() => remove(job)}><Tooltip title="删除任务记录"><Button type="text" danger icon={<TrashIcon />} aria-label="删除任务记录" /></Tooltip></Popconfirm>}
        </div>
      </article>;
    })}</section>
    {jobs.length === 0 && <div className="queue-empty"><CheckCircle size={28} /><strong>队列为空</strong><span>新建项目后，任务会出现在这里。</span></div>}
    <div className="privacy-note"><CheckCircle size={18} />队列、检查点和中间文件仅保存在这台 Mac；重启后运行中任务会恢复为已暂停。</div>
  </div>;
}
