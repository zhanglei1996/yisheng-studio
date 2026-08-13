# Design QA：译声工坊暗黑编辑器

## 可发布配音：场景、音画编辑与发布门禁（2026-08-14）

- visual truth: `docs/design/references/editor-dark-reference.png`
- tested viewport: 1600 × 900，本地 Vite 浏览器预览
- tested surfaces: 编辑器四轨时间轴、音画同步入口、导出预检与发布预设

### 视觉与交互结果

- 保留参考稿的专业暗色剪辑器构图、紧凑密度和四轨结构；第一轨升级为“场景与画面”，同时容纳蓝色场景区、琥珀/红色同步锚点及可点击的裁剪/加速建议。
- “优化音画同步”使用 Phosphor `MagicWand` 图标与蓝色次级操作，不抢占整片重新配音和导出的主动作层级。
- 建议未采用时为琥珀虚线，采用后变为实线；裁剪采用红色语义，加速采用绿色语义，并同时通过文字、边框样式和 `aria-pressed` 表达，不只依赖颜色。
- 导出弹窗新增“便于分享 / 平衡 / 高画质”发布预设；预计体积随预设码率更新，不再固定按 6 Mbps 估算。
- 发布检查以阻断/提醒/信息三级呈现，可显示问题时间范围和建议动作；提醒须明确确认后才启用导出，阻断项不能绕过。
- 1600 × 900 截图检查中，时间轴工具栏、四轨标签、片段和 Inspector 无横向溢出；导出弹窗在首屏完整显示预检、路径、字幕、预设和输出包层级。

### 功能与可访问性检查

- DOM/辅助树确认“优化音画同步”、四条轨道、字幕片段、发布预设和风险确认均有可读名称。
- 浏览器点测：进入项目编辑器、打开导出弹窗、切换风险确认；关键按钮 disabled/confirmed 状态正确。
- 本轮浏览器预览没有桌面数据库，因此时间线使用演示项目验证布局；持久化分析、建议接受和 EDL 预览由 Rust 测试及真实媒体 FFmpeg 回归覆盖。

final result: passed — 未发现 P0/P1/P2 视觉问题；保持既有暗色设计真相的同时，新增同步建议和发布门禁闭环。

## 项目卡片视频首帧封面（2026-08-13）

- feedback evidence：用户提供的项目库截图中，真实 HSTS 项目错误显示了固定的 `RAG Pipeline` 课程图。
- 数据绑定：桌面端项目列表不再从演示 fixtures 取封面；每张卡片根据自身 `projectId` 调用本地首帧封面命令。
- 媒体处理：FFmpeg 从项目原视频的第一个解码视频帧生成 JPEG，宽度最大 960px，缓存到该项目的 `media/cover-first-frame.jpg`；原视频不复制、不上传。
- 历史项目：进入项目库时会为无封面的旧项目自动补齐；已有非空缓存直接复用，不重复抽帧。
- 异常状态：原视频缺失或解码失败时显示“正在生成视频首帧”的中性空态，不再回退为与项目无关的固定图片。
- 可访问性：真实封面的替代文本为“项目名 + 视频首帧”；空态使用既有 Phosphor `FileVideo` 图标和文字共同表意。
- 验证：真实 HSTS 源视频已生成 15,616-byte 首帧 JPEG，内容为视频开头的 `Practical Networking` 标题，与固定 `RAG Pipeline` 图明显不同；Rust 回归测试同时覆盖生成与缓存复用。

final result: passed

## 语义旁白同步模式真实验收（2026-08-13）

- evidence: `docs/design/qa/semantic-narration-2026-08-13/ready-editor.jpeg`、`export-complete.jpeg`
- tested app: 独立 macOS debug bundle；真实 HSTS 课程项目，136 个字幕片段，1920×1080，629.005 秒。
- providers: 阿里百炼 `qwen-plus` 负责语义改写；阿里 Qwen3 TTS `qwen3-tts-instruct-flash / Cherry` 负责中文配音。

### 方案与交互结果

- 新增“语义旁白同步”生成模式：不再逐句直译口播，而是先把相邻字幕组织成 30–60 秒语义场景，由大模型按画面顺序重写为自然中文讲解，再拆成 5–15 秒的同步语音块。
- 字幕译文继续保持原有 136 条和精确时间码，便于校对与烧录；口播文案独立保存，允许为自然表达调整句式和信息密度。
- 配音按短锚点回到视频时间轴，不把整章统一拉伸到总时长；每个语音块独立校验并适配目标窗口，兼顾连续表达与局部同步。
- 真实生成完成后编辑器统一显示“项目可导出”、已处理 136、待处理 0；预览可播放，导出预检明确显示配音与时长检查通过。
- 导出在 App 内完成，产出 H.264/AAC MP4、48kHz 独立中文 WAV、中文/英文 SRT 与双语 ASS；完成态提供查看保存位置。

### 声轨量化对比

- 新版独立中文音轨与视频均为 629.005 秒；MP4 为 1920×1080 H.264 + 48kHz mono AAC。
- 使用 `silencedetect=noise=-42dB:d=0.35`：用户此前认可的平衡块版本累计静音 124.80 秒、最长 3.47 秒；本版累计静音 84.48 秒、最长 3.18 秒，累计静音减少约 32%。
- 被否决的长章节版本存在 18.84 秒最长空白；本版没有超过 3.18 秒的空白。超过 1.5 秒的停顿共 16 处，均保留在短同步锚点之间，未再出现整章尾部的大段等待。

### 视觉与可访问性核验

- 对照 `docs/design/references/editor-dark-reference.png`，专业暗色编辑器、中央视频、右侧检查器和四轨时间轴的布局保持不变；语义模式没有牺牲高密度编辑能力。
- 就绪状态、处理数量和导出主操作使用图标、文字及绿色语义共同表达；不是只依赖颜色。
- 导出对话框初始焦点落在关闭按钮，进行中按钮有持续 loading，完成态为持久结果页面；未依赖短暂 Toast 告知关键结果。
- 通过 macOS 辅助功能树确认：模式状态、播放、片段、时间码、就绪数量、导出预检和完成动作均有可读名称。

final result: passed — 真实阿里云全流程生成、预览和导出完成；与上一可用版本相比，保持 629.005 秒同步锚点的同时显著减少累计静音，并消除了长章节方案的 18.84 秒失控空白。

