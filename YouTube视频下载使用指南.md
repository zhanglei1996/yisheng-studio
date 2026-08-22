# YouTube 视频下载使用指南（macOS）

本文档适用于当前这台 Mac，使用 `yt-dlp` 下载视频，并使用 `FFmpeg` 合并音视频、转换格式或截取片段。

> 请只下载自己拥有版权、已获授权，或许可允许保存的视频，并遵守 YouTube 服务条款及当地法律。YouTube Premium 的官方离线下载只能在 YouTube 内播放，不会生成普通 MP4 文件。

## 1. 当前环境

以下工具已经安装并可直接在“终端”中使用：

```text
yt-dlp  2026.07.04
FFmpeg  8.0.1
```

检查工具是否正常：

```bash
yt-dlp --version
ffmpeg -version
```

## 2. 最常用：下载为 MP4

打开 macOS 的“终端”，粘贴下面的命令。把最后的链接替换成要下载的视频链接即可：

```bash
yt-dlp -t mp4 --no-playlist \
  -P "$HOME/Downloads" \
  "https://www.youtube.com/watch?v=-MWqSD2_37E&t=304s"
```

完成后的文件位于 Finder 的“下载”文件夹。

参数说明：

- `-t mp4`：优先选择兼容性好的 H.264/AAC，并输出为 MP4。
- `--no-playlist`：即使链接来自播放列表，也只下载当前视频。
- `-P "$HOME/Downloads"`：保存到当前用户的“下载”文件夹。
- 链接必须放在英文双引号内，否则链接中的 `&` 会被终端错误解析。

> 链接里的 `t=304s` 只表示网页从 5 分 04 秒开始播放；默认仍会下载完整视频。

## 3. 常用下载方案

### 3.1 限制最高分辨率为 1080p

适合控制文件大小，同时保持较好画质：

```bash
yt-dlp --no-playlist \
  -f "bv*[height<=1080]+ba/b[height<=1080]" \
  --merge-output-format mp4 \
  -P "$HOME/Downloads" \
  "视频链接"
```

将 `1080` 改为 `720`，即可限制为最高 720p。

### 3.2 只下载音频

下载并转换为 WAV，适合语音识别、降噪和中文配音工作流：

```bash
yt-dlp --no-playlist -x --audio-format wav \
  -P "$HOME/Downloads" \
  "视频链接"
```

如果主要用于收听并希望节省空间，可改用 MP3：

```bash
yt-dlp --no-playlist -x --audio-format mp3 --audio-quality 0 \
  -P "$HOME/Downloads" \
  "视频链接"
```

### 3.3 下载字幕

下载上传者提供的字幕和自动字幕，并转换为 SRT：

```bash
yt-dlp --no-playlist --skip-download \
  --write-subs --write-auto-subs \
  --sub-langs "zh.*,en.*" --convert-subs srt \
  -P "$HOME/Downloads" \
  "视频链接"
```

查看某个视频有哪些字幕语言：

```bash
yt-dlp --list-subs "视频链接"
```

如果要同时下载 MP4 和字幕，删除命令中的 `--skip-download`，并添加 `-t mp4`。

### 3.4 只下载指定时间段

下载从 5:04 到 10:00 的片段：

```bash
yt-dlp -t mp4 --no-playlist \
  --download-sections "*00:05:04-00:10:00" \
  --force-keyframes-at-cuts \
  -P "$HOME/Downloads" \
  "https://www.youtube.com/watch?v=-MWqSD2_37E"
```

下载从 5:04 到视频结束：

```bash
yt-dlp -t mp4 --no-playlist \
  --download-sections "*00:05:04-inf" \
  --force-keyframes-at-cuts \
  -P "$HOME/Downloads" \
  "https://www.youtube.com/watch?v=-MWqSD2_37E"
```

`--force-keyframes-at-cuts` 会让切点更准确，但需要重新处理视频，因此速度会慢一些。

### 3.5 下载整个播放列表

只有在确实需要整个播放列表时才使用：

```bash
yt-dlp -t mp4 --yes-playlist \
  -P "$HOME/Downloads/YouTube/%(playlist_title)s" \
  -o "%(playlist_index)03d - %(title)s [%(id)s].%(ext)s" \
  "播放列表链接"
```

### 3.6 自定义文件名和保存目录

保存到当前项目的 `downloads` 目录，并在文件名中保留视频 ID：

```bash
yt-dlp -t mp4 --no-playlist \
  -P "./downloads" \
  -o "%(title)s [%(id)s].%(ext)s" \
  "视频链接"
```

如果目录不存在，yt-dlp 会自动创建。

## 4. 先查看信息，不实际下载

