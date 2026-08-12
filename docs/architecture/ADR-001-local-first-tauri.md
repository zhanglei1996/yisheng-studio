# ADR-001：采用 Tauri 2 本地优先架构

## 状态

已接受。

## 决策

使用 Tauri 2 + React + TypeScript 构建 macOS 桌面应用。Rust 核心负责 SQLite、Keychain、任务编排、文件访问和 Sidecar 生命周期；前端不直接执行 Shell，也不获取任意文件系统权限。

媒体和识别能力通过按架构分发的 FFmpeg、whisper.cpp 与人声分离 Sidecar 提供。外部翻译和在线 TTS 由桌面应用直接调用用户配置的平台，不经过自建后端。

## 原因

- 大文件与中间产物留在本地，降低服务器成本并明确隐私边界。
- Sidecar 允许用户无需安装 Python、Node 或命令行依赖。
- Provider Adapter 避免产品绑定单一模型或语音平台。
- SQLite 检查点和 Artifact 依赖哈希支持长任务恢复与局部重生成。