### 4:23 双声回归修复

- 用户试听在 4:23 发现两种中文声音同时出现。定点检查确认：前一语音块窗口为 247.88–259.06 秒，但旧音频长 19.36 秒，实际延伸至约 267.24 秒；后一块从 259.06 秒开始，形成约 8.18 秒重叠。
- 根因是语义模式把块级生成成功直接视为可发布，跳过超窗 warning；底层 `amix` 允许重叠，因此没有在导出前暴露问题。
- 修复后，语义块与其他模式共用严格超窗判断；发布中文混音前新增相邻音频区间校验，任何大于 20ms 的重叠直接拒绝发布并给出可处理错误。
- 自动修复改为按整个声学块压缩，允许百炼在相邻字幕槽之间删除重复表达，字幕译文和稳定 ID 不变。真实项目经过多轮收紧后 136/136 ready。
- 最终当前缓存核验：52 个有效语音块，超窗块 0，最大边界误差 7ms。4:23 附近块变为 247.88–259.06 秒窗口 / 9.23 秒音频，下一块 259.06–270.96 秒 / 11.58 秒，不再重叠。
- 修复版在 App 内通过配音与时长预检并导出到版本化目录 `(5)`；MP4 为 629.005 秒、1920×1080 H.264 + AAC。

final result: passed — 4:23 双声根因已消除，且发布层新增全片不可重叠不变量，防止其他时间点复发。

## 阿里百炼 + 阿里 TTS 真实全流程验收（2026-08-13）

- evidence: `docs/design/qa/full-flow-alibaba-2026-08-13/01-tts-failure-blocker.png` 至 `08-export-complete.png`
- tested app: 刷新后的独立 macOS App；真实项目 136 个片段，1920×1080，10:29。
- providers: 翻译使用阿里百炼 `qwen-plus`；配音使用阿里百炼 TTS `qwen3-tts-instruct-flash` / `Cherry`。

### 实测路径

1. 从“136 个片段未成功生成配音”的阻断状态进入任务队列，查明首次局部生成缺少整片缓存。
2. 触发首次整片阿里 TTS，136/136 完成；129 个直接就绪，7 个进入时长处理。
3. 批量自动适配使用阿里百炼压缩口播稿并重新合成，自动解决 6 个，剩余 1 个。
4. 最后一个 4.08 秒短片段经受限不变调兜底适配成功；项目变为 136/136 可导出。
5. 播放中文合成预览后执行本地导出；输出 MP4、WAV、中文/英文 SRT 与双语 ASS。

### 发现与修复

- [P0] 首次生成缺少缓存时，界面把所有片段写成“失败”并引导逐个定位。现在识别“整片均 missing/stale 且无音频”状态，主操作改为“首次生成整片配音”，并解释完成后才可局部重试。
- [P0] `status=warning + tts_state=stale` 同时被算作失败阻断与时长提醒，导致编辑器和导出预检互相矛盾。现在 warning 统一为可处理/可知情导出的时长问题，真正 failed/missing/stale 才阻断。
- [P1] 最后一个 warning 没有 `ttsDurationMs`，Inspector 却显示“4.08 秒 / 适配良好”。现在缺失实测时长时明确显示“最新合成仍未适配成功”，时间轴写“时长待适配”，不再伪造 0.0 秒超出量。
- [P1] 常规 1.08x 仍无法容纳短口播时只能反复失败。新增短文案兜底，仅在 32 字以内且所需不变调加速不超过 1.25x 时启用；超过阈值继续保留 warning，避免牺牲自然度。
- [P1] 136 段合成过程中任务队列长期停在 80%。TTS 阶段现在从既有 80% 水位递增到 94%，并保留 `segment done/total` 检查点；自动适配结束补发 100% 结构化事件。
- [P2] 再次批量修复时仍显示上次“解决 6、剩余 1”的结果。开始新操作时先清空旧结果，完成后再展示本轮结论。
- [P2] 导出器会覆盖同名项目目录。本次验收先选择独立目录保护旧文件；实现随后改为自动选择“项目名 (2)/(3)”版本目录，默认不再覆盖历史成果。
- [P2] 导出弹窗硬编码“预计 1.36 GB”，实际整包约 337 MiB（视频 279 MiB + WAV 58 MiB + 字幕），误差约 4 倍。现在按项目时长、6 Mbps 视频码率、AAC 与 48kHz PCM 动态估算，并注明估算口径。

### 视觉与交互核验

- 阻断、时长提醒、就绪与导出完成均同时使用图标、文字和语义色；四轨暗色编辑器与 Phosphor Icons 保持不变。
- 批量修复持续显示“压缩口播稿 → 重新合成 → 校验时长”，完成后自动定位剩余问题；播放按钮实际进入暂停状态并推进时间码。
- 最终导出弹窗明确显示通过检查，输出目录可见；完成态提供“查看保存位置”。
- 输出验证：视频 H.264 + AAC 48kHz mono，1920×1080，629.005 秒，约 279 MiB；独立 WAV 为 PCM 48kHz mono，同为 629.005 秒，约 58 MiB。

final result: passed — 用户原项目已解除阻断，136/136 配音就绪；中文预览可播放；本地成片与全部伴随文件已成功导出到独立验收目录。

## 时长异常引导与导出预检（2026-08-13）

- editor evidence: `docs/design/qa/duration-guidance-1280x720.png`
- export evidence: `docs/design/qa/export-preflight-warning-1280x720.png`
- viewport: 1280 × 720
- 目标状态：一个时长超窗片段、编辑器持久任务横幅、琥珀色时间线标记、知情导出预检。

### 对照结果

- 参考审查图中分散在左侧“项目风险”的问题被提升为编辑器顶部持久任务横幅；横幅直接回答问题、影响与推荐下一步，主操作为“自动修复 1 个片段”。
- 点击“逐个检查”会选择问题片段、将时间线移动到片段并打开“对齐”面板；目标/实际时长、超出量、智能缩短、编辑口播稿与边界调整处于同一任务上下文。
- 问题片段同时使用琥珀边框、警告符号和“时长超出”文字，不只依赖颜色。
- 导出入口持续可达。仅有时长提醒时，导出预检明确显示数量和语音重叠风险，并提供“返回自动修复”与“仍然导出”；真正的配音生成失败仍由同一后端预检阻断。
- 批量自动修复使用“压缩口播稿 → 重新合成 → 校验时长”结构化进度；结果显示自动解决数与剩余数，可一次撤销，撤销后相关音频标记为待重新生成。

