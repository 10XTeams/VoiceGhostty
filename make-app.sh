#!/bin/bash
# 构建并打包成 .app(权限弹窗需要 app bundle + Info.plist,不要直接 swift run)
set -euo pipefail
cd "$(dirname "$0")"

echo "▶ swift build -c release"
swift build -c release

# 先杀掉正在运行的旧实例,否则 open 只会把旧进程切到前台(新二进制不会加载)
echo "▶ 关闭正在运行的旧实例(如有)"
pkill -f 'VoiceGhostty.app/Contents/MacOS/VoiceGhostty' 2>/dev/null || true
sleep 0.5

APP=build/VoiceGhostty.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp .build/release/VoiceGhostty "$APP/Contents/MacOS/"
cp Resources/Info.plist "$APP/Contents/"
cp Resources/AppIcon.icns "$APP/Contents/Resources/"

# 把构建时间 + 提交号写进 bundle,设置窗口会显示出来 —— 一眼就能确认跑的是哪个构建
# (marketing version 跨构建不变,单看它分不出新旧)。commit 后的 + 表示构建时带着未提交改动。
BUILD_STAMP=$(date +%Y%m%d%H%M)
GIT_COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo unknown)
git diff --quiet 2>/dev/null || GIT_COMMIT="${GIT_COMMIT}+"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $BUILD_STAMP" "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :VGGitCommit string $GIT_COMMIT" "$APP/Contents/Info.plist" \
    || /usr/libexec/PlistBuddy -c "Set :VGGitCommit $GIT_COMMIT" "$APP/Contents/Info.plist"
echo "▶ build $BUILD_STAMP / commit $GIT_COMMIT"

# 本地开发用 ad-hoc 签名即可(必须在改完 Info.plist 之后,否则签名失效)
codesign --force --sign - "$APP"

echo "✅ 已生成 $APP"
open "$APP"
