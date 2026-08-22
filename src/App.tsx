import { startTransition, useCallback, useEffect, useState } from "react";
import { Navigate, Route, Routes, useLocation, useNavigate } from "react-router-dom";
import { Button, Popconfirm, message } from "antd";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Books, CheckCircle, ClockCounterClockwise, GearSix,
  HardDrives, Queue, SlidersHorizontal, Warning, Translate, Waveform,
} from "@phosphor-icons/react";
import { EditorPage } from "./components/EditorPage";
import { LibraryPage } from "./pages/LibraryPage";
import { GlossaryPage } from "./pages/GlossaryPage";
import { QueuePage } from "./pages/QueuePage";
import { ProvidersPage } from "./pages/ProvidersPage";
import { SettingsPage } from "./pages/SettingsPage";
import { CreateProjectDialog } from "./components/CreateProjectDialog";
import { ExportDialog } from "./components/ExportDialog";
import { OnboardingDialog } from "./components/OnboardingDialog";
import { desktopBridge } from "./bridge";
import { markNavigationStart } from "./performance";
import { useEditorStore } from "./store";
import type { EditorSaveState, ProjectReadiness, TtsFitProgress, TtsFitResult } from "./domain";

const navItems = [
  { to: "/projects", label: "项目库", icon: Books },
  { to: "/editor", label: "编辑器", icon: SlidersHorizontal },
  { to: "/glossary", label: "术语库", icon: Translate },
  { to: "/queue", label: "任务队列", icon: Queue },
  { to: "/providers", label: "服务商", icon: Waveform },
  { to: "/settings", label: "设置", icon: GearSix },
];

