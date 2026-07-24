#!/bin/bash
# 构建并打包成 .app(权限弹窗需要 app bundle + Info.plist,不要直接 swift run)
set -euo pipefail
cd "$(dirname "$0")"

echo "▶ swift build -c release"
swift build -c release

APP=build/VoiceGhostty.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp .build/release/VoiceGhostty "$APP/Contents/MacOS/"
cp Resources/Info.plist "$APP/Contents/"
cp Resources/AppIcon.icns "$APP/Contents/Resources/"

# 本地开发用 ad-hoc 签名即可
codesign --force --sign - "$APP"

echo "✅ 已生成 $APP"
open "$APP"