### 五项视觉检查

- Fonts and typography：任务标题 13px、正文 11–12px，关键数量与动作没有落入微型辅助字号。
- Spacing and layout rhythm：1280px 下横幅动作换至第二行，中央预览、四轨时间线和 480px Inspector 保持完整，无横向溢出。
- Colors and visual tokens：延续暗色专业剪辑器；蓝色主操作、琥珀时长提醒、红色阻断、绿色就绪语义一致。
- Image/icon fidelity：新增提示与时间线标记使用 Phosphor Icons；未新增 emoji、CSS 图标或手绘 SVG。
- Copy and content：明确“只修改口播稿，不改字幕译文”，并解释时长提醒允许知情导出、生成失败阻止导出。

### 交互与无障碍证据

- 已验证：持久横幅、逐个检查打开“对齐”、导出预检、风险确认门禁、时间线警告文本。
- 键盘命令在输入控件外生效：`Space`、`⌘Z` / `⇧⌘Z`、`⌘S`、方向键、`[` / `]`；输入框和可编辑区域会抑制全局快捷键。
- 批量处理结果区域使用 `aria-live`；焦点环、32×32px 关键点击区域、对话框焦点恢复交由组件库和显式样式共同覆盖；减少动态效果媒体查询已接入。
- 浏览器最终复验无应用 error/warning；视觉 QA 未触发真实在线配音或写入真实项目数据。

final result: passed

## 固定控件与无功能占位清理（2026-08-13）

- visual truth: `docs/design/references/editor-dark-reference.png`
- before evidence: `docs/design/qa/static-controls-audit-2026-08-13/01-editor-static-controls.png`、`02-settings-static-controls.png`
- after evidence: `docs/design/qa/static-controls-audit-2026-08-13/03-editor-after-cleanup.png`、`04-settings-after-cleanup.png`

### 审查结论与处理

- [P1] 文本检查器的“复制原文”“AI 精简”“恢复译文”外观可点击，但没有事件；现已接通复制、恢复译文，并把精简复用到真实的单片段时长适配链路，仅在超时且后端能力可用时显示。
- [P1] 设置页的模型下载、缓存容量、清理缓存、诊断包、遥测开关和四个分栏均是模拟数据或无事件控件；已删除假操作，只保留真实运行时查询、固定关闭遥测的数据边界和使用说明。
- [P2] 声音检查器的三条演绎轨按钮没有功能；已改为真实脚本文档统计和可用的“重新编排/自动导演”控制。
- [P2] 时间轴四轨右侧的扬声器、锁定图标以及“选择工具”按钮只是装饰；已移除假轨道操作，并将当前选择工具改成不可点击的状态标记。
- [P2] 预览速度 `1.0x` 原先无事件；现可循环切换 1.0x、1.25x、1.5x、0.75x，并同步到 video playbackRate。
- [P3] 术语库已有 Select 筛选，额外漏斗按钮没有事件且重复；已删除。
- [P3] 编辑器里的固定三条“本片段术语”和未持久化“上下文备注”容易误导；已删除，真实术语管理仍保留在术语库页面。

### Interaction and visual checks

- 浏览器点测预览速度从 `1.0x` 切换为 `1.25x`；复制和智能缩短入口存在，假备注、假术语和轨道假操作计数均为 0。
- 设置页点测：假分栏、下载、清缓存、诊断操作计数均为 0；真实“查看使用说明”入口存在。
- 1280×720 截图保持暗色专业剪辑器、四轨时间轴和原有密度；设置页去掉空导航后改为单列信息层级，无横向溢出。

final result: passed

## 配音同步字幕导出验收（2026-08-13）

- 导出面板明确标注“视频字幕（按当前配音时间同步）”，默认包同时列出配音同步 SRT 与忠实翻译 SRT，避免把自然口播稿和逐字翻译误认为同一时间轴。
- 视频烧录字幕改用实际 `spokenZh`，忠实翻译仍单独保存为 `中文字幕.srt`；中英双语模式也使用重新对齐的中文口播行。
- 真实 629.005 秒项目用 52 个当前语义配音块生成 136 条同步字幕。字幕起点相对旧源时间轴的中位移动为 169ms，最大前移 3.752 秒、最大后移 3.209 秒，说明旧时间码不能继续复用。
- 4:23 邻域由两条逐字字幕合为一条与真实口播一致的字幕，范围为 259.171–267.489 秒；未改变已验证的中文音轨。
- 导出面板、成功状态和默认包说明保持现有暗色层级、Ant Design 控件与紧凑密度；未改变编辑器四轨布局。

final result: passed

- source visual truth path: `docs/design/references/editor-dark-reference.png`
- implementation screenshot path: `docs/design/qa/editor-implementation-1600x900.png`
- source pixels: 1487 × 1058
- implementation pixels: 1600 × 900
- CSS viewport: 1600 × 900
- device scale factor: browser-reported 1.6; screenshot normalized to CSS pixels by the in-app browser
- state: 编辑器、第二个片段选中、文本检查器、风险片段可见

## Full-view comparison evidence

参考稿和实现截图已在同一轮 QA 中并列打开检查。实现保留了参考稿的主要构图：52px 暗黑顶栏、约 200px 左导航、中央课程预览、右侧三标签片段检查器、下方约半屏的四轨时间轴，以及蓝色选择/琥珀风险/绿色完成的状态语义。

参考图为更接近 1.4:1 的画布，实现验收窗口为 16:9，因此实现按产品约束保持时间轴完整可见，并将检查器宽度收敛到 480px。该差异属于响应式适配，不改变核心区域层级。

## Focused region comparison evidence

- 视频区：使用单独生成的 16:9 RAG 课程视频资产，未裁切参考 UI；课程标题、流程图、讲师位置和暗色光效与目标一致。
- 检查器：源文、字幕译文、配音文案、联动状态、术语表与局部重生成均可见；标签位置和细描边密度与目标一致。
- 时间轴：原始音频、背景声音、中文配音、双语字幕四轨完整显示；波形为 Canvas 数据视图，片段边界和播放头与目标一致。

