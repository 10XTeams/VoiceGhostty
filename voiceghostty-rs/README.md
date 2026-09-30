# VoiceGhostty(Rust 版)

跨平台(macOS / Linux / Windows)的语音终端，是 Swift 版的 Rust 重写。按住空格说话，识别结果打到光标处。**只上屏，不会自动回车执行。**

## 构建与运行

```sh
cargo run --release -p vg-app      # 可执行文件名:voiceghostty
cargo test --workspace
```

## 结构

| crate | 内容 |
|---|---|
| `vg-core` | 配置、GUI 设置(`settings.json`)、主题、自然语言→命令(Claude / 本地 Ollama)、zsh 集成。没有 GUI 依赖 |
| `vg-voice` | 麦克风采集(cpal)、16 kHz 重采样、裁掉静音、**可插拔识别引擎** |
| `vg-app` | egui 窗口 + alacritty_terminal 终端模拟 + 按住说话 + 语音流水线 |

## 配置语音引擎

配置文件路径：`~/.config/voiceghostty/config`，Windows 上是 `%APPDATA%\voiceghostty\config`。与 Swift 版共用同一份配置，原有的 key 都兼容。

```ini
# auto | http | command | off
# auto 的选择顺序:先用 voice-command，其次 API Key，最后指向本机的 voice-api-url
voice-engine = auto
voice-language = zh          # zh | en | auto(也接受 zh-CN / en-US 写法)
voice-punctuation = on       # 主要口述 shell 命令时建议 off
voice-hold-ms = 250          # 按住多久算"按住说话"
voice-device =               # 麦克风名,留空用系统默认
```

