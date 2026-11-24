# VoiceGhostty

## 版本规则(必须遵守)

版本号是语义化版本 `主.次.修订`(x.y.z),唯一来源是 `Cargo.toml` 的 `[workspace.package] version`,不要在别处手写。
完整规则见 [docs/VERSIONING.md](docs/VERSIONING.md)。

### 每次改动完成后都要升版本

每做完一个面向用户的改动(功能、行为变化、bug 修复),提交时一并:

1. 在 `CHANGELOG.md` 的「未发布」下记一条(新增 / 变更 / 修复 / 移除)。
2. 按这次改动里最重的一条决定升哪一位,运行 `scripts/bump-version.sh <级别>`
   (它改 `Cargo.toml`、刷新 `Cargo.lock`、把「未发布」改名成新版本号和日期):
   - `major`(x.0.0):破坏已有约定 —— 删改配置键 / 设置项且不迁移、改掉已有快捷键或语音命令、
     改变写到磁盘的文件位置或格式、放宽安全承诺、提高最低系统版本或放弃平台。
   - `minor`(1.x.0):新增功能 —— 新功能、新快捷键、新语音命令、新配置项(带默认值)、自动迁移的格式升级、明显的行为改进。
   - `patch`(1.0.x):只修问题 —— bug、崩溃、性能、文案、文档、不改约定的重构与依赖升级。
   - 拿不准时选更高的一位。
3. 版本号改动和代码改动放在同一个提交里。

纯内部改动(测试、CI、注释、backlog)不升版本、不记 CHANGELOG。
打 tag、推 tag、发 Release、同步 Homebrew cask 只在用户要求发版时做(流程见 docs/VERSIONING.md)。

### 主 / 次版本(x.y)变化后要更新 README

升了 `major` 或 `minor` 之后,同步改 `README.md` 和 `README.zh-CN.md`(两份内容保持一致):

- 「亮点」里加上新功能(保持简洁,每条一句话,总数控制在 6–8 条,必要时合并或替换旧条目)。
- 新功能有常用快捷键的,加进「常用快捷键」表。
- 被移除或改名的功能、快捷键从 README 删掉或改正。

`patch` 升级不改 README。

## 构建

- cargo 命令前先 `source ~/.cargo/env`。
- `./make-app.sh` 打包到 `build/VoiceGhostty.app`;日常使用的是 `/Applications/VoiceGhostty.app`,验证改动前要把新包复制过去。
