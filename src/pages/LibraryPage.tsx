import { useMemo, useState } from "react";
import { Button, Empty, Input, Progress, Segmented, Tooltip } from "antd";
import { CheckCircle, Clock, DotsThree, FolderOpen, Plus, UploadSimple, WarningCircle } from "@phosphor-icons/react";
import { projects } from "../fixtures";
import { statusLabel } from "../domain";
import { useQuery } from "@tanstack/react-query";
import { desktopBridge } from "../bridge";
import { antdIcon } from "../ui/icons";

const PlusIcon = antdIcon(Plus, 17);
const FolderIcon = antdIcon(FolderOpen, 16);
const MoreIcon = antdIcon(DotsThree, 20);

export function LibraryPage({ onCreate, onOpen }: { onCreate: () => void; onOpen: (projectId: string) => void }) {
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState("全部项目");
  const { data: availableProjects = projects } = useQuery({ queryKey: ["projects"], queryFn: desktopBridge.listProjects });
  const { data: segmentCounts = {} } = useQuery({
    queryKey: ["project-segment-counts", availableProjects.map((project) => project.id)],
    queryFn: async () => Object.fromEntries(await Promise.all(availableProjects.map(async (project) => [project.id, (await desktopBridge.listSegments(project.id)).length]))),
    enabled: desktopBridge.isDesktop() && availableProjects.length > 0,
  });
  const filtered = useMemo(() => availableProjects.filter((project) => project.name.toLowerCase().includes(query.toLowerCase()) && (filter === "全部项目" || (filter === "处理中" ? project.status === "processing" : project.status === "ready"))), [availableProjects, query, filter]);

  return <div className="page library-page">
    <section className="page-header">
      <div><span className="eyebrow">本地项目</span><h1>项目库</h1><p>管理视频中文化任务，所有素材和中间结果都保存在这台 Mac。</p></div>
      <Button type="primary" size="large" icon={<PlusIcon />} onClick={onCreate}>新建项目</Button>
    </section>

    <section className="quick-import" onClick={onCreate} role="button" tabIndex={0}>
      <div className="import-icon"><UploadSimple size={24} /></div>
      <div><strong>拖入英文视频，开始中文化</strong><p>支持 MP4、MOV、MKV，原始文件不会被复制或上传。</p></div>
      <Button icon={<FolderIcon />}>选择文件</Button>
    </section>

    <div className="toolbar-row">
      <Segmented options={["全部项目", "处理中", "可导出"]} value={filter} onChange={setFilter} />
      <Input.Search className="search-field-antd" allowClear value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索项目" />
    </div>

    <section className="project-grid">
      {filtered.map((project) => <article className="project-card" key={project.id} onDoubleClick={() => onOpen(project.id)}>
        <Tooltip title="项目菜单"><Button className="card-menu" type="text" icon={<MoreIcon />} aria-label="项目菜单" /></Tooltip>
        <div className="project-thumb"><img src={project.thumbnail} alt="RAG 技术课程预览" /><span>{project.duration}</span></div>
        <div className="project-card-body">
          <div className="project-title-row"><h3>{project.name}</h3><span className={`status-chip ${project.status}`}>{project.status === "ready" ? <CheckCircle /> : project.status === "waiting_user" ? <WarningCircle /> : <Clock />}{statusLabel[project.status]}</span></div>
          <p>英文 → 简体中文 · {segmentCounts[project.id] ?? "—"} 个片段</p>
          <Progress percent={project.progress} showInfo={false} size="small" />
          <footer><span>{project.progress}%</span><span>{project.updatedAt}</span><Button type="link" size="small" onClick={() => onOpen(project.id)}>{project.status === "ready" ? "查看结果" : "继续处理"}</Button></footer>
        </div>
      </article>)}
    </section>
    {filtered.length === 0 && <Empty className="empty-state" image={Empty.PRESENTED_IMAGE_SIMPLE} description="没有找到项目"><Button type="primary" onClick={onCreate}>新建项目</Button></Empty>}
  </div>;
}
