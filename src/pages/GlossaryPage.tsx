import { useState } from "react";
import { Button, Input, Select, Tooltip } from "antd";
import { DownloadSimple, Funnel, LockSimple, MagnifyingGlass, Plus, Trash, UploadSimple } from "@phosphor-icons/react";
import { glossaryTerms as fixtureTerms } from "../fixtures";
import { antdIcon } from "../ui/icons";

const PlusIcon = antdIcon(Plus, 17);
const FilterIcon = antdIcon(Funnel);
const UploadIcon = antdIcon(UploadSimple);
const DownloadIcon = antdIcon(DownloadSimple);
const TrashIcon = antdIcon(Trash);
const LockIcon = antdIcon(LockSimple);

export function GlossaryPage() {
  const [terms, setTerms] = useState(fixtureTerms);
  const [query, setQuery] = useState("");
  const [scope, setScope] = useState("全部范围");
  const visible = terms.filter((term) => (term.source + term.target).toLowerCase().includes(query.toLowerCase()) && (scope === "全部范围" || (scope === "当前项目" ? term.scope === "project" : term.scope === "global")));
  return <div className="page">
    <section className="page-header"><div><span className="eyebrow">翻译一致性</span><h1>术语库</h1><p>固定产品名、框架名和技术缩写的译法，项目规则优先于全局规则。</p></div><Button type="primary" size="large" icon={<PlusIcon />} onClick={() => setTerms([{ id: crypto.randomUUID(), source: "New term", target: "新术语", policy: "fixed", scope: "project", confidence: 1 }, ...terms])}>添加术语</Button></section>
    <div className="toolbar-row"><div className="toolbar-group"><Input prefix={<MagnifyingGlass />} allowClear value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索源词或译法" className="glossary-search" /><Select value={scope} onChange={setScope} options={["全部范围", "当前项目", "全局"].map((value) => ({ value, label: value }))} /><Tooltip title="筛选"><Button icon={<FilterIcon />} aria-label="筛选" /></Tooltip></div><div className="toolbar-group"><Button icon={<UploadIcon />}>导入 CSV</Button><Button icon={<DownloadIcon />}>导出</Button></div></div>
    <section className="data-panel"><div className="data-table glossary-table"><div className="table-head"><span>源词</span><span>目标译法</span><span>策略</span><span>范围</span><span>置信度</span><span /></div>{visible.map((term) => <div className="table-row" key={term.id}><strong>{term.source}</strong><span>{term.target}</span><span className="neutral-chip">{term.policy === "keep" ? "保留英文" : term.policy === "fixed" ? "固定译法" : "禁用"}</span><span>{term.scope === "project" ? "当前项目" : "全局"}</span><span>{Math.round(term.confidence * 100)}%</span><Tooltip title={term.scope === "global" ? "全局术语不可删除" : "删除"}><Button type="text" className="quiet" icon={term.scope === "global" ? <LockIcon /> : <TrashIcon />} aria-label={term.scope === "global" ? "全局术语不可删除" : "删除"} disabled={term.scope === "global"} onClick={() => setTerms(terms.filter((item) => item.id !== term.id))} /></Tooltip></div>)}</div></section>
  </div>;
}