查看标题、时长和视频 ID：

```bash
yt-dlp --simulate --no-playlist \
  --print "标题：%(title)s" \
  --print "时长：%(duration_string)s" \
  --print "视频ID：%(id)s" \
  "视频链接"
```

查看可下载的全部画质和音轨：

```bash
yt-dlp -F "视频链接"
```

## 5. 登录、年龄限制或机器人验证

如果视频要求登录，可以读取本机浏览器里已有的 YouTube 登录状态：

```bash
yt-dlp -t mp4 --no-playlist \
  --cookies-from-browser chrome \
  -P "$HOME/Downloads" \
  "视频链接"
```

如果使用 Safari，把 `chrome` 改成 `safari`；如果使用 Firefox，则改成 `firefox`。

注意事项：

- 只在自己的电脑和账户上使用该参数。
- 不要把 cookies 文件或带登录信息的调试日志发送给别人。
- 浏览器正在运行时若读取失败，可完全退出浏览器后重试。

## 6. 网络连接与代理

如果出现以下错误，通常是本机到 YouTube 的网络连接问题，不是工具安装问题：

```text
Connection reset by peer
timed out
Unable to download webpage
```

先确认浏览器能够打开目标视频。如果本机已有 HTTP 代理，可在命令里指定代理地址。例如代理端口为 `7890`：

```bash
yt-dlp --proxy "http://127.0.0.1:7890" \
  -t mp4 --no-playlist \
  -P "$HOME/Downloads" \
  "视频链接"
```

SOCKS5 代理示例：

```bash
yt-dlp --proxy "socks5://127.0.0.1:7890" \
  -t mp4 --no-playlist \
  -P "$HOME/Downloads" \
  "视频链接"
```

端口必须以本机代理软件实际显示的端口为准。

## 7. 下载中断、限速与重试

下载被中断后，重新运行原命令即可尝试断点续传。网络不稳定时可以增加重试参数：

```bash
yt-dlp -t mp4 --no-playlist \
  --retries 10 --fragment-retries 10 \
  -P "$HOME/Downloads" \
  "视频链接"
```

限制下载速度为每秒 5 MB：

```bash
yt-dlp -t mp4 --no-playlist --limit-rate 5M \
  -P "$HOME/Downloads" \
  "视频链接"
```

## 8. 常见问题

### 为什么下载时会出现两条进度？

YouTube 的高画质通常把视频和音频分开提供。yt-dlp 会分别下载，然后调用 FFmpeg 自动合并，这是正常现象。

### 为什么最终格式不是 MP4？

使用文档中的 `-t mp4`。若手动选择的编码无法放进 MP4 容器，yt-dlp 可能选择其他容器；不要只把文件扩展名强行改成 `.mp4`。

### 为什么只有画面、没有声音？

不要只选择 `bestvideo`。使用 `-t mp4`，或使用带 `+ba` 的格式表达式，让 yt-dlp 同时下载音轨并通过 FFmpeg 合并。

### 为什么提示 `Video unavailable`？

视频可能已删除、设为私密、限制地区或需要登录。先在浏览器中确认同一网络和账户能正常播放，再按“登录”和“代理”章节处理。

### 如何取消正在进行的下载？

在终端中按 `Control + C`。重新运行同一命令通常可以继续下载。

## 9. 更新与卸载

本机的 yt-dlp 通过 `uv` 工具环境安装。更新命令：

```bash
uv tool upgrade yt-dlp
```

更新后检查版本：

```bash
yt-dlp --version
```

卸载 yt-dlp：

```bash
uv tool uninstall yt-dlp
```

FFmpeg 由 Homebrew 管理，更新命令：

```bash
brew upgrade ffmpeg
```

## 10. 针对当前视频的命令速查

完整 MP4：

```bash
yt-dlp -t mp4 --no-playlist -P "$HOME/Downloads" \
  "https://www.youtube.com/watch?v=-MWqSD2_37E&t=304s"
```

仅 WAV 音频：

```bash
yt-dlp --no-playlist -x --audio-format wav -P "$HOME/Downloads" \
  "https://www.youtube.com/watch?v=-MWqSD2_37E"
```

从 5:04 下载到结束：

```bash
yt-dlp -t mp4 --no-playlist \
  --download-sections "*00:05:04-inf" --force-keyframes-at-cuts \
  -P "$HOME/Downloads" \
  "https://www.youtube.com/watch?v=-MWqSD2_37E"
```

---

参考资料：

- [yt-dlp 官方项目与使用说明](https://github.com/yt-dlp/yt-dlp)
- [YouTube 官方离线观看说明](https://support.google.com/youtube/answer/7381437?hl=zh-Hans)
