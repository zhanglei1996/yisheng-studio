import { useMemo, useState } from "react";
import { Alert, Button, Empty, Form, Input, Popconfirm, Select, Spin, Tooltip, message } from "antd";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { MagnifyingGlass, PencilSimple, Plus, Trash } from "@phosphor-icons/react";
import { desktopBridge } from "../bridge";
import type { GlossaryTerm, Project } from "../domain";
import { glossaryTerms as fixtureTerms } from "../fixtures";
import { antdIcon } from "../ui/icons";
import { AppModal } from "../components/AppModal";

const PlusIcon = antdIcon(Plus, 17);
const EditIcon = antdIcon(PencilSimple);
const TrashIcon = antdIcon(Trash);

type PersistedGlossaryTerm = GlossaryTerm & {
  projectId?: string | null;
  enabled?: boolean;
};

type GlossaryForm = {
  source: string;
  target: string;
  policy: GlossaryTerm["policy"];
  scope: GlossaryTerm["scope"];
  projectId?: string;
  enabled: boolean;
};

type GlossaryBridge = typeof desktopBridge & {
  listGlossary(projectId?: string | null): Promise<PersistedGlossaryTerm[]>;
  saveGlossary(term: PersistedGlossaryTerm): Promise<PersistedGlossaryTerm | null>;
  deleteGlossary(id: string): Promise<void>;
};

const glossaryBridge = desktopBridge as GlossaryBridge;
const isProjectTerm = (term: PersistedGlossaryTerm) => term.scope === "project" || Boolean(term.projectId);
const normalizedFixture = fixtureTerms.map<PersistedGlossaryTerm>((term) => ({ ...term, enabled: term.policy !== "disabled" }));

