<div align="center">

<img src="Resources/AppIcon.png" width="120" alt="VoiceGhostty icon" />

# VoiceGhostty

**为 Claude Code 打造的语音优先终端 —— 按住空格说话,话就落在命令行上。**

macOS 原生。按住说话、自然语言转命令、标签与分屏、一键加载 Claude Code *技能*。

[![Build & Release](https://github.com/10XTeams/VoiceGhostty/actions/workflows/release.yml/badge.svg)](https://github.com/10XTeams/VoiceGhostty/actions/workflows/release.yml)
[![Latest release](https://img.shields.io/github/v/release/10XTeams/VoiceGhostty?sort=semver)](https://github.com/10XTeams/VoiceGhostty/releases)
[![Stars](https://img.shields.io/github/stars/10XTeams/VoiceGhostty?style=social)](https://github.com/10XTeams/VoiceGhostty/stargazers)
![Platform](https://img.shields.io/badge/macOS-13%2B-black?logo=apple)
![Swift](https://img.shields.io/badge/Swift-5%2F6.1-orange?logo=swift)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow)](LICENSE)

[**English →**](README.md)

</div>

---

<div align="center">

![VoiceGhostty 演示 —— 一键启动 Claude、动态加载技能、内置设置面板、标签与分屏](docs/demo.gif)

</div>

## 为什么

用 [Claude Code](https://claude.com/claude-code) 干活,大量时间花在「敲命令、执行、看输出、再敲」上。VoiceGhostty 让「说」这部分不用碰键盘:

- **说,而不是敲。** 按住 <kbd>空格</kbd>,直接把话听写到 shell 提示符上。
- **说想干什么,拿到命令。** 自然语言模式把「撤销上一次提交但保留改动」变成 `git reset --soft HEAD~1`,并在执行前给你一句解释。
- **绝不会突然自己执行。** 识别结果剥离换行后落在光标处,回车永远由**你**来按。
- **随手加载 Claude Code 技能。** 🧩 面板把技能库里的技能同步进当前项目的 `.claude/skills/`,然后自动发 `/reload-skills`。

## 功能

| | |
|---|---|
| 🎙 **按住说话** | 按住 <kbd>空格</kbd>(≥0.25 秒)录音,松开即停;快速点按仍是正常输入空格。也可用 <kbd>⌘⇧M</kbd> 开关。 |
| 🧠 **两种语音模式** | *听写*(本地 Ollama 小模型去语气词、纠错别字,离线可用)与*自然语言*(Claude API 或 Apple 端侧模型 → 命令 + 解释)。 |
| 🌏 **中英双语识别** | 主语言实时识别;结果不可信时,再用另一种语言对完整录音重跑一次 —— 中英混说(如「帮我 ls 一下这个目录」)也能落地。默认自动加标点,可关。 |
| 🚦 **状态灯** | 每个标签页和分屏都有灰 / 黄 / 绿三态点:空闲、执行中、已完成等你处理。由 OSC 133 shell 集成驱动,无集成时用输出静默启发式兜底。在 `claude` 这类 TUI 里(对 shell 而言是一条跑很久的命令),灯改看它的输出:在刷屏 = 在干活,静下来 = 该你了。 |
| 🗂 **标签与嵌套分屏** | <kbd>⌘T</kbd> / <kbd>⌘W</kbd>、<kbd>⌘1…⌘9</kbd>。<kbd>⌘D</kbd> / <kbd>⌘⇧D</kbd> 拆的是**当前分屏**且可反复按 —— 一个标签页能放任意多格、嵌套任意深:连按三次 <kbd>⌘D</kbd> 得到三等分的列,再往另一个方向拆就形成网格。<kbd>⌘⌥</kbd>+方向键在整棵树上切焦点,<kbd>⌘W</kbd> 先关分屏再关标签页,点哪格语音就走哪格。标签名自动跟随工作目录并显示分屏格数,双击可改名。 |
| 🧩 **动态加载技能** | 勾选式面板:从技能库挑技能,应用后复制进 `cwd/.claude/skills/`,取消勾选即移除,并自动发送 `/reload-skills`。 |
| 🎨 **主题与搜索** | 暗色 / 亮色 / Solarized Dark / Dracula,<kbd>⌘F</kbd> 回滚区搜索(带匹配计数),字号实时缩放,<kbd>⌘K</kbd> 清屏。回滚区默认每个分屏 10000 行,可调且对已打开的分屏立即生效。 |
| 📜 **会话历史** | 每个分屏的完整记录追加到 `~/.config/voiceghostty/history/<文件夹名>-<开始时间>.log`。内容取自终端缓冲区,所以重绘型 TUI 落盘的是你真正看到的那些行 —— 且文件保存整个会话,不受回滚行数上限截断。 |
| ✨ **Claude 快捷启动** | 保存常用项目目录,点一下就在当前分屏执行 `cd <目录> && claude`。 |
| ⚙️ **设置面板** | 一个齿轮搞定:切换**默认语言**(语音识别与整个界面一起切,English ⇆ 中文)、开关自动标点、设定回滚行数、开关会话历史、设定默认技能库文件夹、选择**命令输入颜色**,同一面板还有完整快捷键清单。 |
| 🎨 **输入行着色** | 你在 zsh 提示符下敲的整行命令按设定颜色高亮;改色后所有已打开标签页即刻生效。 |
| 🔒 **安全优先** | 语音绝不自动回车,插入前剥离换行;语音识别在端侧完成;API Key 只存在本地配置文件里。 |

完整功能清单(含实现说明):[**docs/FEATURES.zh-CN.md**](docs/FEATURES.zh-CN.md)。

## 快捷键

| 按键 | 动作 | | 按键 | 动作 |
|---|---|---|---|---|
| 按住 <kbd>空格</kbd> | 说话(点按 = 空格) | | <kbd>⌘D</kbd> / <kbd>⌘⇧D</kbd> | 向右 / 向下分屏(可反复) |
| <kbd>⌘⇧M</kbd> | 开始 / 停止录音 | | <kbd>⌘⌥</kbd>←↑↓→ | 切换分屏焦点 |
| | | | <kbd>⌘⌃1…8</kbd> / <kbd>⌘⌃T</kbd> | 把分屏送到第 N 个 / 新标签页 |
| <kbd>⌘T</kbd> | 新建标签页 | | <kbd>⌘+</kbd> <kbd>⌘-</kbd> <kbd>⌘0</kbd> | 字号缩放 / 重置 |
| <kbd>⌘W</kbd> | 关分屏,无分屏则关标签页 | | <kbd>⌘K</kbd> | 清屏 |
| <kbd>⌘⇧]</kbd> / <kbd>⌘⇧[</kbd> | 下一个 / 上一个标签页 | | <kbd>⌘F</kbd> | 搜索回滚内容 |
| <kbd>⌘1</kbd>…<kbd>⌘8</kbd> / <kbd>⌘9</kbd> | 第 N 个 / 最后一个标签页 | | <kbd>⌘C</kbd> <kbd>⌘V</kbd> <kbd>⌘A</kbd> | 复制 / 粘贴 / 全选 |

同样的清单在 app 内 ⚙️ → 快捷键 里也有。

## 安装

> **关于签名。** 构建产物是 ad-hoc 签名、**未经 Apple 公证** —— 公证需要付费的 Apple Developer Program 会员。
> 因此下载下来的构建在清除隔离属性前,macOS 会拒绝打开。下面三种方式都各自处理了这一点,按你的接受程度选。

### 方式 A —— Homebrew(最快)

```bash
brew install --cask 10xteams/tap/voiceghostty
```

cask 会在安装后替你清除隔离属性,装完直接能开。(Homebrew 默认是**添加**隔离属性而不是移除,所以必须由 cask 来做这件事。)

升级 `brew upgrade --cask voiceghostty`,卸载 `brew uninstall --cask voiceghostty`(加 `--zap` 会一并删除 `~/.config/voiceghostty`)。

### 方式 B —— 直接下载

从 [**Releases**](https://github.com/10XTeams/VoiceGhostty/releases) 下载最新 `.zip` 解压,移到 `/Applications`,然后清一次隔离属性:

```bash
xattr -cr /Applications/VoiceGhostty.app
open /Applications/VoiceGhostty.app
```

不想跑 `xattr` 的话:双击打开、让它被拦下,然后到**系统设置 → 隐私与安全性**,拉到底点**仍要打开**。注意在当前版本的 macOS 上,右键→打开已经**不能**再绕过 Gatekeeper 了,网上不少旧教程还在这么写。

macOS 弹窗时授予**麦克风**与**语音识别**权限。

### 方式 C —— 从源码构建

本地构建的 app 不会被打上隔离属性,没有需要绕过的东西。

```bash
git clone https://github.com/10XTeams/VoiceGhostty.git
cd VoiceGhostty
./make-app.sh          # release 构建、打包 .app、启动
```

> 权限弹窗依赖 app bundle 里的 `Info.plist`,所以用 `make-app.sh`,不要直接 `swift run`。

需要 **macOS 13+**。自然语言模式的 *Apple 端侧*后端需要 macOS 26 + Apple Intelligence;没有的话,配置 Anthropic API Key 走 Claude 即可。

## 配置

`~/.config/voiceghostty/config`,极简 `key = value`,`#` 注释:

```ini
font-family = Menlo
font-size   = 14
theme       = dracula          # dark | light | solarized-dark | dracula
foreground  = #f8f8f2          # 可选,覆盖主题前景色
background  = #282a36          # 可选,覆盖主题背景色
scrollback  = 10000            # 每个分屏保留的回滚行数(默认 10000;设置面板优先级更高)

# 自然语言模式
llm-provider = auto            # auto | claude | apple(auto:有 Key 走 Claude,否则端侧)
llm-model    = claude-opus-4-8
llm-api-key  = sk-ant-...      # 留空则读 $ANTHROPIC_API_KEY

# 自然语言模式的文本矫正(本地离线)
correction      = on           # on | off
llm-local-model = qwen3:1.7b
llm-local-url   = http://127.0.0.1:11434

# 🧩 面板列出技能的来源目录
skill-library-dir = ~/.claude/skills
```

未知键和坏行直接忽略,不会报错。全部 13 个键见 [docs/FEATURES.zh-CN.md](docs/FEATURES.zh-CN.md#16-配置文件)。

> 从 Finder 启动的 app 拿不到 shell 环境变量,所以推荐把 Key 写进配置文件并 `chmod 600`。

**app 内设置(⚙️)。** 默认语言、自动标点、回滚行数、会话历史、默认技能库文件夹、命令输入颜色都能在齿轮面板里直接改,无需编辑文件,且优先级高于配置文件。

### 可选:自然语言模式的矫正模型

```bash
brew install ollama
brew services start ollama    # 后台服务,开机自启
ollama pull qwen3:1.7b        # ~1.4GB,矫正任务足够
```

装好后无需任何配置,自然语言模式会先把识别文本过一遍矫正再送给 LLM(app 启动时自动预热模型)。Ollama 没在跑或矫正超过 15 秒,会静默使用原文,绝不阻塞。听写模式不走这条链路,说什么就逐字敲什么。

### Shell 集成

VoiceGhostty 会在 `~/.config/voiceghostty/shell/` 生成一层 ZDOTDIR 包装:先 source 你真正的 `~/.z*` 文件,再追加两样东西 —— 一个 `line-pre-redraw` 钩子给提示符输入行上色(每次重绘都重读 `~/.config/voiceghostty/input-color`,所以改色在所有已打开标签页即刻生效),以及驱动状态灯的 OSC 133 标记。你自己的提示符、历史输出、`claude` 这类全屏 TUI 都不受影响,并且只对 zsh 做注入。

## 工作原理

```
麦克风(AVAudioEngine,RMS 门限 + 预滚缓冲)
  → 端侧识别(SFSpeechRecognizer,主语言实时)
  → 仲裁 → 必要时用备用语言对完整录音重跑一次
  → 文本
     ├─ [听写模式]     本地 Ollama 小模型清理(离线,失败静默回退原文)
     └─ [自然语言模式] LLM → 命令 + 解释
                          ├─ Claude API(structured outputs)
                          └─ Apple 端侧模型(FoundationModels)
  → 剥离换行 → 落到终端光标处  ★ 绝不自动回车 ★
  → 你按回车 → SwiftTerm → PTY → zsh
```

基于 [SwiftTerm](https://github.com/migueldeicaza/SwiftTerm) 与 Apple Speech 框架构建。

## 路线图

- [x] SwiftTerm 起 zsh,语音听写,填到光标处(绝不自动回车)
- [x] 标签 / 分屏 / 主题 / 搜索 / 配置文件
- [x] 自然语言 → 命令(Apple 端侧模型**与** Claude API)
- [x] 本地小模型矫正(服务于自然语言模式)
- [x] Claude Code 技能动态加载
- [x] Shell 集成:OSC 133 状态灯、输入行着色、标签名跟随工作目录
- [x] app 内设置面板 + 双语界面(English ⇆ 中文)
- [x] 会话记录落盘、回滚行数可配置
- [x] 嵌套分屏树:每个标签页任意格数,支持网格布局
- [ ] 命令上下文感知、危险命令分级、语音修正、TTS 朗读
- [ ] 远期:终端内核迁移到 libghostty

## 参与贡献

欢迎 Issue 与 PR。如果 VoiceGhostty 对你有用,点个 ⭐ 能实实在在帮到更多人。

## 许可

[MIT](LICENSE) © 10XTeams
