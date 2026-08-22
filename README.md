# 译声工坊

本地优先的 AI 视频中文化桌面工具。macOS 基线链路已经跑通：本地视频导入 → FFmpeg 媒体准备 → whisper.cpp 英文识别 → DeepSeek/百炼兼容翻译 → 方案三结构化口播编排 → 中文配音 → 时间轴约束 → MP4/字幕/音轨导出。中文配音除 macOS 系统语音外，现已接入阿里百炼 TTS 与讯飞超拟人语音；两家真实 Keychain 凭据已在刷新后的本地 `.app` 中完成最小合成连接测试。

## 已实现

- 暗黑剪辑工作台：真实代理视频播放、三标签片段检查器、四轨时间轴和动态风险提示。
- 项目库、新建项目四步流程、术语库、任务队列、服务商、组件/隐私设置和导出流程。
- 片段选择、文本编辑、字幕/配音联动、声音/对齐状态、拆分、合并、边界拖动、撤销/重做，以及单片段、失败片段和全项目范围的真实配音生成。
- Tauri 2 Rust 边界、SQLite schema/migration、项目与任务持久化、片段非重叠校验、单重型任务互斥、暂停/继续/取消/重试、检查点与崩溃恢复、任务状态事件、Artifact 失效规则和 Keychain 凭据引用。
- 桌面端真实导入 MP4/MOV/MKV，使用 ffprobe 检查媒体并计算源文件指纹；原视频只做引用，代理视频、16 kHz 识别音频和任务检查点写入项目产物目录。
- whisper.cpp 使用 `small.en` 生成时间戳并落库为稳定 UUID 片段；真实 10:29 技术样片识别出 136 个不重叠片段。
- DeepSeek、阿里百炼和自定义 OpenAI 兼容翻译接口；批量 UUID 严格校验、格式异常回退和已完成片段复用。
- 方案三“口播稿内联编排”：在片段内编辑强调、停顿、保护词与演绎方式，自动导演和显式风格共同生成可审阅的结构化口播稿；不支持原生内联控制的引擎会收到等价的标点与导演指令渲染。
- macOS Tingting 系统语音、阿里百炼 Qwen/CosyVoice 与讯飞超拟人语音适配；支持服务商试听、局部生成、缓存复用、失败片段重试、旧混音保留和合成预览刷新。
- 服务商公开配置写入 SQLite；DashScope API Key、讯飞 APIPassword 或 APIKey + APISecret 只写入 macOS Keychain。产品文案明确：在线合成仅发送当前中文口播稿与语音参数，不上传原视频、原始音轨或完整工程。
- 只裁剪音频首尾静音并保留句中自然停顿；云端短音频会做完整性校验与自动重试。片段在 0.82–1.08x 的自然区间内适配源时间窗，并结合相邻口播上下文维持语气连续，仍禁止片段互相侵入。
- 编辑器支持单片段重试和显式“整片重新配音”；整片操作会提示实际片段数与可能的在线费用，初次快速生成流程不会被逐阶段确认打断。
- 导出 H.264/AAC 中文配音 MP4、中文/英文 SRT、中英双语 ASS 与 48 kHz 中文 WAV；任务可暂停、重试并在重启后从检查点继续。
- 独立 TTS smoke harness 位于 `src-tauri/src/bin/tts-smoke.rs`：从 Keychain 取凭据，只输出脱敏结果类别，并以 120 秒硬超时退出。它用于本地诊断，不替代 `.app` 中的 GUI 连接与试听验收。
- 产品、信息架构、隐私、设计规范、架构决策和许可证发布门禁文档。

浏览器预览仍使用固定数据；真实媒体处理、Keychain 与云端语音连接测试仅在 Tauri 桌面应用内启用。当前开发机已经安装 whisper.cpp `v1.9.2`、`small.en` 与 FFmpeg。人声分离、运行时在线安装包和正式签名/公证仍待实现；真实凭据已完成最小连接合成，完整项目的全片合成与主观音质验收仍应使用专门样片执行。本机 FFmpeg 8.0.1 是启用 GPL 的 Homebrew 构建，仅用于开发检查；正式分发必须换成许可证门禁文档要求的 LGPL Sidecar。

## 开发

```bash
pnpm install
pnpm dev
```

浏览器预览默认由 Vite 提供。运行桌面壳需要先安装 Rust stable：

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
pnpm dev:desktop
```

生成可从 Finder 直接打开、且不依赖本地开发服务器的调试版 App：

```bash
pnpm dev:app
```

## 验证

项目采用三级本地验收，默认不部署站点：日常实现使用快速验收，交付前使用完整验收，只有明确需要刷新独立 macOS App 时才进入发布验收。

一级与二级验收都会并行执行前后端检查：

```bash
pnpm verify:fast
pnpm verify:full
```

三级发布验收会先执行完整验收，再生成可从 Finder 独立启动的调试版 App；只有明确需要刷新该 App 时才运行：

```bash
pnpm verify:release
```

Sites 不属于默认验收流程。仅在修改 Sites 适配或明确要求部署时运行 `pnpm verify:sites`；该命令只做本地 Sites 构建和测试，不会部署。

当前开发构建位于 `src-tauri/target/debug/bundle/macos/译声工坊.app`。请通过 `pnpm dev:app` 或 `pnpm verify:release` 更新此构建，不要把 `pnpm dev:desktop` 运行中的临时开发壳当成可独立启动版本。`verify:release` 会完成本机独立运行所需的 ad-hoc 签名和严格 Bundle 校验；面向外部分发时仍需使用 Apple Developer ID 签名并完成 notarization。API Key 在应用左侧“服务商”中配置；不要写入仓库配置文件。

视觉验收记录见 [`design-qa.md`](design-qa.md)。

开发者可从 [`docs/architecture/TECHNICAL_SOLUTION.md`](docs/architecture/TECHNICAL_SOLUTION.md) 了解完整技术栈、数据与媒体链路、关键难点、架构图和演进路线。

工程排障、架构取舍与可复用经验持续记录在 [`docs/engineering/LEARNING_LOG.md`](docs/engineering/LEARNING_LOG.md)。