export function GlossaryPage() {
  const desktop = desktopBridge.isDesktop();
  const queryClient = useQueryClient();
  const [browserTerms, setBrowserTerms] = useState(normalizedFixture);
  const [query, setQuery] = useState("");
  const [scope, setScope] = useState<"all" | "project" | "global">("all");
  const [projectId, setProjectId] = useState<string | null>(null);
  const [modalOpen, setModalOpen] = useState(false);
  const [editing, setEditing] = useState<PersistedGlossaryTerm | null>(null);
  const [form] = Form.useForm<GlossaryForm>();

  const projectsQuery = useQuery({ queryKey: ["projects"], queryFn: desktopBridge.listProjects });
  const projects = projectsQuery.data ?? [];
  const glossaryQuery = useQuery({
    queryKey: ["glossary", projectId],
    queryFn: () => glossaryBridge.listGlossary(projectId),
    enabled: desktop,
    retry: 1,
  });
  const terms = desktop ? (glossaryQuery.data ?? []) : browserTerms;

  const visible = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    return terms.filter((term) => {
      const projectTerm = isProjectTerm(term);
      const matchesScope = scope === "all" || (scope === "project" ? projectTerm : !projectTerm);
      const matchesProject = !projectTerm || !projectId || term.projectId === projectId || (!term.projectId && scope !== "project");
      const matchesQuery = !needle || `${term.source}\n${term.target}`.toLocaleLowerCase().includes(needle);
      return matchesScope && matchesProject && matchesQuery;
    });
  }, [projectId, query, scope, terms]);

  const saveMutation = useMutation({
    mutationFn: async (term: PersistedGlossaryTerm) => {
      if (!desktop) {
        setBrowserTerms((current) => current.some((item) => item.id === term.id)
          ? current.map((item) => item.id === term.id ? term : item)
          : [term, ...current]);
        return term;
      }
      return glossaryBridge.saveGlossary(term);
    },
    onSuccess: async () => {
      if (desktop) await queryClient.invalidateQueries({ queryKey: ["glossary"] });
      setModalOpen(false);
      setEditing(null);
      form.resetFields();
      message.success("术语已保存");
    },
    onError: (error) => message.error(`保存失败：${String(error)}`),
  });

  const deleteMutation = useMutation({
    mutationFn: async (id: string) => {
      if (!desktop) {
        setBrowserTerms((current) => current.filter((item) => item.id !== id));
        return;
      }
      await glossaryBridge.deleteGlossary(id);
    },
    onSuccess: async () => {
      if (desktop) await queryClient.invalidateQueries({ queryKey: ["glossary"] });
      message.success("术语已删除");
    },
    onError: (error) => message.error(`删除失败：${String(error)}`),
  });

  const openCreate = () => {
    setEditing(null);
    form.setFieldsValue({ source: "", target: "", policy: "fixed", scope: projectId ? "project" : "global", projectId: projectId ?? undefined, enabled: true });
    setModalOpen(true);
  };

  const openEdit = (term: PersistedGlossaryTerm) => {
    setEditing(term);
    form.setFieldsValue({
      source: term.source,
      target: term.target,
      policy: term.policy,
      scope: isProjectTerm(term) ? "project" : "global",
      projectId: term.projectId ?? projectId ?? undefined,
      enabled: term.enabled ?? term.policy !== "disabled",
    });
    setModalOpen(true);
  };

  const submit = async () => {
    const values = await form.validateFields();
    const projectScope = values.scope === "project";
    saveMutation.mutate({
      id: editing?.id ?? crypto.randomUUID(),
      source: values.source.trim(),
      target: values.target.trim(),
      policy: values.enabled ? values.policy : "disabled",
      scope: values.scope,
      confidence: editing?.confidence ?? 1,
      projectId: projectScope ? values.projectId ?? projectId : null,
      enabled: values.enabled,
    });
  };

  const projectName = (id?: string | null) => projects.find((project) => project.id === id)?.name ?? "当前项目";

  return <div className="page">
    <section className="page-header">
      <h1>术语库</h1>
      <Button type="primary" size="large" icon={<PlusIcon />} onClick={openCreate}>添加术语</Button>
    </section>

    <div className="toolbar-row">
      <div className="toolbar-group">
        <Input prefix={<MagnifyingGlass />} allowClear value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索源词或译法" className="glossary-search" />
        <Select value={scope} onChange={setScope} options={[{ value: "all", label: "全部范围" }, { value: "project", label: "项目术语" }, { value: "global", label: "全局术语" }]} />
        <Select<Project["id"]> allowClear value={projectId ?? undefined} onChange={(value) => setProjectId(value ?? null)} placeholder="选择项目" loading={projectsQuery.isLoading} options={projects.map((project) => ({ value: project.id, label: project.name }))} style={{ minWidth: 190 }} />
      </div>
      {!desktop && <span className="neutral-chip">浏览器原型数据</span>}
    </div>

    {glossaryQuery.isError && <Alert type="error" showIcon title="术语库加载失败" description={String(glossaryQuery.error)} action={<Button size="small" onClick={() => glossaryQuery.refetch()}>重试</Button>} />}
    <section className="data-panel">
      {glossaryQuery.isLoading && desktop ? <div className="queue-empty"><Spin /><span>正在读取术语库…</span></div> : <div className="data-table glossary-table">
        <div className="table-head"><span>源词</span><span>目标译法</span><span>策略</span><span>范围</span><span>状态</span><span /></div>
        {visible.map((term) => <div className="table-row" key={term.id}>
          <strong>{term.source}</strong>
          <span>{term.target}</span>
          <span className="neutral-chip">{term.policy === "keep" ? "保留原文" : term.policy === "fixed" ? "固定译法" : "禁用"}</span>
          <span>{isProjectTerm(term) ? projectName(term.projectId) : "全局"}</span>
          <span>{term.enabled === false || term.policy === "disabled" ? "已停用" : "已启用"}</span>
          <span className="toolbar-group">
            <Tooltip title="编辑"><Button type="text" className="quiet" icon={<EditIcon />} aria-label="编辑" onClick={() => openEdit(term)} /></Tooltip>
            <Popconfirm title="删除这条术语？" description="后续翻译将不再应用该规则。" okText="删除" cancelText="取消" okButtonProps={{ danger: true }} onConfirm={() => deleteMutation.mutate(term.id)}>
              <Tooltip title="删除"><Button type="text" danger className="quiet" loading={deleteMutation.isPending && deleteMutation.variables === term.id} icon={<TrashIcon />} aria-label="删除" /></Tooltip>
            </Popconfirm>
          </span>
        </div>)}
      </div>}
      {!glossaryQuery.isLoading && visible.length === 0 && !glossaryQuery.isError && <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={query || scope !== "all" || projectId ? "没有匹配的术语" : "还没有术语规则"}><Button type="primary" icon={<PlusIcon />} onClick={openCreate}>添加第一条术语</Button></Empty>}
    </section>

    <AppModal open={modalOpen} title={editing ? "编辑术语" : "添加术语"} okText="保存" cancelText="取消" confirmLoading={saveMutation.isPending} onOk={submit} onCancel={() => { setModalOpen(false); setEditing(null); form.resetFields(); }}>
      <Form form={form} layout="vertical" requiredMark={false} preserve={false}>
        <Form.Item name="source" label="源词" rules={[{ required: true, whitespace: true, message: "请输入源词" }, { max: 256, message: "源词过长" }]}><Input autoFocus placeholder="例如 RAG" /></Form.Item>
        <Form.Item name="target" label="目标译法" rules={[{ required: true, whitespace: true, message: "请输入目标译法" }, { max: 256, message: "译法过长" }]}><Input placeholder="例如 检索增强生成" /></Form.Item>
        <Form.Item name="policy" label="应用策略"><Select options={[{ value: "fixed", label: "固定译法" }, { value: "keep", label: "保留原文" }]} /></Form.Item>
        <Form.Item name="scope" label="作用范围"><Select options={[{ value: "project", label: "指定项目" }, { value: "global", label: "全局项目" }]} /></Form.Item>
        <Form.Item noStyle shouldUpdate={(before, after) => before.scope !== after.scope}>{({ getFieldValue }) => getFieldValue("scope") === "project" && <Form.Item name="projectId" label="所属项目" rules={[{ required: true, message: "请选择项目" }]}><Select placeholder="选择项目" options={projects.map((project) => ({ value: project.id, label: project.name }))} /></Form.Item>}</Form.Item>
        <Form.Item name="enabled" label="状态"><Select options={[{ value: true, label: "启用" }, { value: false, label: "停用" }]} /></Form.Item>
      </Form>
    </AppModal>
  </div>;
}
