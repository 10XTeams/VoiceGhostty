#!/bin/bash
# 升版本号:改 Cargo.toml、刷新 Cargo.lock、把 CHANGELOG 的「未发布」改成新版本。规则见 docs/VERSIONING.md。
#
#   scripts/bump-version.sh patch            # 1.0.0 -> 1.0.1
#   scripts/bump-version.sh minor            # 1.0.1 -> 1.1.0
#   scripts/bump-version.sh major            # 1.1.0 -> 2.0.0
#   scripts/bump-version.sh 1.2.0-beta.1     # 指定版本(预发布也行)
#
# 只改本地文件,不提交、不打 tag、不推送;结束时打印接下来要敲的命令。
set -euo pipefail
cd "$(dirname "$0")/.."

ARG="${1:-}"
if [[ -z "$ARG" ]]; then
    sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//' >&2
    exit 1
fi

SEMVER='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?$'

# [workspace.package] 里的 version,也就是 Cargo.toml 里第一条顶格的 version =
CURRENT=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
[[ "$CURRENT" =~ $SEMVER ]] || { echo "✗ Cargo.toml 当前版本 '$CURRENT' 不是合法的语义化版本" >&2; exit 1; }
MAJOR=${BASH_REMATCH[1]} MINOR=${BASH_REMATCH[2]} PATCH=${BASH_REMATCH[3]} PRE=${BASH_REMATCH[4]}

case "$ARG" in
    major) NEW="$((MAJOR + 1)).0.0" ;;
    minor) NEW="$MAJOR.$((MINOR + 1)).0" ;;
    # 预发布 1.1.0-beta.2 之后的 patch 就是正式的 1.1.0
    patch) if [[ -n "$PRE" ]]; then NEW="$MAJOR.$MINOR.$PATCH"; else NEW="$MAJOR.$MINOR.$((PATCH + 1))"; fi ;;
    *) NEW="$ARG" ;;
esac
[[ "$NEW" =~ $SEMVER ]] || { echo "✗ '$NEW' 不是合法的语义化版本(例:1.2.0、1.2.0-beta.1)" >&2; exit 1; }

# 新版本必须比当前高。语义化版本里 1.2.0-beta.1 < 1.2.0,而 sort -V 认为相反,所以分开比:
# 先比 主.次.修订,相同时只允许 预发布 -> 预发布(更大) 或 预发布 -> 正式版
core() { echo "${1%%-*}"; }
# a 严格大于 b(按 sort -V)
gt() { [[ "$1" != "$2" && "$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -1)" == "$1" ]]; }
NEW_PRE="${NEW#"$(core "$NEW")"}"
if [[ "$(core "$NEW")" == "$(core "$CURRENT")" ]]; then
    # 主.次.修订相同:当前得是预发布,新版本是正式版或更大的预发布
    ok=false
    if [[ -n "$PRE" ]] && { [[ -z "$NEW_PRE" ]] || gt "$NEW_PRE" "$PRE"; }; then ok=true; fi
else
    ok=false
    gt "$(core "$NEW")" "$(core "$CURRENT")" && ok=true
fi
$ok || { echo "✗ 新版本 $NEW 不比当前的 $CURRENT 高" >&2; exit 1; }

# 「未发布」下面得有内容:没有改动就不该发版
UNRELEASED=$(awk '/^## \[未发布\]/{on=1; next} /^## \[/{on=0} on && NF' CHANGELOG.md)
if [[ -z "$UNRELEASED" ]]; then
    echo "✗ CHANGELOG.md 的「## [未发布]」下面是空的,先把这一版的改动记上" >&2
    exit 1
fi

if git rev-parse -q --verify "refs/tags/v$NEW" >/dev/null; then
    echo "✗ tag v$NEW 已经存在" >&2
    exit 1
fi

TODAY=$(date +%Y-%m-%d)
echo "▶ $CURRENT -> $NEW"

# Cargo.toml:只改第一条顶格 version(workspace 版本),依赖里的 version 都不是顶格的
awk -v new="$NEW" '!done && /^version = "/ { print "version = \"" new "\""; done=1; next } { print }' Cargo.toml > Cargo.toml.tmp
mv Cargo.toml.tmp Cargo.toml

# Cargo.lock:三个 crate 的版本跟着变。--offline 免得网络不稳时卡住;这一步不需要下载任何东西
if command -v cargo >/dev/null || { [[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env"; }; then
    cargo update --workspace --offline --quiet
else
    echo "⚠ 找不到 cargo,Cargo.lock 没刷新;下次构建时会自动更新" >&2
fi

# CHANGELOG:「未发布」改成新版本,上面另起一个空的「未发布」;底部链接同步
awk -v new="$NEW" -v today="$TODAY" -v cur="$CURRENT" '
    /^## \[未发布\]/ { print; print ""; print "## [" new "] — " today; next }
    /^\[未发布\]: / {
        sub(/compare\/v.*\.\.\.HEAD/, "compare/v" new "...HEAD")
        print
        print "[" new "]: https://github.com/10XTeams/VoiceGhostty/releases/tag/v" new
        next
    }
    { print }
' CHANGELOG.md > CHANGELOG.md.tmp
mv CHANGELOG.md.tmp CHANGELOG.md

echo "✓ 已更新 Cargo.toml、Cargo.lock、CHANGELOG.md。检查无误后:"
echo
echo "    git commit -am \"chore: 发布 v$NEW\""
echo "    git tag v$NEW"
echo "    git push && git push origin v$NEW"
