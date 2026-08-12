# 信息架构与页面状态

## 固定导航

1. 项目库：最近项目、快速导入、项目状态与存储信息。
2. 编辑器：视频预览、片段检查器、时间轴、风险处理与导出入口。
3. 术语库：全局术语和项目覆盖。
4. 任务队列：唯一重型运行任务、等待任务、暂停与重试。
5. 服务商：翻译、TTS 连接与凭据管理。
6. 设置：运行时模型、缓存、隐私、诊断与更新。

处理阶段属于项目状态，不作为第二套导航。

## 项目状态机

`draft → queued → processing → waiting_user → processing → ready → exporting → completed`

任意运行状态可进入 `paused`、`failed` 或 `cancelled`。恢复后从最近一个已验证 Artifact 的下一阶段继续。

## 任务阶段

`media_check → proxy → extract_audio → separate? → transcribe → glossary_review → translate → text_review? → synthesize → align → mix → ready`

## 片段失效规则

| 修改内容 | 失效范围 |
|---|---|
| 原文 | 翻译、术语校验、配音、对齐、混音、字幕、导出 |
| 字幕译文（联动） | 配音、对齐、混音、字幕、导出 |
| 字幕译文（解除联动） | 字幕、导出 |
| 配音文案、声音、语速 | 配音、对齐、混音、导出 |
| 时间边界 | 对齐、混音、字幕、导出 |
| 源文件重新关联且指纹一致 | 不失效 |
| 源文件指纹变化 | 阻断，要求重新导入 |

