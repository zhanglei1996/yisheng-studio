# 译声工坊视觉验收

## 当前视觉基线

- 产品：macOS 本地优先 AI 视频中文化桌面应用。
- 视觉基准：`docs/design/references/editor-dark-reference.png`。
- 必须保持：专业暗色视频编辑器、紧凑信息密度、三栏工作区、四轨时间轴，以及蓝 / 绿 / 琥珀 / 红语义色。
- 图标：产品界面统一使用 Phosphor Icons；不使用 emoji、文本字符、CSS 绘图或临时 SVG 代替可见图标。

## 开源仓库保留的视觉证据

- 编辑器主界面：`docs/readme/editor-workbench.jpeg`。
- 项目库：`docs/readme/project-library.png`。
- README 英文原片形态：`docs/readme/demo-before.jpg`、`demo-before.mp4`。
- README 中文成片形态：`docs/readme/demo-after.jpg`、`demo-after.mp4`。
- 应用图标主文件：`src-tauri/icons/app-icon-master.png`。

README 演示素材由 `scripts/generate-readme-demo.sh` 使用仓库自有文案与 macOS 系统语音生成，不依赖或再分发第三方测试视频。

## 验收清单

- [x] 1440 × 900 桌面基线下，左侧导航、中央预览、右侧检查器和底部四轨时间轴结构清晰。
- [x] 项目库保持单一主入口、紧凑卡片密度和明确的处理状态。
- [x] 蓝色表示主操作或选中，绿色表示就绪，琥珀色表示风险，红色表示阻断。
- [x] 原视频、字幕、背景声和中文配音在时间轴上有独立语义，不依赖颜色作为唯一状态线索。
- [x] README 截图不展示 API Key、Cookie、用户路径或其他敏感信息。
- [x] README 对比素材具备可重复生成脚本，且不包含来源不明的视频画面或音频。

## 维护方式

历史过程截图已从开源仓库中清理，只保留当前基线和 README 所需的代表性证据。后续产品设计变更应：

1. 与 `docs/design/references/editor-dark-reference.png` 做视觉对照；
2. 更新本文件中的基线说明与代表性截图；
3. 运行 `pnpm verify:full` 并完成相关交互检查；
4. 删除被新截图取代的旧图片，避免重新积累过程性资产。
