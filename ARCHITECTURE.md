# 译声工坊架构地图

本文是仓库级架构入口，只描述稳定边界与依赖方向。具体决策、迁移步骤和产品规则由下方链接承载。

## 系统定位

译声工坊是 macOS 本地优先的视频中文化桌面应用。原视频、原始音轨、工程数据库和中间产物留在本机；Rust 核心负责可信执行边界，React 只负责呈现状态和发送用户意图。在线翻译或配音只能通过显式配置的 Provider 发送必要文本与参数。

## 目标依赖方向

```text
React Feature UI
  -> typed IPC contracts
  -> application use cases
  -> workflow/domain
  -> ports
  <- infrastructure adapters (SQLite, FFmpeg, Keychain, HTTP, Sidecar)
```

依赖只能沿箭头向前：

- Tauri commands 是薄适配层，只做边界解析、调用 application use case、映射错误和发布事件。
- Workflow Runner 是任务状态、下一节点、重试和人工审核等待的唯一权威。
- Domain 不依赖 Tauri、SQLite、HTTP、FFmpeg 或文件系统。
- Infrastructure 实现 domain/application 定义的 ports，不反向决定业务流程。
- React 不根据阶段字符串编排后端，只发送开始、继续、取消、审核决定和节点重试等意图。

## 目标模块

```text
src-tauri/src/
  contracts/       IPC DTO 与稳定枚举
  application/     用例、事务边界与查询服务
  workflow/        定义、Runner、节点结果、运行日志与资源策略
  domains/         project、localization、dubbing、export
  ports/           Repository、Provider、MediaRuntime、ArtifactStore
  infrastructure/  SQLite、Provider、FFmpeg、Keychain、Runtime 实现
  commands/        Tauri 薄适配层
  observability/   脱敏结构化事件与诊断包

src/
  app/             路由、全局壳和 Provider
  features/        library、queue、editor、providers、export
  entities/        project、job、segment
  shared/api/      生成的 IPC client 与合同
```

当前仍是迁移中的模块化单体，不因为目录目标而提前拆分 Cargo crate，也不把单机工作流拆成网络微服务。FFmpeg、Whisper、人声分离和本地模型可以作为受控 Sidecar/Runtime。

## 核心不变量

1. 任务阶段必须使用类型化 `JobStage`，持久化未知值必须报错。
2. 同一时刻最多运行一个重型任务；取消后的节点不得发布产物。
3. 节点产物先写 staging，验证成功后才成为可见 Artifact。
4. Artifact 身份由真实输入、实现/模型版本和参数共同决定；展示状态不能作为缓存身份。
5. 人工审核是显式工作流结果，不与普通暂停混用。
6. 不允许完整原声绕过安全音频模式进入最终混音。
7. 凭据只进入 Keychain，日志、诊断包、数据库公开配置和 IPC 均不得携带密钥。

## 事实源索引

- 系统技术方案：[`docs/architecture/TECHNICAL_SOLUTION.md`](docs/architecture/TECHNICAL_SOLUTION.md)
- 本地优先决策：[`docs/architecture/ADR-001-local-first-tauri.md`](docs/architecture/ADR-001-local-first-tauri.md)
- 工作流内核决策：[`docs/architecture/ADR-002-versioned-workflow-runner.md`](docs/architecture/ADR-002-versioned-workflow-runner.md)
- 迁移执行计划：[`docs/engineering/WORKFLOW_REFACTOR_EXECUTION_PLAN.md`](docs/engineering/WORKFLOW_REFACTOR_EXECUTION_PLAN.md)
- 可靠性目标：[`RELIABILITY.md`](RELIABILITY.md)
- 质量基线：[`QUALITY_SCORE.md`](QUALITY_SCORE.md)
- 隐私与版权：[`docs/product/PRIVACY_AND_COPYRIGHT.md`](docs/product/PRIVACY_AND_COPYRIGHT.md)
- 工程取舍：[`docs/engineering/LEARNING_LOG.md`](docs/engineering/LEARNING_LOG.md)