export function App() {
  const location = useLocation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [createOpen, setCreateOpen] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);
  const [onboardingOpen, setOnboardingOpen] = useState(false);
  const [fittingWarnings, setFittingWarnings] = useState(false);
  const [fitProgress, setFitProgress] = useState<TtsFitProgress | null>(null);
  const [fitResult, setFitResult] = useState<TtsFitResult | null>(null);
  const [saveState, setSaveState] = useState<EditorSaveState>({ status: "idle" });
  const [rebuildingTranslation, setRebuildingTranslation] = useState(false);
  const [activeProjectId, setActiveProjectId] = useState<string | null>(null);
  const isEditor = location.pathname === "/editor";
  const [editorVisited, setEditorVisited] = useState(isEditor);
  const navigateResponsive = useCallback((path: string) => {
    markNavigationStart(path);
    startTransition(() => navigate(path));
  }, [navigate]);
  const { data: persistedJobs = [] } = useQuery({ queryKey: ["jobs"], queryFn: desktopBridge.listJobs, refetchInterval: desktopBridge.isDesktop() ? 3000 : false });
  const { data: appProjects = [] } = useQuery({ queryKey: ["projects"], queryFn: desktopBridge.listProjects });
  const resolvedProjectId = activeProjectId && appProjects.some((project) => project.id === activeProjectId) ? activeProjectId : appProjects[0]?.id ?? null;
  const activeProject = appProjects.find((project) => project.id === resolvedProjectId);
  const { data: activeSegments = [] } = useQuery({ queryKey: ["segments", resolvedProjectId], queryFn: () => desktopBridge.listSegments(resolvedProjectId!), enabled: isEditor && Boolean(resolvedProjectId) && desktopBridge.isDesktop(), staleTime: 60_000 });
  const { data: readiness = null } = useQuery<ProjectReadiness | null>({ queryKey: ["readiness", resolvedProjectId], queryFn: () => desktopBridge.getProjectReadiness(resolvedProjectId!), enabled: Boolean(resolvedProjectId), refetchInterval: isEditor && desktopBridge.isDesktop() ? 3000 : false });
  const { data: activePreflight = null } = useQuery({ queryKey: ["export-preflight", resolvedProjectId], queryFn: () => desktopBridge.getExportPreflight(resolvedProjectId!), enabled: isEditor && Boolean(resolvedProjectId) && desktopBridge.isDesktop(), staleTime: 3000 });
  const activeJobs = desktopBridge.isDesktop() ? persistedJobs.filter((job) => !["succeeded", "cancelled"].includes(job.status)).length : 2;
  const activeProjectJob = persistedJobs.find((job) => job.projectId === resolvedProjectId && job.status === "running");
  const blockingSegmentIds = new Set(activePreflight?.blockingSegmentIds ?? []);
  const failedTtsSegments = activeSegments.filter((segment) => blockingSegmentIds.has(segment.id));
  const timingWarningSegments = activeSegments.filter((segment) => segment.status === "warning" && segment.ttsState !== "failed");
  const warningCount = failedTtsSegments.length + timingWarningSegments.length;
  const processedCount = activeSegments.filter((segment) => !["pending", "warning"].includes(segment.status)).length;
  const pendingCount = activeSegments.length - processedCount;
  useEffect(() => {
    if (isEditor) setEditorVisited(true);
  }, [isEditor]);
  useEffect(() => {
    setFitResult(null);
    setFitProgress(null);
  }, [resolvedProjectId]);
  useEffect(() => {
    let unlisten: () => void = () => undefined;
    desktopBridge.onTtsFitProgress((progress) => {
      if (progress.projectId === resolvedProjectId) setFitProgress(progress);
    }).then((dispose) => { unlisten = dispose; });
    return () => unlisten();
  }, [resolvedProjectId]);
  const exportFromEditor = useCallback(() => setExportOpen(true), []);
  const regenerateFromEditor = useCallback(async (segmentId?: string) => {
    const job = persistedJobs.find((item) => item.projectId === resolvedProjectId);
    if (!resolvedProjectId || !job) return;
    try {
      message.loading({ content: "正在重新生成中文配音…", key: "segment-tts", duration: 0 });
      const result = !segmentId && activeProject?.ttsSyncMode === "semantic"
        ? await desktopBridge.runSemanticNarration(resolvedProjectId, job.id)
        : await desktopBridge.runTts(resolvedProjectId, job.id, segmentId ? [segmentId] : undefined);
      if (result.previewMedia) {
        queryClient.setQueriesData(
          { queryKey: ["preview-media", resolvedProjectId] },
          result.previewMedia,
        );
      }
      await Promise.all([queryClient.invalidateQueries({ queryKey: ["segments", resolvedProjectId] }), queryClient.invalidateQueries({ queryKey: ["jobs"] }), queryClient.invalidateQueries({ queryKey: ["projects"] }), queryClient.invalidateQueries({ queryKey: ["readiness", resolvedProjectId] }), queryClient.invalidateQueries({ queryKey: ["export-preflight", resolvedProjectId] })]);
      if (result.failedSegments.length) message.error({ content: `${segmentId ? "当前语音块" : "整片"}生成失败：${result.failedSegments[0]?.message ?? "请重试"}`, key: "segment-tts" });
      else if (result.warningIds.length) message.warning({ content: `重新配音完成，仍有 ${result.warningIds.length} 个片段需要调整`, key: "segment-tts" });
      else if (segmentId && result.affectedSegmentIds.length > 1) message.success({ content: `当前语音块已重新生成，共 ${result.affectedSegmentIds.length} 条字幕`, key: "segment-tts" });
      else if (!segmentId && result.synthesisUnitCount > 0 && result.synthesisUnitCount < activeSegments.length) message.success({ content: `已按 ${result.synthesisUnitCount} 个连续语音块生成整片配音，时长校验通过`, key: "segment-tts" });
      else message.success({ content: "中文配音已重新生成并通过时长校验", key: "segment-tts" });
    } catch (error) { message.error({ content: String(error), key: "segment-tts" }); }
  }, [activeProject?.ttsSyncMode, activeSegments.length, persistedJobs, queryClient, resolvedProjectId]);
  const locateIssue = useCallback((kind: "failed" | "timing") => {
    const issues = kind === "failed" ? failedTtsSegments : timingWarningSegments;
    if (!issues.length) return;
    const editor = useEditorStore.getState();
    const currentIndex = issues.findIndex((segment) => segment.id === editor.selectedId);
    const next = issues[(currentIndex + 1) % issues.length];
    editor.selectSegment(next.id);
    editor.setInspectorTab(kind === "failed" ? "voice" : "align");
    if (!isEditor) navigateResponsive("/editor");
    message.info(`已定位到第 ${issues.indexOf(next) + 1}/${issues.length} 个${kind === "failed" ? "生成失败" : "时长待调整"}片段`);
  }, [failedTtsSegments, isEditor, navigateResponsive, timingWarningSegments]);
  const fitWarnings = useCallback(async (segmentIds?: string[]) => {
    const job = persistedJobs.find((item) => item.projectId === resolvedProjectId);
    if (!resolvedProjectId || !job) return null;
    setFitResult(null);
    setFittingWarnings(true);
    setFitProgress({ projectId: resolvedProjectId, stage: "compressing", completed: 0, total: segmentIds?.length ?? timingWarningSegments.length, progress: 0 });
    try {
      const result = await desktopBridge.fitTtsWarnings(resolvedProjectId, job.id, segmentIds);
      setFitResult(result);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["segments", resolvedProjectId] }),
        queryClient.invalidateQueries({ queryKey: ["jobs"] }),
        queryClient.invalidateQueries({ queryKey: ["projects"] }),
        queryClient.invalidateQueries({ queryKey: ["readiness", resolvedProjectId] }),
        queryClient.invalidateQueries({ queryKey: ["export-preflight", resolvedProjectId] }),
      ]);
      if (result.remainingIds.length) {
        const editor = useEditorStore.getState();
        editor.selectSegment(result.remainingIds[0]);
        editor.setInspectorTab("align");
      } else if (segmentIds?.length === 1) {
        const next = timingWarningSegments.find((segment) => segment.id !== segmentIds[0]);
        if (next) {
          const editor = useEditorStore.getState();
          editor.selectSegment(next.id);
          editor.setInspectorTab("align");
        }
      }
      return result;
    } catch (error) { message.error(String(error)); return null; }
    finally { setFittingWarnings(false); }
  }, [persistedJobs, queryClient, resolvedProjectId, timingWarningSegments.length]);
  const undoFitWarnings = useCallback(async () => {
    if (!resolvedProjectId) return;
    try {
      const restoredIds = await desktopBridge.undoTtsFit(resolvedProjectId);
      setFitResult(null);
      setFitProgress(null);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["segments", resolvedProjectId] }),
        queryClient.invalidateQueries({ queryKey: ["readiness", resolvedProjectId] }),
      ]);
      message.success(`已撤销 ${restoredIds.length} 个片段的自动缩短；相关配音已标记为需要重新生成`);
    } catch (error) { message.error(String(error)); }
  }, [queryClient, resolvedProjectId]);
  return (
    <div className="app-shell">
      <header className="window-drag-strip" data-tauri-drag-region aria-hidden="true" />
      <aside className="sidebar">
        <nav className="main-nav" aria-label="主导航">
          <p className="nav-caption">工作空间</p>
          {navItems.map((item) => {
            const active = location.pathname === item.to;
            const Icon = item.icon;
            return (
              <Button type="text" key={item.to} className={`nav-item ${active ? "active" : ""}`} title={item.label} onClick={() => navigateResponsive(item.to)}>
                <Icon size={18} weight={active ? "fill" : "regular"} /><span>{item.label}</span>{item.to === "/queue" && activeJobs > 0 && <em>{activeJobs}</em>}
              </Button>
            );
          })}
        </nav>

        {isEditor && resolvedProjectId && (
          <section className="project-health">
            <div className="section-heading"><span>项目状态</span>{activeProject?.workflowMode !== "quick" && warningCount > 0 && <span className="count-badge warning">{warningCount}</span>}</div>
            {activeProject?.workflowMode === "quick" && <div className="risk-row risk-clear"><CheckCircle size={18} /><span><strong>{activeProjectJob || fittingWarnings ? "正在后台自动处理" : readiness?.canExport ? "项目可直接导出" : "自动生成已接管"}</strong><small>{warningCount ? `剩余 ${warningCount} 项正在自动修复；详情可到任务队列查看` : "快速模式不会要求逐项确认"}</small></span></div>}
            {activeProject?.workflowMode !== "quick" && failedTtsSegments.length > 0 && <Button type="text" className="risk-row" onClick={() => locateIssue("failed")}><Warning className="danger-text" size={18} /><span><strong>在线配音失败</strong><small>{failedTtsSegments.length} 个片段 · 点击逐个定位并查看原因</small></span></Button>}
            {activeProject?.workflowMode !== "quick" && (timingWarningSegments.length > 0 ? <div className="risk-block"><Button type="text" className="risk-row" onClick={() => locateIssue("timing")}><Warning className="warning-text" size={18} /><span><strong>{fittingWarnings ? "正在自动修复时长" : "时长适配提醒"}</strong><small>{timingWarningSegments.length} 个片段 · 点击逐个检查</small></span></Button></div> : failedTtsSegments.length === 0 && <div className="risk-row risk-clear"><CheckCircle size={18} /><span><strong>{readiness?.canExport ? "项目可导出" : "当前无阻断风险"}</strong><small>{activeSegments.length ? readiness?.nextAction ?? "识别片段均可继续处理" : "处理后将在这里显示状态"}</small></span></div>)}
            {activeSegments.length > 0 && <Popconfirm title="重新翻译全部字幕？" description="会将原文文本发送给当前翻译服务商并覆盖现有译文；不会上传原视频或原声。" okText="开始重新翻译" cancelText="取消" onConfirm={async () => {
              const job = persistedJobs.find((item) => item.projectId === resolvedProjectId);
              if (!resolvedProjectId || !job) return;
              setRebuildingTranslation(true);
              try {
                await desktopBridge.rebuildTranslation(resolvedProjectId, job.id);
                await Promise.all([queryClient.invalidateQueries({ queryKey: ["segments", resolvedProjectId] }), queryClient.invalidateQueries({ queryKey: ["jobs"] }), queryClient.invalidateQueries({ queryKey: ["projects"] })]);
                message.success("字幕已按原始片段边界重新翻译");
              } catch (error) { message.error(String(error)); }
              finally { setRebuildingTranslation(false); }
            }}><Button type="link" size="small" loading={rebuildingTranslation}>重新翻译全部字幕</Button></Popconfirm>}
            <div className="project-mini-stats">
              <span><CheckCircle size={14} /> 已处理 {processedCount}</span>
              <span><ClockCounterClockwise size={14} /> 待处理 {pendingCount}</span>
            </div>
          </section>
        )}

        <div className="sidebar-bottom">
          <Button type="text" className="storage-row" onClick={() => navigateResponsive("/settings")}><HardDrives size={16} /><span><strong>本地存储</strong><small>1.23 TB 可用</small></span><span className="status-dot success" /></Button><p className={`save-state ${saveState.status}`} aria-live="polite">{saveState.status === "saving" ? "正在保存…" : saveState.status === "error" ? "保存失败" : saveState.savedAt ? `已保存 ${saveState.savedAt}` : "更改会自动保存"}</p>
        </div>
      </aside>

      <main className={`workspace ${isEditor ? "editor-workspace" : ""}`}>
        {(isEditor || editorVisited) && <div className={`editor-route-cache ${isEditor ? "active" : "inactive"}`} aria-hidden={!isEditor}>
          <EditorPage active={isEditor} projectId={resolvedProjectId} readiness={readiness} activeJob={activeProjectJob} fitProgress={fitProgress} fitResult={fitResult} fittingWarnings={fittingWarnings} onSaveStateChange={setSaveState} onFitWarnings={fitWarnings} onUndoFit={undoFitWarnings} onCreate={() => setCreateOpen(true)} onExport={exportFromEditor} onRegenerate={(segmentId) => regenerateFromEditor(segmentId)} onRegenerateAll={() => regenerateFromEditor()} />
        </div>}
        {!isEditor && <Routes>
          <Route path="/" element={<Navigate to="/projects" replace />} />
          <Route path="/projects" element={<LibraryPage onCreate={() => setCreateOpen(true)} onOpen={(projectId) => { setActiveProjectId(projectId); navigateResponsive("/editor"); }} />} />
          <Route path="/glossary" element={<GlossaryPage />} />
          <Route path="/queue" element={<QueuePage onOpenProject={(projectId) => { setActiveProjectId(projectId); navigateResponsive("/editor"); }} />} />
          <Route path="/providers" element={<ProvidersPage />} />
          <Route path="/settings" element={<SettingsPage onOnboarding={() => setOnboardingOpen(true)} />} />
        </Routes>}
      </main>

      <CreateProjectDialog open={createOpen} onClose={() => setCreateOpen(false)} onComplete={async (options) => {
        try {
          const project = await desktopBridge.createProjectFromMedia(options);
          if (project) setActiveProjectId(project.id);
          const workflow = project ? await desktopBridge.enqueueWorkflow(project.id) : null;
          setCreateOpen(false);
          navigate(desktopBridge.isDesktop() ? "/queue" : "/editor");
          await Promise.all([queryClient.invalidateQueries({ queryKey: ["projects"] }), queryClient.invalidateQueries({ queryKey: ["jobs"] })]);
          if (project && workflow) {
            void (async () => {
              const toastKey = `new-project-${project.id}`;
              try {
                message.loading({ key: toastKey, content: options.audioMode === "separate" ? "工作流正在本地分离人声并处理视频…" : "工作流正在处理视频、识别与中文配音…", duration: 0 });
                const result = await desktopBridge.startWorkflow(workflow.job.id);
                if (result?.nextAction === "open_editor") {
                  message.info({ key: toastKey, content: result.currentNodeId === "export_publish" ? "中文配音已准备好，可以预览并导出" : "工作流已保存检查点，请打开编辑器确认后继续" });
                } else if (result?.nextAction === "retry") {
                  message.warning({ key: toastKey, content: "工作流已暂停并保留检查点，可在任务队列安全重试" });
                } else {
                  message.success({ key: toastKey, content: "工作流处理完成" });
                }
              } catch (error) {
                message.error({ key: toastKey, content: `生成中断：${String(error)}`, duration: 8 });
              } finally {
                await Promise.all([queryClient.invalidateQueries({ queryKey: ["projects"] }), queryClient.invalidateQueries({ queryKey: ["jobs"] })]);
              }
            })();
          }
        } catch (error) {
          message.error(String(error));
        }
      }} />
      <ExportDialog open={exportOpen} onClose={() => setExportOpen(false)} onResolveIssues={(kind) => { setExportOpen(false); locateIssue(kind); }} projectId={resolvedProjectId} />
      <OnboardingDialog open={onboardingOpen} onClose={() => setOnboardingOpen(false)} />
    </div>
  );
}