## Findings

- P3：参考图的时间轴工具栏图标略大，当前实现为紧凑型 32px 控件；不影响功能或层级，可在后续像素级抛光中调整。
- P3：参考图顶部撤销/重做位于全局标题栏中间，实现将项目级撤销/重做放在编辑器动作栏，避免与非编辑器页面产生无效控制；这是有意的产品化偏差。

五个必查维度：

- Fonts and typography：系统 SF/PingFang 与 SF Mono 时间码正确，层级、字重、截断和换行无阻断问题。
- Spacing and layout rhythm：顶栏、侧栏、预览、检查器和时间轴比例稳定；1600×900 无页面滚动或隐藏的持久控件。
- Colors and visual tokens：背景、边框、主文字和蓝/绿/琥珀/红语义色与设计 Token 一致，对比度清楚。
- Image quality and asset fidelity：课程画面为独立 16:9 高分辨率资产；没有截图裁切、占位图、CSS 人物或手绘 SVG。
- Copy and content：中文产品文案和专业术语贴合视频中文化场景，没有将设计提示词暴露到产品 UI。

## Interaction and console evidence

- 已测试：主导航、片段选择、文本/声音/对齐标签、拆分片段、撤销、项目创建四步流程、版权确认门禁、导出与成功状态。
- 拆分后片段数由 4 变 5，撤销后恢复为 4。
- 版权未确认时“继续”按钮禁用，确认后启用。
- 浏览器控制台：0 条 error，0 条 warning。

## Comparison history

第一次实现即无 P0/P1/P2 问题，因此未触发阻断级修复循环。构建前已修正 TypeScript 静态资源声明和重复 package 配置，这些属于工程验证，不计入视觉 QA 迭代。

## Follow-up polish

- 若后续确定首发 MacBook 尺寸，可再针对 1440×900 的物理窗口做图标和检查器字距微调。

## Feedback iteration：原生标题栏、组件库与导出弹窗

- feedback source path: `docs/design/feedback/export-directory-before.png`
- revised implementation path: `docs/design/qa/export-dialog-antd.png`
- feedback source pixels: 1262 × 238
- revised dialog capture pixels: 775 × 846
- tested state: 编辑器打开导出弹窗，中文字幕选中，路径输入未聚焦

### Earlier findings

- [P1] 应用在内容层伪造 macOS 三色窗口按钮，无法提供真正的关闭、最小化和缩放行为。
- [P2] 顶部标题栏和编辑器动作栏各有一个导出按钮，入口重复。
- [P2] 输出目录由两个独立边框控件拼接，聚焦描边穿过按钮并发生视觉重叠。
- [P2] 自制表单控件缺少统一的 hover、focus、loading 和 disabled 质感。

### Fixes and post-fix evidence

- Tauri 改为系统装饰与 macOS Overlay 标题栏；内容层三色圆点已删除，标题栏预留 68px 给系统交通灯。
- 删除全局标题栏导出按钮，只保留编辑器动作栏中的项目级导出入口。DOM 验证按钮数量为 1。
- 导出弹窗迁移到 Ant Design Modal、Button、Input 和 Segmented，并用暗色 Theme Token 统一边框、圆角、焦点和加载状态。
- 输出目录改成一个完整 `Input` 容器：文件夹前缀、只读路径和尾部选择按钮共享同一边框。测量显示输入容器右边界 1064px，文本区右边界 1018px，尾部按钮从 1026px 开始，保留 8px 间距且无重叠。
- Tauri 已接入目录选择插件；浏览器预览保持路径不变，桌面端调用原生文件夹选择器。
- 改版截图中路径控件、字幕选择、结果包和底部操作形成稳定的 13px 节奏，未出现用户反馈截图中的双描边。

### Required fidelity surfaces

- Fonts and typography：继续使用系统 SF/PingFang；Ant Design Token 使用同一字体栈，正文和表单标签层级一致。
- Spacing and layout rhythm：弹窗统一 22px 内容边距；路径、分段选择与结果列表纵向节奏稳定。
- Colors and visual tokens：组件库暗色 Token 映射到既有背景、边框、主蓝与次文字颜色。
- Image quality and asset fidelity：本次调整没有新增或替换图像资产。
- Copy and content：保留原有导出术语和数据说明，仅消除重复操作入口。

### Interaction and console evidence

- 已验证：唯一导出入口、弹窗打开/关闭、字幕选项、导出 loading/完成状态、桌面端目录选择插件编译。
- 浏览器控制台：0 条 error，0 条 warning。
- TypeScript、Vite 构建、Rust 单测和 Clippy 均通过。

final result: passed

## 问题片段定位与时间线导航（2026-08-13）

- evidence: `docs/design/qa/timeline-horizontal-navigation-1280x720.png`
- viewport: 1280 × 720
- 项目风险卡将“定位问题”和“自动适配”拆成独立动作；定位会循环选择下一个问题片段，并打开声音或对齐页，不再要求用户在 136 段中目测搜索。
- 时间线工具栏增加左右移动按钮，同时支持触控板横向滚动与 `Shift + 滚轮`；选择问题片段仍会自动将它置于视窗附近。
- 实测连续左移后标尺从 05:01–05:22 变为 04:35–04:56，证明长项目可跨视窗导航；控制台为 0 条 error、0 条 warning。
- 新控件继续使用 Phosphor Icons，保持暗色编辑器密度和既有语义色。

final result: passed

## 音频连续性专项（2026-08-13）

- evidence: `docs/design/qa/tts-continuity-regenerate-confirm-1280x720.png`
- viewport: 1280 × 720
- 整片重新配音位于编辑器主操作栏，使用 Phosphor Waveform 图标，不改变四轨时间线和右侧 Inspector 的既有密度。
- 付费云服务的整片重跑只在用户显式发起时确认；确认内容显示实际片段数与费用提示，初次快速生成工作流仍会自动完成，不插入逐阶段确认。
- 应用内浏览器验证按钮、确认标题、动态片段数和取消操作均可见；控制台为 0 条 error、0 条 warning。未点击“开始生成”，避免在视觉验收中触发真实云端费用。
- 音频回归由 Rust 测试覆盖：安全边缘裁剪保留句中 400ms 自然停顿；过短返回会被完整性守卫拒绝并重试。

final result: passed

## 编辑器无项目空态（2026-08-13）

