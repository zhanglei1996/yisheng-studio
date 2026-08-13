import { useMemo, useState } from "react";
import { Button, Dropdown, Empty, Input, Progress, Segmented, message } from "antd";
import { CheckCircle, Clock, DotsThree, FileVideo, PencilSimple, Plus, Trash, WarningCircle } from "@phosphor-icons/react";
import { projects } from "../fixtures";
import { readinessLabel, statusLabel, type ProjectReadiness } from "../domain";
import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { desktopBridge } from "../bridge";
import { antdIcon } from "../ui/icons";
import { AppModal } from "../components/AppModal";

const PlusIcon = antdIcon(Plus, 17);
const MoreIcon = antdIcon(DotsThree, 20);
const EditIcon = antdIcon(PencilSimple, 16);
const TrashIcon = antdIcon(Trash, 16);

export function LibraryPage({ onCreate, onOpen }: { onCreate: () => void; onOpen: (projectId: string) => void }) {
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState("全部项目");
  const [editing, setEditing] = useState<{ id: string; name: string } | null>(null);
  const [deleting, setDeleting] = useState<{ id: string; name: string } | null>(null);
  const [savingName, setSavingName] = useState(false);
  const [deletingProject, setDeletingProject] = useState(false);
  const queryClient = useQueryClient();
  const { data: availableProjects = projects } = useQuery({ queryKey: ["projects"], queryFn: desktopBridge.listProjects });
  const readinessQueries = useQueries({ queries: availableProjects.map((project) => ({ queryKey: ["readiness", project.id], queryFn: () => desktopBridge.getProjectReadiness(project.id), staleTime: 3000 })) });
  const readinessByProject = useMemo(() => Object.fromEntries(readinessQueries.flatMap((query, index) => query.data ? [[availableProjects[index].id, query.data as ProjectReadiness]] : [])), [availableProjects, readinessQueries]);
  const filtered = useMemo(() => availableProjects.filter((project) => {
    if (!project.name.toLowerCase().includes(query.toLowerCase())) return false;
    if (filter === "全部项目") return true;
    const readiness = readinessByProject[project.id];
    if (filter === "可导出") return readiness ? readiness.canExport : project.status === "ready";
    return readiness ? !readiness.canExport : project.status === "processing";
  }), [availableProjects, filter, query, readinessByProject]);
  const saveName = async () => {
    if (!editing?.name.trim()) return;
    try {
      setSavingName(true);
      await desktopBridge.renameProject(editing.id, editing.name);
      await queryClient.invalidateQueries({ queryKey: ["projects"] });
      message.success("项目名称已更新");
      setEditing(null);
    } catch (error) { message.error(String(error)); }
    finally { setSavingName(false); }
  };
  const deleteProject = async (id: string) => {
    try {
      setDeletingProject(true);
      await desktopBridge.deleteProject(id);
      await Promise.all([queryClient.invalidateQueries({ queryKey: ["projects"] }), queryClient.invalidateQueries({ queryKey: ["jobs"] })]);
      message.success("项目及其生成文件已删除，原始视频未受影响");
      setDeleting(null);
    } catch (error) { message.error(String(error)); }
    finally { setDeletingProject(false); }
  };

  return <div className="page library-page">
    <section className="page-header">
      <h1>项目库</h1>
      <Button type="primary" size="large" icon={<PlusIcon />} onClick={onCreate}>新建项目</Button>
    </section>

    <div className="toolbar-row">
      <Segmented options={["全部项目", "处理中", "可导出"]} value={filter} onChange={setFilter} />
      <Input.Search className="search-field-antd" allowClear value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索项目" />
    </div>

    <section className="project-grid">
      {filtered.map((project) => { const readiness = readinessByProject[project.id]; return <article className="project-card" key={project.id} role="button" tabIndex={0} aria-label={`打开项目 ${project.name}，${readiness ? readinessLabel[readiness.phase] : statusLabel[project.status]}`} onClick={() => onOpen(project.id)} onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); onOpen(project.id); } }}>
        <Dropdown trigger={["click"]} menu={{ items: [{ key: "rename", label: "重命名项目", icon: <PencilSimple /> }, { type: "divider" }, { key: "delete", danger: true, label: "删除项目", icon: <Trash /> }], onClick: ({ key, domEvent }) => { domEvent.stopPropagation(); if (key === "rename") setEditing({ id: project.id, name: project.name }); else setDeleting({ id: project.id, name: project.name }); } }}><Button className="card-menu" type="text" icon={<MoreIcon />} aria-label="项目菜单" onClick={(event) => event.stopPropagation()} /></Dropdown>
        <div className="project-thumb">{project.thumbnail ? <img src={project.thumbnail} alt={`${project.name} 视频首帧`} /> : <div className="project-thumb-empty"><FileVideo size={28} /><small>正在生成视频首帧</small></div>}<span>{project.duration}</span></div>
        <div className="project-card-body">
          <div className="project-title-row"><h3>{project.name}</h3><span className={`status-chip ${readiness?.phase ?? project.status}`}>{readiness?.canExport && !readiness.warningCount ? <CheckCircle /> : readiness?.phase === "review" || readiness?.phase === "export_warning" ? <WarningCircle /> : <Clock />}{readiness ? readinessLabel[readiness.phase] : statusLabel[project.status]}</span></div>
          <p>英文 → 简体中文 · {project.segmentCount ?? "—"} 个片段</p>
          <Progress percent={readiness?.progress ?? project.progress} showInfo={false} size="small" />
          <footer><span>{readiness?.progress ?? project.progress}%</span><span>{project.updatedAt}</span><Button type="link" size="small" onClick={(event) => { event.stopPropagation(); onOpen(project.id); }}>{readiness?.warningCount ? `处理 ${readiness.warningCount} 个时长问题` : readiness?.nextAction ?? (project.status === "ready" ? "查看结果" : "继续处理")}</Button></footer>
        </div>
      </article>; })}
    </section>
    {filtered.length === 0 && <Empty className="empty-state" image={Empty.PRESENTED_IMAGE_SIMPLE} description="没有找到项目"><Button type="primary" onClick={onCreate}>新建项目</Button></Empty>}
    <AppModal open={Boolean(editing)} title="重命名项目" okText="保存" cancelText="取消" confirmLoading={savingName} okButtonProps={{ disabled: !editing?.name.trim() }} onOk={saveName} onCancel={() => setEditing(null)}><Input autoFocus prefix={<EditIcon />} value={editing?.name ?? ""} maxLength={120} showCount onPressEnter={saveName} onChange={(event) => setEditing((current) => current ? { ...current, name: event.target.value } : current)} /></AppModal>
    <AppModal open={Boolean(deleting)} title="删除项目" okText="删除项目" cancelText="取消" confirmLoading={deletingProject} okButtonProps={{ danger: true }} onOk={() => deleting && deleteProject(deleting.id)} onCancel={() => setDeleting(null)}><p className="modal-confirm-copy">将删除“{deleting?.name}”的任务、字幕、配音和预览文件，但不会删除原始视频。此操作不可撤销。</p></AppModal>
  </div>;
}
