# 工作流内核重构执行计划

状态：M4 已完成
负责人：仓库维护者
相关决策：`ARCHITECTURE.md`、`ADR-002-versioned-workflow-runner.md`

## 目标

把任务编排权从 React 页面和大型 Tauri command 收口到可恢复、可测试、版本化的 Rust Workflow Runner，同时保持现有项目、任务、缓存和导出结果兼容。

## 非目标

- 不改变当前产品视觉或编辑交互。
- 不把应用拆成微服务或引入消息队列。
- 不开放用户自定义工作流。
- 不在第一批迁移真实媒体、TTS 或导出实现。

## 里程碑

### M0：架构护栏与类型基线

- [x] 建立顶层架构地图、可靠性与质量基线。
- [x] 将持久任务阶段改为 Rust/TypeScript 类型化 `JobStage`。
- [x] 未知 SQLite 阶段值在读取边界失败，不再静默回退。
- [x] 加入文件增长、越层调用、直接 Tauri invoke 和裸阶段字符串门禁。
- [x] `verify:fast` 执行架构门禁。

完成条件：不改变用户行为；现有测试通过；新门禁能对人为违规 fixture 失败。

### M1：Runner 最小内核

- [x] 定义 `WorkflowDefinition`、`WorkflowNode`、`NodeOutcome`、`ExecutionContext`。
- [x] 定义节点资源类别、取消令牌和重试分类。
- [x] 使用 fake nodes 覆盖完成、失败、等待审核、取消和恢复。
- [x] 增加 `workflow_runs`、`node_runs`、`run_events` 迁移。
- [x] 从运行事件投影兼容 `JobSummary`。

完成条件：Runner 可在临时 SQLite 中跨进程恢复，重复执行不会重复发布已完成节点产物。

### M2：迁移媒体、ASR 与翻译

- [x] `MediaPrepareNode` 接管音频抽取、人声分离和代理生成。
- [x] `AsrNode` 接管 whisper 执行与片段原子替换。
- [x] `TranslationNode` 接管批处理、格式重试和增量复用。
- [x] `ReviewGateNode` 表达字幕/翻译审核。
- [x] 旧 Tauri commands 从 IPC handler 移除，由 application workflow use cases 统一调用。

完成条件：QueuePage 不再决定上述阶段的下一步；快速模式和审核模式使用同一工作流定义。

### M3：迁移配音与发布链路

- [x] TTS 严格、平衡、连续和语义模式由 `TtsSynthesisNode` 选择节点策略。
- [x] Artifact staging、非空校验、回滚和原子发布成为统一服务。
- [x] 对齐校验、自动适配、混音预览与外部导出发布进入工作流合同。
- [x] 取消后产物不可发布；局部重试只重跑失效节点/单元。

完成条件：任务恢复、缓存命中、局部重生成和安全音频预检均由 Runner 合同覆盖。

### M4：删除兼容编排并收口前端

- [x] QueuePage 删除 `runPersistedStage`。
- [x] Bridge 的队列控制只暴露工作流意图和查询接口。
- [x] Queue 使用 workflow feature hook 与 React Query projection。
- [x] 从 Tauri handler 删除旧 job 阶段手工交接入口。
- [x] 将 `commands.rs`/`db.rs` 上限分别收紧至 5300/2850 行并写入质量基线。

完成条件：搜索不到前端阶段分发；commands 只保留薄适配层；完整验收和真实媒体交互检查通过。

## 验证矩阵

| 风险 | 自动验证 | 人工验证 |
|---|---|---|
| 状态迁移漂移 | 状态表驱动单元测试、SQLite 集成测试 | 队列暂停/继续/取消 |
| 重复云端调用 | fake Provider 调用计数、Artifact 哈希测试 | 切换音轨后缓存提示 |
| 崩溃产生半成品 | staging/atomic publish 测试 | 处理中强退后重启 |
| IPC 合同漂移 | Rust/TS 合同生成或一致性测试 | 桌面主要路径 |
| 安全音频回退 | filter graph 与 preflight 测试 | 多类型真实样片试听 |
| 凭据泄露 | 日志/诊断脱敏测试 | 导出诊断包检查 |

## 决策日志

- 2026-08-14：选择模块化单体与固定版本化 DAG；不采用微服务或通用工作流平台。
- 2026-08-14：先建立机械护栏和类型基线，再迁移执行逻辑，避免拆文件但继续保留隐式控制面。
- 2026-08-14：M1 Runner 与 SQLite Store 通过接口分离；运行事件投影旧任务模型，已完成节点按 node version 幂等跳过。
- 2026-08-14：M2-M4 使用一个版本为 4 的固定生产 DAG 接管真实节点；审核通过由显式 resume 表达，快速和审核模式共享定义。
- 2026-08-14：历史任务按旧状态/阶段分类迁移；早阶段任务可由 Runner 接管，晚阶段暂停或等待任务直接回到编辑器输入边界，终态任务保持终态，禁止因缺少 `workflow_run` 而从媒体准备重跑。
- 2026-08-14：TTS 文件与完整导出目录使用同卷 staging + rename 发布；数据库提交失败时恢复上一版文件。
- 2026-08-14：外部导出是等待输入节点，导出成功后以幂等外部完成事件结束 workflow run。