- feedback source: `docs/design/qa/editor-empty-feedback-1995x1248.png`（用户提供的无项目却显示演示编辑器状态）。
- [P0] 无项目时 EditorPage 继续显示 `Building Reliable AI Agents`、四轨演示数据、字幕表单和导出按钮，形成“存在可编辑项目”的错误心智模型。
- 修复：`projectId == null` 时进入专用编辑器空态，不挂载预览、时间轴和 Inspector，也不显示项目风险或导出动作。
- 主动作：中央“新建项目”直接打开既有创建流程；顶部保留“选择项目”，便于将来从项目列表切换。
- 信息层级：视频、声波、AI sparkle 使用现有 Phosphor Icons；三条步骤说明首次工作流；底部明确原视频本地与第三方文本传输边界。
- 响应式：桌面横向三步，窄屏改为纵向；空态继续使用编辑器三行 Grid，不影响状态栏和侧边导航。

final result: passed

## 一次性云配音工作流与错误可见性（2026-08-13）

- 反馈证据：`docs/design/qa/workflow-audit-2026-08-13/01-create-system-only.png`、`02-blank-toast-and-glossary-stop.png`、`03-tts-failed-without-reason.png`。
- 审计路径：新建项目 → 服务配置 → 自动生成 → 任务队列 → 中断恢复；反馈截图为 1995×1248，独立 macOS App 为本地 debug bundle。

### Findings and fixes

- [P0] “自动生成”只执行媒体准备，并在术语检查点停下，实际不是自动流程。现在创建后连续执行媒体准备、本地识别、翻译、口播导演与配音；仅“先校对口播稿”会在云配音前等待用户。
- [P0] TTS 临时文件以 `.wav.pending` 结尾，FFmpeg 无法推断输出封装格式，导致 57% 中断。临时文件改为 `.pending.wav`，标准化命令同时显式指定 WAV 输出格式。
- [P1] 创建页只能选系统 Tingting。现在优先列出已配置且已保存凭据的高级云 TTS，并按服务商目录选择项目默认声音；系统语音保留为本地免费后备。
- [P1] Ant Design 静态 Toast holder 没有继承暗色主题，产生“白条无内容”。静态 holder 与应用共用暗色 token、中文 locale，并限制同时显示最多 3 条；CSS 增加暗色兜底。
- [P1] 队列只显示检查点，不显示 `errorMessage`。现在卡片内显示“中断原因”、可行动说明以及具名的“重试”入口；Keychain 错误明确说明解锁后直接重试，无需重新输入密钥。
- [P2] 创建页与编辑器的声音选择语义重叠。现在创建页定义“项目默认配音引擎/声音”（首次整片生成）；编辑器明确为“项目默认声音”的后续设置，片段按钮仍是显式“重新生成本片段”。

### Verification

- `pnpm typecheck`、`pnpm build` 通过。
- Rust 51 个库测试、4 个 harness 测试、7 个 provider contract 测试通过；1 个依赖外部媒体样本的既有测试按预期 ignored。
- `cargo clippy --all-targets --all-features -- -D warnings`、`git diff --check` 通过。
- 本地 debug `.app` 打包成功；未部署 Sites。

final result: passed — 自动模式不再停在术语确认；高级语音可在创建时作为项目默认；Toast、失败原因与安全重试路径已收口。

## 项目身份、配音来源与删除管理（2026-08-13）

- [P1] 同名源文件生成的项目卡片与队列记录无法区分。确认创建前现在提供“项目名称”输入，默认取源文件名；项目卡片菜单支持随时重命名。
- [P1] 项目和任务只有取消动作，没有生命周期清理。项目菜单新增危险确认删除，级联清理字幕、任务、产物记录以及 App 内的代理/配音/预览，明确保留原始视频；运行中项目必须先取消。非运行任务可单独删除，项目与产物继续保留。
- [P1] 队列无法确认实际合成设置。每条任务现在显示“配音：服务商 · 模型 · 音色”，数据来自项目当前 TTS 默认与服务商目录，不显示密钥。
- 删除、重命名继续复用 Ant Design Modal/Dropdown/Popconfirm 与 Phosphor Icons，沿用深色紧凑布局和红色危险语义。
- `pnpm verify:full` 覆盖 TypeScript、Vite、Rust、Clippy/Fmt；新增数据库测试验证重命名、运行中删除保护与项目删除对任务的级联。

final result: passed

## 方案三：口播稿内联编排与双云 TTS（2026-08-13）

- selected design reference: `/Users/mac/.codex/generated_images/019ff649-aa95-74b0-82ed-ecab11a60b90/exec-25950c6a-9257-45fc-8d86-e106c6fea292.png`
- combined visual comparison: `docs/design/qa/tts-voice-reference-comparison.png`
- editor captures: `docs/design/qa/tts-voice-editor-1440x900.png`, `docs/design/qa/tts-voice-editor-1280x720.png`
- credential form captures: `docs/design/qa/provider-aliyun-form-1440x900.png`, `docs/design/qa/provider-iflytek-form-1440x900.png`
- real credential verification capture: `docs/design/qa/provider-real-credentials-verified-1440x900.jpg`
- source pixels: 1672 × 941
- implementation capture pixels / CSS viewports: 1440 × 900 and 1280 × 720；保存截图与对应 CSS viewport 为 1:1
- combined comparison pixels: 2880 × 960；源图与 1440 × 900 实现图在同一画布中归一化并列检查
- tested state: 第二个片段选中、声音检查器、结构化口播稿、自动导演开启；阿里百炼与讯飞超拟人新增服务商表单。

### Reference comparison

- 延续参考稿的暗黑专业剪辑构图：中央预览、左侧四轨时间轴、右侧声音 Inspector 跨预览与时间线两行；蓝色用于选择/主操作，绿色用于保护与就绪状态，琥珀用于停顿与时长风险。
- 右侧信息层级与参考一致：项目统一音色 → 字幕译文/口播稿 → 内联强调与停顿 → 演绎轨 → Auto + 五种显式风格 → 高级参数 → 试听/片段生成。
- 1440×900 下所有核心编排内容完整可见且底部操作保持固定；1280×720 下正文区域按设计滚动，试听与生成操作仍固定可达，没有横向溢出或遮挡。
- 实现未复刻参考中的人物视频帧，继续使用本地媒体/代理占位状态；这是数据状态差异，不是布局偏差。

