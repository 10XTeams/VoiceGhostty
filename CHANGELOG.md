# 更新日志

格式参照 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/),版本号遵循[语义化版本](https://semver.org/lang/zh-CN/),
本项目对「主 / 次 / 修订」的具体约定见 [docs/VERSIONING.md](docs/VERSIONING.md)。

日常改动先记在「未发布」下;发版时 `scripts/bump-version.sh` 把它改名成新版本号并另起一个空的「未发布」。

## [未发布]

### 修复
- Linux / Windows 无法编译(全局热键的非 macOS 分支推断不出类型)。

## [1.0.0] — 2026-11-13

第一个正式版本。之前的 0.1.0 是开发期的占位版本号,没有单独记录;1.0.0 之前的历史见 git 提交记录。
从这一版起,配置文件、设置文件、快捷键和语音命令都算公开约定,改动它们按 [docs/VERSIONING.md](docs/VERSIONING.md) 升级版本号。

### 语音
- 按住说话(默认空格,可配置)、⌘⇧M 开关录音、macOS 全局热键;本机引擎边说边出实时字幕。
- 可插拔识别引擎:内置 whisper.cpp(离线,Metal 加速)、外部命令、OpenAI 兼容 HTTP 接口。
- 听写 / 自然语言两种模式(Claude API 或本地 Ollama),可选先用小模型纠错。
- 语音命令(清屏、新标签、第 N 个标签、向右分屏、撤销)与撤销上一次语音上屏。
- 焦点在搜索框、改名框、设置输入框时,识别结果落进该输入框。
- 识别结果剥离换行,永远不自动回车。

### 终端
- alacritty_terminal 内核,真彩、宽字符、CJK 基线校正、字号实时缩放。
- 标签页、任意嵌套的分屏;分屏跨标签页移动、与相邻分屏交换位置。
- 拖动分隔线调整分屏大小(双击恢复等分,⌘⌃= 整个标签页等分);⌘⇧↩ 放大当前分屏。
- 四态状态灯(空闲 / 执行中 / 已完成 / 等你输入)、⌘B 窗格列表侧边栏、完成提示音、后台系统通知。
- zsh shell 集成(命令边界、输入行着色)、回滚区搜索、会话历史落盘、点击移动光标。

### Claude Code
- 状态栏显示 Claude 上下文占用。
- 🧩 加载器:技能 / Agent / 插件三个页签,一次应用到当前项目。
- ✨ 常用项目一键启动 claude,「新建项目」。

### 其他
- 设置面板、四套主题、配置文件 `~/.config/voiceghostty/config`。
- macOS 预编译包(ad-hoc 签名)与 Homebrew cask;Linux / Windows 可从源码构建。

[未发布]: https://github.com/10XTeams/VoiceGhostty/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/10XTeams/VoiceGhostty/releases/tag/v1.0.0
