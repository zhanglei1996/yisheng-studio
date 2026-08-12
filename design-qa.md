# Design QA：译声工坊暗黑编辑器

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