### Provider and privacy interaction checks

- “添加服务商”预设中可选择阿里百炼 TTS（Qwen / CosyVoice）与讯飞超拟人语音。
- 阿里表单包含地域、模型、音色与 DashScope API Key；讯飞表单包含 AppID、VCN，以及 APIPassword 或 APIKey + APISecret 两种鉴权。
- 密钥输入明确标注只写入 macOS Keychain；在线 TTS 明确只发送当前待合成中文文案与语音参数，不上传原视频、原始音轨或完整工程。
- 浏览器预览未录入或传输任何真实凭据；真实连接测试仅在本地 `.app` 中进行。

### Visual and accessibility findings

- Fonts and typography：SF/PingFang 与 SF Mono 时间码保持原设计层级；10–12px 紧凑编辑器信息仍可辨识。
- Spacing and layout rhythm：480–640px 响应式 Inspector、紧凑演绎轨与 105px 固定操作区形成稳定节奏。
- Colors and visual tokens：沿用既有 Token，无新增不一致色板；停顿、保护、选择、失败语义清晰分离。
- Image/icon fidelity：所有可见操作图标继续使用 Phosphor Icons，没有 emoji、CSS 图标、手绘 SVG 或占位资产。
- Copy and content：供应商能力按实际接口呈现；讯飞六风格被定义为应用导演层，未冒充服务商原生情绪能力。

### Interaction and console evidence

- 已验证：文本/声音标签切换、结构化口播标记显示、项目音色选择、自动导演开关、六风格选择、服务商预设切换及两家密钥表单字段。
- 首轮浏览器日志仅有 Ant Design `Alert.message` 弃用提示；服务商页改为 `title` 属性后，用全新标签页复验为 0 条 error、0 条 warning。
- 上述截图状态的并列视觉比较没有发现可操作的 P0/P1/P2 差异；既有截图证据继续有效。
- 刷新后的独立 `.app` 已验证服务商导航、加载/成功态、真实 Keychain 读取，以及阿里百炼与讯飞最小合成连接测试；两张服务卡均显示“已验证”。实际结果：阿里约 761ms、讯飞约 1046ms，截图未包含任何凭据明文。
- TTS smoke harness 已接入 Keychain、脱敏输出和 120 秒硬超时；命令行进程身份的 Keychain 读取曾超时，桌面端现改用有界系统 Keychain 读取并增加服务商测试超时，界面不再无限旋转。
- 口播标记循环、输入法安全提交和失败片段重试已有静态与构建验证；本轮未人为制造云端失败以补拍失败卡，因此保留为后续回归状态，不阻断所选主流程。
- 已定义 `verify:fast`、`verify:full`、`verify:release` 三级本地验收；未做 Sites 部署，本轮只使用本地预览和本地桌面工作流。

final result: passed — 独立 `.app` 已完成两家真实凭据的最小合成连接验证；全片音质盲测属于下一阶段产品评测。

## 桌面 CSP 样式回归修复（2026-08-12）

- 故障证据：Tauri Web Inspector 报告 Ant Design 的运行时样式被 `style-src` 拒绝；业务 CSS 正常，因此表现为整体布局保留、按钮/分段控件/搜索框等组件退回原生样式。
- 修复范围：复用 Tauri 注入到启动样式的 nonce，并传递给页面 `ConfigProvider` 与静态浮层 holder；未关闭或放宽 CSP。
- 验收要求：独立 `.app` 中 Ant Design 动态 `style[data-css-hash]` 均带 nonce，控制台无样式 CSP 拒绝，项目库与编辑器视觉恢复。

final result: passed — independent `.app` verified with 39 Ant Design runtime styles, all carrying the same Tauri nonce, and zero console errors.

## 性能专项：编辑器保活与后台预览（2026-08-12）

- 视觉范围：未改变专业暗黑编辑器构图、四轨密度或蓝/绿/琥珀/红状态语义；新增逻辑仅影响挂载、缓存与后台任务时序。
- 菜单往返：编辑器首次访问后保留现有 DOM 和会话状态，非编辑器路由使用 `display: none` 隐藏并暂停视频；返回时无需重建视频、两张波形 Canvas 和 Inspector。
- 中文预览：已有合成文件仍显示绿色“中文合成预览”；过期或缺失时立即显示原始代理状态，首帧后后台生成并自动切换，不再用长时间占位阻塞编辑器。
- 组件语义：所有可见图标继续使用 Phosphor Icons，没有新增文字字形、emoji、CSS 图标或手绘 SVG。
- 响应式与可访问性：保活容器沿用编辑器 100% 高度；隐藏时设置 `aria-hidden`，视频自动暂停，不产生后台播放。

final result: passed

## 反馈迭代：启动反馈、中文合成预览与 App 图标（2026-08-12）

- visual truth: `docs/design/references/editor-dark-reference.png`
- app icon master: `src-tauri/icons/app-icon-master.png`（1024 × 1024）
- semantic palette: graphite/navy base, blue video/localization loop, green dubbed-audio state, amber playback focus.

### Findings and fixes

- [P1] 编辑器预览固定引用原始 `preview-proxy.mp4`，即使中文音轨已生成也仍播放英文；已切换为后端解析的合成预览源，并提供可见来源状态。
- [P1] 开发预览入口没有即时产品反馈，且只允许 `terminal.local` Host；已增加首屏品牌启动状态、主入口预热与 `preview.local` / localhost 兼容。
- [P2] 原有 `src-tauri/icons/icon.png` 实际是课程视频帧，不是产品图标；已替换为完整跨尺寸品牌 iconset，并写入 Tauri bundle 配置。

### Visual and interaction checks

- 图标在 32px、128px、512px 下保留清晰的画框、声波与播放三层轮廓；无文字、emoji、手绘 SVG 或不相关课程画面。
- 预览来源徽标位于画面左上，不遮挡底部双语字幕；绿色表示中文合成，琥珀表示原始代理等待配音。
- 启动画面沿用现有深色表面、蓝色主动作和紧凑密度，不改变编辑器四轨布局。

final result: passed

## Feedback iteration：交通灯垂直对齐与全局基础组件统一

