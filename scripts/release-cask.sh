#!/bin/bash
# 发布后同步 Homebrew cask:从 GitHub Release 拉下 zip、算 sha256、更新 cask 定义,
# 并(可选)推送到 tap 仓库 10XTeams/homebrew-tap。
#
#   ./scripts/release-cask.sh v0.1.0           # 只更新本仓库里的 cask 定义
#   ./scripts/release-cask.sh v0.1.0 --push    # 顺带推到 tap 仓库
#
# 前置:release 已经建好且 zip 资产已上传(打 tag 后 CI 会做),本机 gh 已登录。
set -euo pipefail
cd "$(dirname "$0")/.."

TAG="${1:-}"
PUSH="${2:-}"
TAP_REPO="10XTeams/homebrew-tap"
CASK_SRC="packaging/homebrew/voiceghostty.rb"

if [[ -z "$TAG" ]]; then
    echo "用法: $0 <tag> [--push]     例: $0 v0.1.0 --push" >&2
    exit 1
fi
VERSION="${TAG#v}"   # v0.1.0 -> 0.1.0

echo "▶ 拉取 release 资产 $TAG"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
gh release download "$TAG" --pattern '*.zip' --dir "$WORK"

ZIP=$(find "$WORK" -name '*.zip' -maxdepth 1 | head -1)
[[ -n "$ZIP" ]] || { echo "✗ release $TAG 里没有 .zip 资产" >&2; exit 1; }

EXPECTED="VoiceGhostty-${TAG}.zip"
if [[ "$(basename "$ZIP")" != "$EXPECTED" ]]; then
    # cask 的 url 是按这个名字拼的,对不上就装不下来
    echo "✗ 资产名 $(basename "$ZIP") 与 cask 期望的 $EXPECTED 不符" >&2
    echo "  改 release.yml 的 ZIP 变量,或改 cask 里的 url" >&2
    exit 1
fi

SHA=$(shasum -a 256 "$ZIP" | awk '{print $1}')
echo "▶ version $VERSION / sha256 $SHA"

# 只动这两行,其余(caveats/zap/postflight)保持人工维护
/usr/bin/sed -i '' \
    -e "s|^  version \".*\"|  version \"${VERSION}\"|" \
    -e "s|^  sha256 \".*\"|  sha256 \"${SHA}\"|" \
    "$CASK_SRC"
echo "✅ 已更新 $CASK_SRC"

if [[ "$PUSH" != "--push" ]]; then
    echo
    echo "下一步(要推到 tap 仓库就加 --push):"
    echo "  $0 $TAG --push"
    exit 0
fi

echo "▶ 同步到 tap 仓库 $TAP_REPO"
TAP_DIR="$WORK/tap"
gh repo clone "$TAP_REPO" "$TAP_DIR" -- --depth 1 2>/dev/null || {
    echo "✗ clone $TAP_REPO 失败 —— 仓库还没建?" >&2
    echo "  gh repo create $TAP_REPO --public -d 'Homebrew tap for 10XTeams'" >&2
    exit 1
}
mkdir -p "$TAP_DIR/Casks"
cp "$CASK_SRC" "$TAP_DIR/Casks/voiceghostty.rb"
git -C "$TAP_DIR" add Casks/voiceghostty.rb
if git -C "$TAP_DIR" diff --cached --quiet; then
    echo "✓ tap 里已经是这个版本,无需提交"
else
    git -C "$TAP_DIR" commit -m "voiceghostty ${VERSION}"
    git -C "$TAP_DIR" push
    echo "✅ 已推送到 $TAP_REPO"
fi

echo
echo "验证:"
echo "  brew untap 10xteams/tap 2>/dev/null; brew install --cask 10xteams/tap/voiceghostty"
