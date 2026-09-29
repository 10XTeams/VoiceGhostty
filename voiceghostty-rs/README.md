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

## 快捷键

macOS 上用 ⌘，其他平台用 Ctrl+Shift：T 新建标签页，W 关闭标签页，1–9 切换标签页，=/-/0 调整字号，M 切换语音模式，逗号打开设置。
