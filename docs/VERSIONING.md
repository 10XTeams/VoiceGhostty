# 版本号规则

VoiceGhostty 用[语义化版本](https://semver.org/lang/zh-CN/) `主.次.修订`(`MAJOR.MINOR.PATCH`),从 **1.0.0** 起算。

## 什么算「公开约定」

语义化版本靠「是否破坏兼容」来决定升哪一位,所以先说清楚哪些东西用户会依赖。VoiceGhostty 是桌面应用、不是库,
它的公开约定是**用户手上会留下来、或者养成了习惯的东西**:

| 约定 | 位置 |
|---|---|
| 配置文件的键名与取值 | `~/.config/voiceghostty/config` |
| 设置文件的格式 | `~/.config/voiceghostty/settings.json` |
| 快捷键 | `crates/vg-app/src/shortcuts.rs` |
| 语音命令的说法 | `crates/vg-core/src/voice_commands.rs` |
| 写到用户磁盘上的文件位置与格式 | 会话历史、zsh 集成目录、加载器写进项目的 `.claude/` |
| 安全承诺 | 识别结果永不自动回车;内置 whisper 时音频不出本机 |
| 支持的平台 | macOS 最低版本、Linux / Windows 的构建方式 |

界面布局、颜色、提示文案、内部代码结构**不算**约定,随时可改,不影响版本号的哪一位。

## 升哪一位

**主版本(X.0.0)** —— 用户升级后,原来能用的东西不能用了、或者行为变了却没有提示:
- 删除或改名配置键、设置项,且不自动迁移;改变已有配置值的含义
- 改掉已有的快捷键或语音命令(只新增备选不算)
- 改变写到磁盘上的文件位置或格式,导致旧文件读不了、或别的工具读不了
- 放宽安全承诺
- 提高最低系统版本、放弃一个平台

**次版本(1.X.0)** —— 新增功能,老用户什么都不用改:
- 新功能、新快捷键、新语音命令、新配置项(带默认值)
- 新增识别引擎、主题、语言
- 自动迁移的格式升级(旧文件能读、读完写成新格式)
- 明显的行为改进(比如状态灯判断更准),但不改变已有约定

**修订版(1.0.X)** —— 只修问题:
- 修 bug、修崩溃、修性能
- 文案、翻译、文档
- 不改变任何约定的内部重构、依赖升级

拿不准时选更高的一位:宁可让用户多看一眼更新日志,也不要让人以为「只是修 bug」结果配置失效了。

## 预发布

需要先给少数人试用时,用预发布后缀:`1.2.0-beta.1`、`1.2.0-beta.2`、`1.2.0-rc.1`,最后发 `1.2.0`。
预发布版本不进 Homebrew cask。

## 发版流程

1. 平时每个面向用户的改动都在 [CHANGELOG.md](../CHANGELOG.md) 的「未发布」下记一条(新增 / 变更 / 修复 / 移除)。
2. 根据「未发布」里最重的一条决定升哪一位,运行:
   ```sh
   scripts/bump-version.sh minor        # 或 major / patch / 1.2.0-beta.1
   ```
   它会改 `Cargo.toml` 的版本、刷新 `Cargo.lock`、把 CHANGELOG 的「未发布」改成新版本号和日期。
   「未发布」是空的会拒绝执行 —— 没有改动就不该发版。
3. 检查改动、提交,打 tag 并推送(脚本会打印出这几条命令):
   ```sh
   git commit -am "chore: 发布 v1.2.0"
   git tag v1.2.0
   git push && git push origin v1.2.0
   ```
4. CI(`.github/workflows/release.yml`)校验 tag 与 `Cargo.toml` 一致、CHANGELOG 里有这一版,然后构建 .app、
   建 GitHub Release(说明取自 CHANGELOG 对应那一节)。
5. 正式版再运行 `scripts/release-cask.sh v1.2.0 --push` 同步 Homebrew cask。

## 版本号出现在哪

唯一来源是 `Cargo.toml` 的 `[workspace.package] version`,其余都由它派生:

- 二进制里的 `CARGO_PKG_VERSION`,设置窗口底部显示 `v1.0.0 (abc1234+)`(`+` 表示带未提交改动)
- 打包时写入 `Info.plist` 的 `CFBundleShortVersionString`;`CFBundleVersion` 是构建时间戳
- git tag `v1.0.0`、Release 资产 `VoiceGhostty-v1.0.0.zip`、Homebrew cask 的 `version`

不要在别处手写版本号。