- feedback source path: `/var/folders/m9/dnqp68_s64zdvsjq8hlw8v0c0000gn/T/codex-clipboard-22247267-7d45-4148-8fcb-1a827b09e029.png`
- project library capture: `docs/design/qa/library-antd-unified.png`
- editor capture: `docs/design/qa/editor-antd-unified.png`
- CSS viewport: 1600 × 900

### Findings and fixes

- [P2] macOS 原生交通灯沿用默认 Overlay 坐标，中心线高于 52px 产品顶栏的 26px 中心。Tauri 窗口现在显式使用 `trafficLightPosition: { x: 13, y: 17 }`；18px 控件的垂直中心为 26px。
- [P2] 项目库、编辑器检查器、队列、服务商、术语库、设置、首次引导和新建项目仍混用原生控件与 Ant Design，焦点、加载、禁用、危险确认和密度不一致。现已迁移通用 Button、Input/TextArea、Select、Checkbox/Radio、Segmented、Switch、Slider、Progress、Tooltip、Popconfirm、Modal、Steps 和 Empty。
- [P3] 时间轴片段仍使用原生按钮，因为它承担绝对定位、边界拖动与选段语义，属于编辑器专用交互而非基础组件；时间轴工具按钮与缩放已使用 Ant Design。

### Post-fix evidence

- 项目库检测到 19 个 Ant Design 按钮、1 个分段控件、1 个搜索输入；仅拖拽导入容器保留原生 button 语义，无横向溢出。
- 编辑器检测到 30 个 Ant Design 按钮、1 个检查器分段控件、3 个 Ant Design 文本域和 1 个缩放滑杆；剩余原生按钮均为时间轴片段或产品专用交互。
- 术语库、队列、服务商和设置页面均无横向溢出；队列和服务商页面基础按钮已经全部迁移。
- 已验证声音标签页的两个 Select、语速 Slider、试听按钮，以及导出弹窗的 Input、Segmented 和 Modal。
- TypeScript、Vite、Sites 测试、Rust 单测与 Clippy 全部通过。

final result: passed
# Guided duration-exception flow — 2026-08-13

- Added a persistent editor task banner that explains the timing problem, export impact, recommended automatic fix, and manual-review fallback.
- Batch fitting now reports structured `compressing`, `synthesizing`, and `validating` progress and returns resolved/remaining counts.
- A remaining warning selects the first exception and opens the alignment inspector; the inspector presents smart shorten, edit script, and adjust-boundary actions in order.
- Export distinguishes blocking TTS failures from advisory timing warnings. Warnings require explicit acknowledgement; failed/stale audio is rejected again by the backend.
- Project library, queue, sidebar, editor header, and export modal derive their labels from the same readiness semantics.
- Timeline warning clips now use amber border, background, icon text, and accessible labels rather than color alone.
- Added keyboard navigation and save feedback: Space, Cmd/Ctrl+Z, Shift+Cmd/Ctrl+Z, arrows, bracket issue navigation, and Cmd/Ctrl+S; inputs suppress editor shortcuts.
- Added visible focus rings, `aria-live` status messages, 32–36px task targets, and reduced-motion handling.

## Visual acceptance targets

- Editor at 1995×1248: banner remains fully visible without covering the preview; primary automatic-fix action is visually dominant.
- Editor at 1280×720: banner copy and actions remain readable; timeline and inspector retain usable height.
- Export modal at 1440×900: blocking, warning, and ready states each expose one clear next action.
- Project cards and queue rows use matching counts and wording for the same persisted project.
# 平衡模式连续配音验收（2026-08-13）

- 视觉基准：继续沿用 `docs/design/references/editor-dark-reference.png` 的专业暗色编辑器、四轨时间轴与蓝/绿/琥珀/红语义色。
- 新增入口：声音面板增加“平衡模式 / 严格同步”，说明会影响的字幕条数、语音块时长和云端数据范围。
- 任务流：切换模式后明确提示需要重新生成整片；生成期将 136 条字幕聚合为 52 个语音块，并在编辑器横幅持续显示 `已完成/总数`。
- 真实验收：阿里百炼 `qwen3-tts-instruct-flash / Cherry` 完成全片，2 个超时片段经自动适配后归零，导出预检通过。
- 产物对比：相对逐句模式，`>=300ms` 静音从 171 降至 136，`>=500ms` 静音从 110 降至 81；总视频时长仍为 629.005 秒。
- 兼容修正：历史 Rust 脚本 JSON 中的 `duration_ms` 在前端读取时规范化为 `durationMs`，避免停顿标记显示 `undefined/NaN`。
- 当前截图：`docs/design/qa/balanced-flow-alibaba-2026-08-13/04-final-balanced-ready.png` 与 `05-export-preflight-ready.png`。
- 仍需人工判断：画面静止时不能证明声学自然度，已在实际 App 中播放检查；不同模型/音色仍建议做盲听评分。

## Realtime 长章节实验结论（2026-08-13）

- 实验入口：阿里百炼 Qwen3-TTS Realtime，同一会话持续追加约 60–100 秒章节，136 条字幕合并为 7 个章节。
- 正向结果：章节内短停顿与声音状态更连续，生成与导出链路均可完成。
- 否决原因：真实试听出现明显音画语义漂移；长章节内的速度误差会持续累积，画面进入下一知识点时旁白仍在上一段。
- 量化旁证：实验成片出现 7 个 `>=2s` 静音，最长约 18.85 秒。继续用整章变速只能对齐总时长，无法恢复句意与画面的局部对应，因此不作为修复方案。
- 产品决策：重新将“平衡模式（5–15 秒锚点）”设为推荐和新建云项目默认；长章节入口从用户界面撤下。当前真实项目已恢复平衡模式、重新生成 52 个语音块，并自动解决剩余 1 个时长提醒。
- 后续方向：保持 5–15 秒画面锚点，在块边界做上下文续接、响度/音色一致化与短交叉淡化；不得用扩大自由漂移窗口换取连续性。

final result: rejected for product use — sound continuity improved, but semantic audio-video synchronization regressed.

# 讯飞跨供应商链路验收（2026-08-13）

