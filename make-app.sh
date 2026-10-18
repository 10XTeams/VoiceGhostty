#!/bin/bash
# 构建并打包成 macOS .app(麦克风权限弹窗需要 app bundle + Info.plist,直接跑裸二进制拿不到权限)。
#
#   ./make-app.sh              # release 构建(含 whisper Metal)、打包、关掉旧实例并启动
#   ./make-app.sh --no-open    # 只构建和打包(CI 用)
#   VG_FEATURES=              # 覆盖 cargo features,留空则不编 whisper(不需要 cmake)
set -euo pipefail
cd "$(dirname "$0")"

OPEN=1
[[ "${1:-}" == "--no-open" ]] && OPEN=0
FEATURES="${VG_FEATURES-whisper-metal}"

# 版本号只有 Cargo.toml 这一处;提交号注入二进制(build.rs 读 VG_GIT_COMMIT)并写进 plist。
# commit 后的 + 表示构建时带着未提交改动。
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
GIT_COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo unknown)
git diff --quiet 2>/dev/null || GIT_COMMIT="${GIT_COMMIT}+"
BUILD_STAMP=$(date +%Y%m%d%H%M)
export VG_GIT_COMMIT="$GIT_COMMIT"

if [[ -n "$FEATURES" ]]; then
    echo "▶ cargo build --release -p vg-app --features $FEATURES"
    cargo build --release -p vg-app --features "$FEATURES"
else
    echo "▶ cargo build --release -p vg-app"
    cargo build --release -p vg-app
fi

if [[ $OPEN == 1 ]]; then
    # 先杀掉正在运行的旧实例,否则 open 只会把旧进程切到前台(新二进制不会加载)
    echo "▶ 关闭正在运行的旧实例(如有)"
    pkill -f 'VoiceGhostty.app/Contents/MacOS/VoiceGhostty' 2>/dev/null || true
    sleep 0.5
fi

APP=build/VoiceGhostty.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/voiceghostty "$APP/Contents/MacOS/VoiceGhostty"
cp packaging/macos/Info.plist "$APP/Contents/"
cp packaging/macos/AppIcon.icns "$APP/Contents/Resources/"

/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $BUILD_STAMP" "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Add :VGGitCommit string $GIT_COMMIT" "$APP/Contents/Info.plist" \
    || /usr/libexec/PlistBuddy -c "Set :VGGitCommit $GIT_COMMIT" "$APP/Contents/Info.plist"
echo "▶ v$VERSION / build $BUILD_STAMP / commit $GIT_COMMIT"

# 本地开发用 ad-hoc 签名即可(必须在改完 Info.plist 之后,否则签名失效)
codesign --force --sign - "$APP"

echo "✅ 已生成 $APP"
[[ $OPEN == 1 ]] && open "$APP"
exit 0
