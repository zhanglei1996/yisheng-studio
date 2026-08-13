import courseFrame from "./assets/course-frame-rag.png";
import type { GlossaryTerm, Job, Project, Segment } from "./domain";

export { courseFrame };

export const projects: Project[] = [
  { id: "p1", name: "Building Reliable AI Agents", duration: "24:18", progress: 78, status: "waiting_user", updatedAt: "今天 10:42", thumbnail: courseFrame, segmentCount: 48 },
  { id: "p2", name: "React Server Components 深度解析", duration: "18:36", progress: 100, status: "ready", updatedAt: "昨天 18:20", thumbnail: courseFrame, segmentCount: 36 },
  { id: "p3", name: "RAG From Scratch", duration: "42:07", progress: 36, status: "processing", updatedAt: "8 月 10 日", thumbnail: courseFrame, segmentCount: 82 },
];

export const initialSegments: Segment[] = [
  { id: "seg-01", startMs: 306120, endMs: 310420, sourceText: "When building AI applications, reliability is critical.", subtitleZh: "在构建 AI 应用时，可靠性至关重要。", spokenZh: "构建 AI 应用时，可靠性至关重要。", linked: false, status: "ready", voice: "普通话 · 女声 · 自然", speed: 1 },
  { id: "seg-02", startMs: 310420, endMs: 315460, sourceText: "The agent retrieves the relevant context before generating a response.", subtitleZh: "智能体会先检索相关上下文，再生成回答。", spokenZh: "智能体会先检索相关上下文，再生成回答。", linked: true, status: "warning", voice: "普通话 · 女声 · 自然", speed: 1, overflowMs: 800 },
  { id: "seg-03", startMs: 315460, endMs: 318760, sourceText: "This is typically achieved through a RAG pipeline.", subtitleZh: "这通常通过 RAG 流程来实现。", spokenZh: "这通常通过 RAG 流程实现。", linked: false, status: "ready", voice: "普通话 · 女声 · 自然", speed: 1 },
  { id: "seg-04", startMs: 318760, endMs: 322620, sourceText: "The retrieved passages are combined with the user question as input.", subtitleZh: "检索到的片段会与用户问题一起作为提示输入模型。", spokenZh: "检索片段会和用户问题一起输入模型。", linked: false, status: "ready", voice: "普通话 · 女声 · 自然", speed: 1.04 },
];

export const glossaryTerms: GlossaryTerm[] = [
  { id: "t1", source: "Agent", target: "智能体", policy: "fixed", scope: "project", confidence: 0.98 },
  { id: "t2", source: "RAG", target: "RAG", policy: "keep", scope: "global", confidence: 1 },
  { id: "t3", source: "token", target: "令牌", policy: "fixed", scope: "global", confidence: 0.91 },
  { id: "t4", source: "pipeline", target: "流程", policy: "fixed", scope: "project", confidence: 0.89 },
  { id: "t5", source: "prompt", target: "提示词", policy: "fixed", scope: "global", confidence: 0.96 },
];

export const jobs: Job[] = [
  { id: "j1", projectId: "p3", project: "RAG From Scratch", stage: "本地语音识别", progress: 36, status: "running", eta: "约 08:20" },
  { id: "j2", projectId: "p1", project: "Building Reliable AI Agents", stage: "等待文本确认", progress: 78, status: "waiting_user", eta: "—" },
  { id: "j3", project: "LLM Evaluation Methods", stage: "等待队列", progress: 0, status: "queued", eta: "约 31 分钟后" },
];