**方案 A：外部命令（本地离线，推荐）**。例如使用 [whisper.cpp](https://github.com/ggml-org/whisper.cpp)：

```ini
voice-engine = command
voice-command = whisper-cli -m ~/models/ggml-large-v3-turbo.bin -l {lang} -nt -np -f {wav}
```

`{wav}` 会替换成 16 kHz 单声道 WAV 临时文件的路径，`{lang}` 替换成识别语言。命令的 stdout 就是识别结果。任何能从 WAV 文件读出文字的程序都可以接进来。

**方案 B：OpenAI 兼容 HTTP 接口**。OpenAI、Groq 等云服务，以及本机的 faster-whisper-server、speaches、whisper.cpp server 都可以用：

```ini
voice-engine = http
voice-api-url = https://api.openai.com/v1/audio/transcriptions
voice-api-key = sk-...       # 为空时依次读取环境变量 VOICE_API_KEY、OPENAI_API_KEY
voice-api-model = whisper-1
```

地址指向本机（127.0.0.1 或 localhost）时不需要 Key，并且会自动绕过系统代理。

## 自然语言模式

状态栏可以切换"听写"和"自然语言"两种模式（快捷键 ⌘M；Linux/Windows 上是 Ctrl+Shift+M）。

```ini
llm-provider = auto          # auto(有 Key 用 Claude,否则 Ollama)| claude | ollama
llm-api-key = sk-ant-...     # 或环境变量 ANTHROPIC_API_KEY
llm-model = claude-opus-5-5
llm-local-model = qwen3:1.7b # Ollama:纠错,以及没有 Claude Key 时生成命令
correction = on
```

## 终端功能

- **状态灯**：标签页上的圆点表示窗格状态，灰色是空闲，黄色是正在运行，绿色是运行结束、等你处理。变绿时会响一声提示音（合成音，可以在设置里关掉），5 秒内最多响一次。你正在看的窗格保持灰色；窗口切到后台后，它也会照常变色和响。
  - zsh 下根据命令的开始和结束精确判断。shell 集成写在 `~/.config/voiceghostty/shell-rs/`，每个窗格的状态记在 `run/` 目录下的状态文件里。
  - claude、vim 这类全屏程序，以及没有 shell 集成的情况，根据输出和响铃来推断。
- **Claude 上下文占用**：窗格前台在跑 Claude Code 时，状态栏会显示 `claude ▬ 65K / 1M · 7%`，数据读自 `~/.claude/projects/` 下的 transcript，统计到 claude 的上一次请求为止。占用到 70% 变橙色，到 90% 变红色。
- **会话记录**：每个窗格写一份 `~/.config/voiceghostty/history/<目录>-<时间>.log`，记录的是终端最终显示的内容，不是原始字节流。可以在设置里关闭。
- **选中与复制**：拖动选中，双击选中一个词，三击选中一整行。macOS 上用 ⌘C 复制；其他平台用 Ctrl+Shift+C 复制，单独的 Ctrl+C 仍然是中断。

## 🧩 加载器与常用项目

- **🧩 加载器**：点状态栏的 🧩，或按 ⌘L（其他平台 Ctrl+Shift+L）打开。它会扫描设置里添加的文件夹，按技能、Agent、插件三个页签列出，勾选后把它们加载到当前窗格所在的项目里：
  - 技能复制到 `.claude/skills/`，Agent 复制到 `.claude/agents/`。已经存在的文件不会被覆盖。
  - 插件在 `.claude/settings.local.json` 里启用或停用，只改 `enabledPlugins` 和 `extraKnownMarketplaces` 这两个键，文件里的其他设置保持不变。
  - 同一个名字只能由一个文件夹提供，排在前面的文件夹优先（可以在设置里调整顺序），后面的同名项会标成"重名"并禁止勾选。
  - 应用之后，程序会在终端里依次发送 `/reload-skills` 和 `/reload-plugins`，两条之间间隔 2 秒。
- **✨ 常用项目**：保存常用目录，点一下就在当前窗格里执行 `cd <目录> && claude`。

## 分屏、搜索与其他

- **分屏**：沿同一个方向继续分屏时会加一个等宽的兄弟窗格，连按 3 次得到 3 等分；换个方向分屏则可以拼出网格。按方向键切换焦点时依据分屏树判断相邻关系。分屏可以移到其他标签页，也可以移出成新标签页，shell 和滚动历史都会跟着一起走。分屏状态下，当前窗格会描一圈边框，每个窗格有自己的状态灯。
- **搜索**：按字面文本搜索滚动历史；查询词全是小写时忽略大小写。找到的内容会被选中，可以直接复制。
- **标签页**：标签名默认用各分屏所在目录的名字；双击标签可以改名，清空后恢复自动命名；中键点击关闭标签页。
- **退出确认**：关闭窗口前会先确认，因为所有 shell 都会被挂断。

## 快捷键

按 ⌘/（Linux/Windows 上是 Ctrl+Shift+/）可以打开完整的快捷键表。macOS 沿用 Swift 版的快捷键：

| 功能 | macOS | Linux / Windows |
|---|---|---|
| 新建 / 关闭 | ⌘T / ⌘W | Ctrl+Shift+T / W |
| 向右 / 向下分屏 | ⌘D / ⌘⇧D | Ctrl+Shift+D / E |
| 切换分屏焦点 | ⌘⌥ 加方向键 | Ctrl+Alt 加方向键 |
| 分屏移到第 N 个标签页 | ⌘⌃1…8 | Ctrl+Alt+1…8 |
| 分屏移到上一个 / 下一个标签页，或移出成新标签页 | ⌘⌃← / → / T | Ctrl+Alt+Shift+← / → / T |
| 下一个 / 上一个标签页 | ⌘⇧] / ⌘⇧[ | Ctrl+PageDown / PageUp |
| 第 N 个 / 最后一个标签页 | ⌘1…8 / ⌘9 | Alt+1…8 / Alt+9 |
| 搜索 / 清屏 / 全选 | ⌘F / ⌘K / ⌘A | Ctrl+Shift+F / K / A |
| 开始或停止录音 / 切换语音模式 | ⌘⇧M / ⌘⇧N | Ctrl+Shift+M / N |
| 加载器 / 设置 | ⌘L / ⌘, | Ctrl+Shift+L / , |
| 字号 | ⌘+ / ⌘- / ⌘0 | Ctrl+Shift++ / - / 0 |

Linux/Windows 上不用单独的 Ctrl 组合键做快捷键，因为 Ctrl+T、Ctrl+W、Ctrl+D 这些在 shell 里都有用。