- 视觉基准未变：继续保留专业暗色编辑器、四轨时间轴和既有语义色；本轮没有新增布局或图标体系。
- 服务商页的连接测试现在承担“当前 VCN 真实可合成”的语义。讯飞发音人按账号单独授权，静态音色营销列表已移除，避免把未授权音色展示为可选项。
- 项目从阿里切到讯飞时，旧片段 `Cherry` 覆盖会被清除；界面中的当前服务、数据发送说明与真实请求保持一致。
- 语义旁白文案已改成“双服务分工”：阿里百炼接收场景文字/字幕做改写，当前 TTS 服务只接收中文口播稿与参数；原视频和原声不上传。
- 真实交互结果：`x7_susu_pro` 暴露发音人授权错误；`x6_lingxiaoxuan_pro` 短句验证成功，并完成 52/52 个整片语音块。32 个时长提醒已进入既有自动修复流程，锁屏前没有出现生成失败或双声重叠。
- 可访问性：错误不再只靠红色状态；文本说明包括错误类型、推荐操作和“未继续重复请求”。

## 完整结果

- 两轮自动适配分别解决 29/32 和 3/3 个讯飞时长问题，最终 136/136 ready、52/52 当前语音块与依赖哈希一致，最大块时长比画面窗口短 248ms，没有跨块重叠。
- 导出预检为 ready，实际导出 629.005 秒 MP4、48kHz WAV 和 136 条配音同步字幕；成片只有一条 AAC 音轨。
- 导出完成后发现预览媒体刷新会把 video 重置到 0 秒但保留旧时间轴读数；已在 `loadedmetadata` 恢复 Zustand 中的播放头，避免播放时突然跳回开头。
- 讯飞相对阿里：`>=300ms` 静音 105 vs 126、累计 137.19s vs 142.61s；`>=1s` 静音累计 112.14s vs 101.57s。讯飞短停顿更少，但长空档更集中。
- 两次语义稿字符数接近（2556 vs 2576），但文本序列相似度仅 0.769，因此本轮成片是端到端方案对比，不应把全部听感差异归因于 TTS 音色。

final result: passed — iFLYTEK provider switching, entitlement validation, fitting, playback, preflight and real export completed; Alibaba remains the recommended default based on pacing and native direction support.

## 项目库信息降噪与弹窗统一（2026-08-13）

- source visual truth: `docs/design/qa/project-library-cleanup-2026-08-13/01-before-header.png`、`02-before-title.png`、`03-before-duplicate-entry.png`、`04-before-modal.png`
- implementation screenshots: `docs/design/qa/project-library-cleanup-2026-08-13/05-after-library.png`、`06-after-modal.png`、`08-after-step-2.png`、`09-after-step-3.png`、`10-after-step-4.png`
- combined comparison: `docs/design/qa/project-library-cleanup-2026-08-13/comparison-contact-sheet.png`
- viewport and density: before library 2880 × 1800 @2x（归一化为 1440 × 900）；before modal 1676 × 1266（等高归一化并置于 1440 × 900 画布）；implementation 1600 × 900，Browser CSS viewport 1600 × 900、device pixel ratio 1。
- state: 项目库默认态、打开“新建项目”第一步，以及生成方式、服务配置、确认开始三步。

### Full-view comparison

- [passed] 顶部项目切换、自动生成、本地处理和队列四项已从画面移除，仅保留 28px 原生窗口拖动区；主导航和工作区从第一屏开始。
- [passed] 项目库及术语库、任务队列、服务商、设置页面都只保留一个 `h1`；页面级新增/帮助动作仍位于同一基线。
- [passed] 项目库大面积拖入入口已删除，新建项目只保留右上角按钮和弹窗，筛选、搜索、项目卡随之上移。
- [passed] 新建项目弹窗从“眉题 + 当前步骤标题”收敛为单一“新建项目”，内容宽度由 680px 保留但垂直高度显著收紧，四步均无裁切或横向溢出。

### Focused region comparison

- 标题与页头：系统字体、25px 页面标题和右侧 42px 主按钮形成清晰单层级；删除的描述文案不再占据上方垂直节奏。
- 弹窗：17px 单标题、15/20/13px header padding、18/20px body padding、11/20px footer padding；关闭、文件、模式、目录、完成状态统一使用 Phosphor Icons。
- 拖拽：第一步同时绑定浏览器 DOM `dragenter/dragover/dragleave/drop` 与 Tauri `Webview.onDragDropEvent`；进入态使用蓝色边框/底色和“松开以导入视频”，并在探测前校验支持的扩展名。`dragDropEnabled` 在 Tauri 窗口配置中显式开启。
- 图像：项目卡继续使用视频首帧，没有引入占位封面或低清替代；此轮没有新增图像资产。
- 文案：第一步主动作统一为“拖入视频或点击选择”，导出弹窗主标题统一为“导出视频”，没有重复 title。

### Interaction and accessibility checks

- Browser 逐步通过 1 → 2 → 3 → 4；文件选择、权利确认、上一步/继续、关闭按钮和各表单控件均可访问。
- DOM 可见结构确认所有主要页面各只有一个 `h1`；新建项目弹窗暴露 `dialog` 语义和可访问名称。
- 拖拽的 DOM 回退在浏览器原型中接受文件；Tauri 桌面实现使用官方路径事件并交给同一 `loadPath` 媒体探测。当前 Browser 无法制造带本地绝对路径的原生 macOS Finder 拖拽，因此桌面行为由 Tauri API 类型检查、显式窗口配置和编译验证覆盖。

### Comparison history

- Earlier P1：头部四项静态控件、三行页头和第二个导入入口造成主任务重复。Fix：移除旧头栏与 quick-import，统一为单标题和单入口。Post-fix evidence：`05-after-library.png`。
- Earlier P1：弹窗 title 重复、留白过大、拖入无事件处理。Fix：新增 `AppModal`，统一 header/body/footer、Phosphor 关闭图标，并实现 DOM + Tauri 双通道拖拽。Post-fix evidence：`06-after-modal.png` 以及 `08`–`10` 各步骤截图。
- Earlier P2：重命名、删除、导出、首次引导、服务商、术语库弹窗视觉不一致。Fix：所有完整 Modal 和删除确认均复用 `AppModal`；导出图标改为 Phosphor。Post-fix evidence：组件级检查与 Browser 流程无视觉漂移。

### Findings

- 无剩余 P0/P1/P2。P3：浏览器预览中的 Ant Design Alert 仍使用框架自带状态图标；它们语义正确且不影响本轮交付，可在后续全局图标专项统一。

final result: passed
