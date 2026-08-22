# 可靠性目标

## 用户可感知保证

- 应用重启后，运行中任务恢复为可继续状态，不谎报成功。
- 暂停、取消或失败不会破坏上一次已发布的可用音轨和预览。
- 相同真实输入命中缓存时，不重复请求在线 Provider。
- 未通过校验的临时媒体不会成为正式 Artifact。
- 安全音频模式缺少有效背景轨时阻止导出，不回退到完整原声。
- 未知任务阶段、未知 IPC 枚举和无效 Provider 返回在边界失败，不静默猜测。

## 运行事件最低字段

每次工作流与节点事件至少记录：`run_id`、`node_run_id`、节点 ID/版本、attempt、状态、时间、进度、错误类别、输入/输出 Artifact ID。事件不得记录密钥、完整请求头、原始视频内容或不必要的用户文案。

生产 Runner 额外记录 `resource_acquired` 与 `node_observed` 事件，包括资源类别、等待毫秒数、执行毫秒数和脱敏 outcome。单进程同时只允许一个完整重工作流进入执行区；CPU、媒体、网络、磁盘资源另有分类信号量。

## 失败分类

- `validation`：输入或领域不变量错误，不自动重试。
- `dependency`：Runtime、模型、文件或权限缺失，等待用户修复。
- `provider_retryable`：限流、超时和短暂网络故障，受预算限制重试。
- `provider_terminal`：鉴权、模型/音色无效或确定性拒绝，不自动重试。
- `media`：FFmpeg/Sidecar 失败，保留脱敏 stderr 摘要和 staging 证据。
- `cancelled`：用户取消，后续发布必须被 CAS/状态检查阻断。

## 验收层级

- `verify:fast`：架构门禁、TypeScript、轻量测试、Rust check。
- `verify:full`：前述检查、前端构建、Rust tests、Clippy、格式检查。
- `verify:release`：仅明确要求发布时生成独立 App。
- 真实媒体检查：涉及音频图、Sidecar、预览、导出或系统权限时必做。

M4 验收还要求：前端源码不得按持久化 stage 分发执行；旧媒体/ASR/翻译/job 手工交接 command 不得出现在 Tauri handler；TTS 文件和导出目录必须从同卷 staging 原子发布。
