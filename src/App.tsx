import { useState } from "react";
import { Navigate, Route, Routes, useLocation, useNavigate } from "react-router-dom";
import { Badge, Button, Tooltip, message } from "antd";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Books, CaretDown, CheckCircle, ClockCounterClockwise, GearSix,
  HardDrives, ListBullets, Queue, SidebarSimple, SlidersHorizontal, Warning,
  Sparkle, Translate, VideoCamera, Waveform,
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
import { antdIcon } from "./ui/icons";
import { desktopBridge } from "./bridge";

const navItems = [
  { to: "/projects", label: "项目库", icon: Books },
  { to: "/editor", label: "编辑器", icon: SlidersHorizontal },
  { to: "/glossary", label: "术语库", icon: Translate },
  { to: "/queue", label: "任务队列", icon: Queue },
  { to: "/providers", label: "服务商", icon: Waveform },
  { to: "/settings", label: "设置", icon: GearSix },
];

const SidebarIcon = antdIcon(SidebarSimple, 17);
const ListIcon = antdIcon(ListBullets, 16);

export function App() {
  const location = useLocation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);
  const [onboardingOpen, setOnboardingOpen] = useState(false);
  const [fittingWarnings, setFittingWarnings] = useState(false);
  const [rebuildingTranslation, setRebuildingTranslation] = useState(false);
  const [activeProjectId, setActiveProjectId] = useState<string | null>(null);
  const isEditor = location.pathname === "/editor";
  const { data: persistedJobs = [] } = useQuery({ queryKey: ["jobs"], queryFn: desktopBridge.listJobs, refetchInterval: desktopBridge.isDesktop() ? 3000 : false });
  const { data: appProjects = [] } = useQuery({ queryKey: ["projects"], queryFn: desktopBridge.listProjects });
  const resolvedProjectId = activeProjectId ?? appProjects[0]?.id ?? null;
  const activeProject = appProjects.find((project) => project.id === resolvedProjectId);
  const { data: activeSegments = [] } = useQuery({ queryKey: ["segments", resolvedProjectId], queryFn: () => desktopBridge.listSegments(resolvedProjectId!), enabled: Boolean(resolvedProjectId) && desktopBridge.isDesktop() });
  const activeJobs = desktopBridge.isDesktop() ? persistedJobs.filter((job) => !["succeeded", "cancelled"].includes(job.status)).length : 2;
  const warningCount = activeSegments.filter((segment) => segment.status === "warning").length;
  const processedCount = activeSegments.filter((segment) => !["pending", "warning"].includes(segment.status)).length;
  const pendingCount = activeSegments.length - processedCount;

  return (
    <div className={`app-shell ${sidebarCollapsed ? "sidebar-collapsed" : ""}`}>
      <header className="titlebar" data-tauri-drag-region>
        <Tooltip title={sidebarCollapsed ? "展开侧边栏" : "收起侧边栏"} mouseEnterDelay={0.5}>
          <Button className="titlebar-sidebar" type="text" icon={<SidebarIcon />} aria-label={sidebarCollapsed ? "展开侧边栏" : "收起侧边栏"} onClick={() => setSidebarCollapsed((value) => !value)} />
        </Tooltip>
        <Button className="project-switcher" type="text" onClick={() => navigate("/projects")}>
          <span className="project-mark"><Sparkle weight="fill" size={15} /></span>
          <span>{activeProject?.name ?? "选择项目"}</span><CaretDown size={13} />
        </Button>
        <span className="mode-pill">快速生成 / 先校对</span>
        <div className="titlebar-spacer" />
        <div className="top-status"><span className="status-dot success" />本地处理</div>
        <Button className="queue-button" icon={<ListIcon />} onClick={() => navigate("/queue")}>队列 <Badge count={activeJobs} size="small" /></Button>
      </header>

      <aside className="sidebar">
        <nav className="main-nav" aria-label="主导航">
          <p className="nav-caption">工作空间</p>
          {navItems.map((item) => {
            const active = location.pathname === item.to;
            const Icon = item.icon;
            return (
              <Button type="text" key={item.to} className={`nav-item ${active ? "active" : ""}`} title={item.label} onClick={() => navigate(item.to)}>
                <Icon size={18} weight={active ? "fill" : "regular"} /><span>{item.label}</span>{item.to === "/queue" && activeJobs > 0 && <em>{activeJobs}</em>}
              </Button>
            );
          })}
        </nav>

        {!sidebarCollapsed && isEditor && (
          <section className="project-health">
            <div className="section-heading"><span>项目风险</span>{warningCount > 0 && <span className="count-badge warning">{warningCount}</span>}</div>
            {warningCount > 0 ? <Button type="text" className="risk-row" loading={fittingWarnings} onClick={async () => {
              const job = persistedJobs.find((item) => item.projectId === resolvedProjectId);
              if (!resolvedProjectId || !job) return;
              setFittingWarnings(true);
              try {
                const remaining = await desktopBridge.fitTtsWarnings(resolvedProjectId, job.id);
                await Promise.all([queryClient.invalidateQueries({ queryKey: ["segments", resolvedProjectId] }), queryClient.invalidateQueries({ queryKey: ["jobs"] }), queryClient.invalidateQueries({ queryKey: ["projects"] })]);
                if (remaining.length) message.warning(`自动压缩后仍有 ${remaining.length} 个片段需要手工调整`); else message.success("超时片段已全部自动适配");
              } catch (error) { message.error(String(error)); }
              finally { setFittingWarnings(false); }
            }}><Warning className="warning-text" size={18} /><span><strong>{fittingWarnings ? "正在压缩配音文案" : "配音时长需处理"}</strong><small>{warningCount} 个片段 · 点击自动适配</small></span></Button> : <div className="risk-row risk-clear"><CheckCircle size={18} /><span><strong>当前无阻断风险</strong><small>{activeSegments.length ? "识别片段均可继续处理" : "处理后将在这里显示风险"}</small></span></div>}
            {activeSegments.length > 0 && <Button type="link" size="small" loading={rebuildingTranslation} onClick={async () => {
              const job = persistedJobs.find((item) => item.projectId === resolvedProjectId);
              if (!resolvedProjectId || !job) return;
              setRebuildingTranslation(true);
              try {
                await desktopBridge.rebuildTranslation(resolvedProjectId, job.id);
                await Promise.all([queryClient.invalidateQueries({ queryKey: ["segments", resolvedProjectId] }), queryClient.invalidateQueries({ queryKey: ["jobs"] }), queryClient.invalidateQueries({ queryKey: ["projects"] })]);
                message.success("字幕已按原始片段边界重新翻译");
              } catch (error) { message.error(String(error)); }
              finally { setRebuildingTranslation(false); }
            }}>重新校准全部翻译</Button>}
            <div className="project-mini-stats">
              <span><CheckCircle size={14} /> 已处理 {processedCount}</span>
              <span><ClockCounterClockwise size={14} /> 待处理 {pendingCount}</span>
            </div>
          </section>
        )}

        <div className="sidebar-bottom">
          {!sidebarCollapsed && <><Button type="text" className="storage-row" onClick={() => navigate("/settings")}><HardDrives size={16} /><span><strong>本地存储</strong><small>1.23 TB 可用</small></span><span className="status-dot success" /></Button><p>已保存&nbsp; 10:42</p></>}
        </div>
      </aside>

      <main className={`workspace ${isEditor ? "editor-workspace" : ""}`}>
        <Routes>
          <Route path="/" element={<Navigate to="/projects" replace />} />
          <Route path="/projects" element={<LibraryPage onCreate={() => setCreateOpen(true)} onOpen={(projectId) => { setActiveProjectId(projectId); navigate("/editor"); }} />} />
          <Route path="/editor" element={<EditorPage projectId={resolvedProjectId} onExport={() => setExportOpen(true)} onRegenerate={async () => {
            const job = persistedJobs.find((item) => item.projectId === resolvedProjectId);
            if (!resolvedProjectId || !job) return;
            try {
              message.loading({ content: "正在重新生成中文配音…", key: "segment-tts", duration: 0 });
              const warnings = await desktopBridge.runTts(resolvedProjectId, job.id);
              await Promise.all([queryClient.invalidateQueries({ queryKey: ["segments", resolvedProjectId] }), queryClient.invalidateQueries({ queryKey: ["jobs"] }), queryClient.invalidateQueries({ queryKey: ["projects"] })]);
              if (warnings.length) message.warning({ content: `重新配音完成，仍有 ${warnings.length} 个片段需要调整`, key: "segment-tts" });
              else message.success({ content: "中文配音已重新生成并通过时长校验", key: "segment-tts" });
            } catch (error) { message.error({ content: String(error), key: "segment-tts" }); }
          }} />} />
          <Route path="/glossary" element={<GlossaryPage />} />
          <Route path="/queue" element={<QueuePage onOpenProject={(projectId) => { setActiveProjectId(projectId); navigate("/editor"); }} />} />
          <Route path="/providers" element={<ProvidersPage />} />
          <Route path="/settings" element={<SettingsPage onOnboarding={() => setOnboardingOpen(true)} />} />
        </Routes>
      </main>

      <CreateProjectDialog open={createOpen} onClose={() => setCreateOpen(false)} onComplete={async (options) => {
        try {
          const project = await desktopBridge.createProjectFromMedia(options);
          if (project) setActiveProjectId(project.id);
          const job = project ? await desktopBridge.enqueueJob(project.id) : null;
          setCreateOpen(false);
          navigate(desktopBridge.isDesktop() ? "/queue" : "/editor");
          await Promise.all([queryClient.invalidateQueries({ queryKey: ["projects"] }), queryClient.invalidateQueries({ queryKey: ["jobs"] })]);
          if (project && job) {
            void desktopBridge.prepareMedia(project.id, job.id).then(async () => {
              await Promise.all([queryClient.invalidateQueries({ queryKey: ["projects"] }), queryClient.invalidateQueries({ queryKey: ["jobs"] })]);
              message.success("媒体准备完成，已生成预览代理和识别音频");
            }).catch((error) => message.error(String(error)));
          }
        } catch (error) {
          message.error(String(error));
        }
      }} />
      <ExportDialog open={exportOpen} onClose={() => setExportOpen(false)} projectId={resolvedProjectId} />
      <OnboardingDialog open={onboardingOpen} onClose={() => setOnboardingOpen(false)} />
    </div>
  );
}
