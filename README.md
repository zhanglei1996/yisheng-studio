# 译声工坊

本地优先的 AI 视频中文化桌面工具。目前已经跑通 macOS 上的真实纵向链路：本地视频导入 → FFmpeg 媒体准备 → whisper.cpp 英文识别 → DeepSeek/百炼兼容翻译 → macOS 系统中文配音 → 时间轴约束 → MP4/字幕/音轨导出。

## 已实现

- 暗黑剪辑工作台：真实代理视频播放、三标签片段检查器、四轨时间轴和动态风险提示。
- 项目库、新建项目四步流程、术语库、任务队列、服务商、组件/隐私设置和导出流程。
- 片段选择、文本编辑、字幕/配音联动、声音/对齐状态、拆分、合并、边界拖动、撤销/重做和局部重生成模拟。
- Tauri 2 Rust 边界、SQLite schema/migration、项目与任务持久化、片段非重叠校验、单重型任务互斥、暂停/继续/取消/重试、检查点与崩溃恢复、任务状态事件、Artifact 失效规则和 Keychain 凭据引用。
- 桌面端真实导入 MP4/MOV/MKV，使用 ffprobe 检查媒体并计算源文件指纹；原视频只做引用，代理视频、16 kHz 识别音频和任务检查点写入项目产物目录。
- whisper.cpp 使用 `small.en` 生成时间戳并落库为稳定 UUID 片段；真实 10:29 技术样片识别出 136 个不重叠片段。
- DeepSeek、阿里百炼和自定义 OpenAI 兼容翻译接口；批量 UUID 严格校验、格式异常回退和已完成片段复用。公开配置写入 SQLite，API Key 只写入 macOS Keychain。
- macOS Tingting 系统语音、本地静音裁剪、最高 1.15x 不变调调速、禁止片段互相侵入、超时风险标记和完整中文音轨混合。
- 导出 H.264/AAC 中文配音 MP4、中文/英文 SRT、中英双语 ASS 与 48 kHz 中文 WAV；任务可暂停、重试并在重启后从检查点继续。
- 产品、信息架构、隐私、设计规范、架构决策和许可证发布门禁文档。

浏览器预览仍使用固定数据；真实媒体处理仅在 Tauri 桌面应用内启用。当前开发机已经安装 whisper.cpp `v1.9.2`、`small.en` 与 FFmpeg。人声分离、讯飞/腾讯 TTS、运行时在线安装包、单片段真实重生成和正式签名/公证仍待实现。本机 FFmpeg 8.0.1 是启用 GPL 的 Homebrew 构建，仅用于开发检查；正式分发必须换成许可证门禁文档要求的 LGPL Sidecar。

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

```bash
pnpm typecheck
pnpm build
pnpm test:sites
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
pnpm tauri build --debug --bundles app
```

当前开发构建位于 `src-tauri/target/debug/bundle/macos/译声工坊.app`。请通过 `pnpm dev:app` 更新此构建，不要把 `pnpm dev:desktop` 运行中的临时开发壳当成可独立启动版本。API Key 在应用左侧“服务商”中配置；不要写入仓库配置文件。

视觉验收记录见 [`design-qa.md`](design-qa.md)。
