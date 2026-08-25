#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="$repo_root/docs/readme"
icon_path="$repo_root/public/app-icon.png"
latin_font="/System/Library/Fonts/Helvetica.ttc"
subtitle_font_dir="/System/Library/Fonts"
temp_dir="$(mktemp -d)"

cleanup() {
  rm -rf "$temp_dir"
}
trap cleanup EXIT

for command_name in ffmpeg ffprobe say; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "Missing required command: $command_name" >&2
    exit 1
  fi
done

if [[ ! -f "$icon_path" ]]; then
  echo "Missing app icon: $icon_path" >&2
  exit 1
fi

mkdir -p "$output_dir"

say -v Samantha -r 165 -o "$temp_dir/source.aiff" \
  "Your source video stays on your Mac. Yisheng Studio turns English speech into editable Chinese subtitles and dubbing."
say -v Tingting -r 185 -o "$temp_dir/localized.aiff" \
  "原始视频始终保留在你的 Mac。本地识别后，你可以校对中文字幕和配音，再导出成片。"

printf '%s\n' \
  '[Script Info]' \
  'ScriptType: v4.00+' \
  'PlayResX: 1280' \
  'PlayResY: 720' \
  '' \
  '[V4+ Styles]' \
  'Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding' \
  'Style: Default,Arial Unicode MS,30,&H00FFFFFF,&H000000FF,&HCC05080D,&HCC05080D,0,0,0,0,100,100,0,0,3,1,0,2,80,80,70,1' \
  '' \
  '[Events]' \
  'Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text' \
  'Dialogue: 0,0:00:00.40,0:00:08.80,Default,,0,0,0,,Your source video stays on your Mac.\NYisheng Studio turns English speech into editable Chinese subtitles and dubbing.' \
  > "$temp_dir/source.ass"

printf '%s\n' \
  '[Script Info]' \
  'ScriptType: v4.00+' \
  'PlayResX: 1280' \
  'PlayResY: 720' \
  '' \
  '[V4+ Styles]' \
  'Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding' \
  'Style: Default,Arial Unicode MS,28,&H00FFFFFF,&H000000FF,&HCC05080D,&HCC05080D,0,0,0,0,100,100,0,0,3,1,0,2,80,80,70,1' \
  '' \
  '[Events]' \
  'Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text' \
  'Dialogue: 0,0:00:00.40,0:00:08.80,Default,,0,0,0,,Your source video stays on your Mac.\N原始视频始终保留在你的 Mac。' \
  'Dialogue: 0,0:00:03.60,0:00:08.80,Default,,0,0,0,,Local recognition creates editable subtitles and dubbing.\N本地识别后，你可以校对中文字幕和配音，再导出成片。' \
  > "$temp_dir/localized.ass"

make_clip() {
  local audio_path="$1"
  local subtitle_path="$2"
  local title="$3"
  local status="$4"
  local accent="$5"
  local output_path="$6"
  local duration

  duration="$(ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 "$audio_path" | awk '{ printf "%.3f", $1 + 0.7 }')"

  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "color=c=0x070b12:s=1280x720:r=30:d=$duration" \
    -loop 1 -i "$icon_path" \
    -i "$audio_path" \
    -filter_complex "[0:v]drawbox=x=78:y=68:w=1124:h=584:color=0x0d1420:t=fill,drawbox=x=78:y=68:w=1124:h=6:color=$accent:t=fill[base];[1:v]scale=132:132[icon];[base][icon]overlay=x=574:y=142,drawtext=fontfile='$latin_font':text='$title':fontcolor=white:fontsize=44:x=(w-text_w)/2:y=308,drawtext=fontfile='$latin_font':text='$status':fontcolor=$accent:fontsize=22:x=(w-text_w)/2:y=376,subtitles='$subtitle_path':fontsdir='$subtitle_font_dir',format=yuv420p[v]" \
    -map "[v]" -map 2:a:0 \
    -c:v libx264 -preset medium -crf 26 \
    -c:a aac -b:a 128k \
    -movflags +faststart -shortest "$output_path"
}

make_clip \
  "$temp_dir/source.aiff" \
  "$temp_dir/source.ass" \
  "SOURCE VIDEO" \
  "ENGLISH AUDIO" \
  "0x94a3b8" \
  "$output_dir/demo-before.mp4"

make_clip \
  "$temp_dir/localized.aiff" \
  "$temp_dir/localized.ass" \
  "LOCALIZED OUTPUT" \
  "CHINESE DUB + BILINGUAL SUBTITLES" \
  "0x48a6ff" \
  "$output_dir/demo-after.mp4"

ffmpeg -hide_banner -loglevel error -y -ss 2 -i "$output_dir/demo-before.mp4" -frames:v 1 -q:v 3 "$output_dir/demo-before.jpg"
ffmpeg -hide_banner -loglevel error -y -ss 2 -i "$output_dir/demo-after.mp4" -frames:v 1 -q:v 3 "$output_dir/demo-after.jpg"

echo "README demo assets generated in $output_dir"
