# VoiceGhostty — 功能清单

按源码扫描整理的完整功能清单。每一项都标出了实现文件,所以这份文档同时也是代码地图。

[English →](FEATURES.md)

**技术栈:** Rust。界面 [egui](https://github.com/emilk/egui) / eframe,终端模拟
[alacritty_terminal](https://github.com/alacritty/alacritty),录音与提示音 [cpal](https://github.com/RustAudio/cpal),
可选的进程内 [whisper-rs](https://github.com/tazz4843/whisper-rs)(whisper.cpp),HTTP 走 reqwest(rustls,无系统 OpenSSL 依赖)。
三个 crate:`vg-core`(配置、主题、LLM、shell 集成、加载器 —— 不依赖 GUI)、`vg-voice`(录音、预处理、识别引擎)、
`vg-app`(窗口、终端控件、流水线)。约 9.9k 行 Rust、46 个文件(去掉单元测试约 8.4k 行)。
跨平台(macOS / Linux / Windows);开箱不依赖任何外部服务 —— 编进内置 whisper 时连语音识别也完全离线。

---

## 1. 语音采集

| 功能 | 说明 | 源文件 |
|---|---|---|
| 按住说话 | 按住 <kbd>空格</kbd> ≥ `voice-hold-ms`(默认 250 ms)开始录音,松开即停。低于阈值的快速点按会原样补发为普通空格,不影响正常输入;按住期间吞掉系统按键重复。只在终端窗格有键盘焦点时生效;带 Ctrl / Alt / ⌘ 的空格不拦;输入法正在组字(拼音还没上屏)时空格归输入法。窗格失焦时放弃当前按键,正在录音则停止。 | `crates/vg-app/src/ptt.rs`、`crates/vg-app/src/terminal/view.rs` |
| 开关录音 | <kbd>⌘⇧M</kbd>,或点状态栏的语音胶囊。录音中胶囊变红,显示「正在听」、28 根柱子的电平波形(约每 40 ms 采样一次)和计时;再点一下停止。 | `crates/vg-app/src/app.rs`、`crates/vg-app/src/chrome.rs` |
| 录音 | cpal 打开系统默认输入设备(或 `voice-device` 指定的那个);f32 / i16 / u16 三种采样格式,多声道取平均下混成单声道。cpal 的 `Stream` 在 macOS 上不是 `Send`,所以流在专用线程里创建和持有,主线程只通过通道发「停止」并取回录音。录音期间持续更新 RMS 电平供界面画波形。 | `crates/vg-voice/src/recorder.rs` |
| 预处理 | 线性插值重采样到 16 kHz(Whisper 系的原生采样率);以 20 ms 为窗、RMS 阈值 0.01 裁掉首尾静音,两端各留 200 ms 余量避免吞掉起音。全程静音、或有声段短于 0.3 s → 不跑识别,状态栏提示「没听到声音」。 | `crates/vg-voice/src/clip.rs`、`crates/vg-voice/src/lib.rs` |
| 整段识别 | 不是流式:松开后把整段录音交给引擎,所以**没有实时字幕**。识别、纠错、生成命令都在后台线程,结果经通道回到 UI 线程;状态栏胶囊依次显示「识别中…」「整理语句…」「生成命令…」。 | `crates/vg-app/src/pipeline.rs` |
| 结果清洗 | 所有引擎的输出统一清洗:多行拼成一行(中文与中文之间不加空格),去掉 Whisper 的 `[BLANK_AUDIO]` / `[Music]` 之类方括号标注。 | `crates/vg-voice/src/engine.rs`(`clean_transcript`) |
| 标点 | `voice-punctuation` / 设置里的「自动标点」,默认开。通过提示词引导 Whisper 系模型:中文时要求简体(否则常出繁体),关标点时给一句无标点的示例。主要口述 shell 命令时建议关掉 —— 句号是噪音。 | `crates/vg-voice/src/engine.rs`(`TranscribeOptions::prompt`) |
| 权限 | 麦克风权限由系统弹窗处理,弹窗依赖 `.app` bundle 里的 `NSMicrophoneUsageDescription`(所以要用 `make-app.sh` 打包,裸二进制拿不到权限)。找不到设备或流启动失败时报一句提示检查麦克风权限的错误。 | `crates/vg-voice/src/recorder.rs`、`packaging/macos/Info.plist` |

## 2. 识别引擎

识别引擎**可插拔**,由 `voice-engine` 选择。所有引擎都是同一个接口(`SpeechEngine`):吃 16 kHz 单声道、已裁静音的整段录音,
吐一段文本。每次开始录音前先构造一次引擎,没配好当场报错,不会录完才发现;状态栏胶囊里显示当前引擎名
(如 `whisper · large-v3-turbo`)。

| 引擎 | 说明 | 源文件 |
|---|---|---|
| `whisper`(内置) | whisper.cpp 进程内推理,编译时开启 feature `whisper`(CPU)或 `whisper-metal`(macOS GPU),需要 cmake 与 C++ 编译器。模型是 ggml 格式:`voice-model` 指定路径,否则取 `<配置目录>/models/` 里按名排序的第一个 `ggml-*.bin`。**全进程只载一次**,按模型路径缓存,换了路径才重载;推理用的 `WhisperState`(约 460 MB 的 KV cache 与 Metal 计算缓冲)同样只建一次反复复用,锁在整次识别期间持有。**按录音时长缩小 `audio_ctx`**:Whisper 不管说了多长都把音频补到 30 s 再算一遍编码器,延迟几乎全在这里;实际帧数(每秒 50 帧)+ 约 5 s 余量,向上取到 64 的倍数,夹在 [512, 1500] —— M3 上 4 s 语音约快 3 倍,下限 512 是因为更小的上下文会让解码器幻听、重复。开 flash attention;贪心解码、单段、不带跨段上下文;线程数 = min(核数, 8)。App 启动时后台**预载**模型,第一次说话不用等;退出前**显式释放**,否则 ggml 的 Metal 设备在 exit 时先析构会直接 abort。 | `crates/vg-voice/src/whisper.rs`、`crates/vg-voice/src/lib.rs` |
| `command`(外部命令) | `voice-command` 模板,`{wav}` 替换成 16 kHz 单声道 WAV 临时文件路径、`{lang}` 替换成识别语言(都按平台 shell 规则加引号),经 `sh -c`(Windows 用 `cmd /C`)执行,stdout 就是结果。例:`whisper-cli -m ~/models/ggml-base.bin -l {lang} -nt -f {wav}`。非零退出码报错并附 stderr 前 300 字。任何能从 WAV 读出文字的程序都能接。 | `crates/vg-voice/src/engine.rs` |
| `http`(OpenAI 兼容) | `POST voice-api-url`(默认 `https://api.openai.com/v1/audio/transcriptions`)multipart:`model`、`response_format=json`、`file`、非 `auto` 时的 `language`、以及上面的提示词 `prompt`。Key 来自 `voice-api-key`,为空依次读 `VOICE_API_KEY`、`OPENAI_API_KEY`。地址指向本机(`127.0.0.1` / `localhost` / `[::1]` / `0.0.0.0`)时不需要 Key,且**绕过系统代理**直连。60 s 超时;错误信息从响应体的 `error.message` / `error` / `detail` 里取。OpenAI、Groq 等云服务和本机的 faster-whisper-server、speaches、whisper.cpp server 都走这个协议。 | `crates/vg-voice/src/engine.rs` |
| `off` | 关闭语音,空格就是空格。 | — |
| `auto`(默认) | 按可用性挑第一个:内置 whisper(编进来了且找到模型)→ 外部命令(`voice-command` 非空)→ HTTP(有 Key,或地址指向本机)。都没有则报错,状态栏胶囊显示「还没有可用的语音引擎 · 去设置」,点一下打开设置。 | `crates/vg-voice/src/lib.rs`(`build_engine`) |

识别语言 `voice-language` / 设置里的「识别语言」:`zh` / `en` / `auto`(也接受 `zh-CN`、`en-US` 写法,取横线前的部分)。
**没有**旧版那套中英双语仲裁重跑:一次录音只跑一次识别,语言是什么就按什么识别。

## 3. 两种语音模式

由状态栏左侧的分段控件切换,<kbd>⌘⇧N</kbd> 也能切(`crates/vg-app/src/pipeline.rs`,`VoiceMode`)。

### 听写

识别结果**原样**落到光标处,不经过任何改写。听写要敲的多半是命令片段、路径、文件名,小模型在这类文本上改坏的概率高于改对,所以这条链路刻意保持逐字。唯一的加工是插入前把换行替换成空格(见第 4 节安全模型)。状态栏显示「已上屏:…」。

### 自然语言

说意图,得到一条 shell 命令 + 一句解释(状态栏显示「『你说的话』→ 解释」)。

识别文本先过一遍**本地矫正**,再送给 LLM —— 口水词和同音错字先清掉,LLM 拿到的是干净意图。

| 功能 | 说明 | 源文件 |
|---|---|---|
| 本地小模型 | Ollama `/api/chat`,默认 `http://127.0.0.1:11434`、`qwen3:1.7b`,`temperature 0`。去语气词、去结巴重复,尽力纠正同音错字。用 few-shot 示例约束 —— 只给规则的话,1.7B 级模型会随机删实词。 | `crates/vg-core/src/ollama.rs` |
| 绝不阻塞 | 服务没起、超时(15 秒)、HTTP 失败、解析失败 → 静默使用原文,直接送 LLM。矫正只能是锦上添花,绝不能吞掉用户说的话。`correction = off` 可整体关掉。 | `crates/vg-core/src/ollama.rs`、`crates/vg-app/src/pipeline.rs` |
| 安全阀 | 先去掉模型回显的 `/no_think`、`/think`;矫正后文本相对原文缩到 50% 以下(原文超过 6 个字时才查,「嗯 ls」这种本来就该大幅压缩)或膨胀超过 150%,判定模型跑飞,回退原文。 | `crates/vg-core/src/ollama.rs`(`sanitize`) |
| 预热 | 开着矫正时 App 启动后台发一次 `/api/generate` 把模型载入内存(`keep_alive: 30m`,每次请求也都带),把十几秒的冷加载挪到你开口之前;失败静默忽略。 | `crates/vg-app/src/app.rs`、`crates/vg-core/src/ollama.rs` |
| 绕过系统代理 | reqwest 客户端 `.no_proxy()`,防止系统代理(Clash/Surge 等)劫持本机回环请求 —— 实测经代理后模型输出异常冗长直到超时。 | `crates/vg-core/src/ollama.rs` |
| 结构化输出 | Ollama `format` JSON Schema 语法约束,回复只含一个 `text` 字段 —— 这也是真正压住 qwen3 思考输出的手段(chatml 模板下的 think 开关不可靠)。 | `crates/vg-core/src/ollama.rs` |

| 后端 | 说明 | 源文件 |
|---|---|---|
| Claude API | 直接 `POST /v1/messages`(无官方 Rust SDK),默认模型 `claude-opus-5-5`,`max_tokens 1024`,30 s 超时。用 **structured outputs**(`output_config.format: json_schema`)由 API 层保证 `{command, explanation}` 结构;`effort: low` 压低语音场景的延迟;带 `fallbacks: "default"` 与对应的 beta 头,安全分类器拒绝时服务端在同一次调用里换模型重跑,所以解析时取**最后一个** text 块。HTTP 401 / 403 / 429 / 5xx 与 `stop_reason: refusal` 都翻译成一句人话。 | `crates/vg-core/src/claude.rs` |
| 本地 Ollama | 没有 Claude Key 时的跨平台离线兜底(取代旧版的 Apple 端侧模型):同一个 `/api/chat` + JSON Schema 约束,60 s 超时,输出用 `parse_loose` 容错解析 —— 从回复里取最外层 `{…}`,找不到 JSON 就把整段当命令;`command` 为空则报错并把 `explanation` 当原因。服务不可用时提示去检查 Ollama 和模型。 | `crates/vg-core/src/ollama.rs`、`crates/vg-core/src/nl2command.rs` |
| 后端选择 | `llm-provider = auto`(有 Key 走 Claude,否则 Ollama)/ `claude`(没 Key 直接报错)/ `ollama`。API Key 优先读配置文件,其次 `$ANTHROPIC_API_KEY` —— 从 Finder / 开始菜单启动的 App 看不到 shell 环境变量,所以配置文件是主路径。 | `crates/vg-core/src/nl2command.rs` |
| 上下文 | 系统提示词里写明操作系统(macOS / Linux / Windows)、用户的 shell(`$SHELL` 末段;Windows 为 PowerShell)和当前活动分屏的工作目录,提升命令准确率;要求危险操作在解释里标注,做不到就把 `command` 留空并说明原因。 | `crates/vg-core/src/nl2command.rs`(`instructions`) |

## 4. 安全模型

- **绝不自动执行。** 识别结果只是「敲」到终端光标处(`type_text`:先滚到底部,再原样写入 PTY,不带回车),回车由你来按。
- **剥离换行。** 插入前把 `\r` / `\n` 替换成空格并去掉首尾空白,句子里混进的换行不会触发执行。听写和自然语言两条路径都过这一步。
- **空结果会提示**(「没听到声音」),不静默丢弃。
- **先看解释。** 自然语言模式在状态栏给出「『意图』→ 解释」,提示词约定要求对危险操作做出标注。
- 全 App 只有两处会真正回车,都是用户显式点击触发的:常用项目菜单的 `cd '<目录>' && claude`,以及加载器应用后的 `/reload-skills` / `/reload-plugins`(多条之间隔 2 秒依次发送,因为 claude 执行第一条斜杠命令时第二条若到达会停在输入框里不提交)。

源文件:`crates/vg-core/src/nl2command.rs`(`strip_newlines`)、`crates/vg-app/src/terminal/session.rs`(`type_text`)、`crates/vg-app/src/app.rs`(`pending_commands`)

## 5. 终端内核

| 功能 | 说明 | 源文件 |
|---|---|---|
| 真 PTY shell | alacritty_terminal 的 `Term` 状态机 + `EventLoop`(PTY 读写在它自己的 IO 线程里,UI 线程只在绘制时短暂加锁读网格)。启动 `$SHELL`(空则 `/bin/sh`),zsh / bash / fish / sh / ksh 带 `-l` 作为**登录 shell**(读 `.zprofile` 拿到完整 PATH);Windows 优先 `pwsh.exe`,没有再用 `powershell.exe`,带 `-NoLogo`。起始目录固定为用户主目录。环境变量:`TERM=xterm-256color`、`COLORTERM=truecolor`、`TERM_PROGRAM=VoiceGhostty`、`VG_STATE_FILE`(见第 9 节)。 | `crates/vg-app/src/terminal/session.rs`、`crates/vg-core/src/shell.rs` |
| 渲染 | 每帧按窗格可用区域重算列数行数并 resize PTY。自绘网格:前景/背景、反色、DIM、下划线、删除线、宽字符(CJK 字形比两格窄,放在两格正中,字距才均匀)、256 色立方与灰阶、24 位真彩;光标 block / beam / underline,失焦时只描边;光标下的字反色画。 | `crates/vg-app/src/terminal/view.rs` |
| 字体 | egui 自带字体没有中日韩字形也没有 ⌘⇧ 符号,启动时从系统字体组一套:终端用 `font-family`(在系统字体目录按文件名找,跳过 bold/italic 变体)→ macOS 的 SF Mono → egui 自带的 Hack;界面用 SF Pro(可变字体,另派生 Medium / Semibold 两个字重)→ Ubuntu;之后依次是 CJK 后备(苹方 / 冬青黑体 / 微软雅黑 / Noto CJK / 文泉驿)和符号字体。CJK 字体行高比拉丁字体大得多,中文基线会高出一截(约 0.21em),启动时实测两者基线差并用 `y_offset_factor` 拉回。<kbd>⌘+</kbd> / <kbd>⌘=</kbd> / <kbd>⌘-</kbd> / <kbd>⌘0</kbd> 实时缩放,限制在 8–48,字号是全局的,所有窗格同步;默认 13。 | `crates/vg-app/src/fonts.rs`、`crates/vg-app/src/app.rs` |
| 主题 | 暗色 / 亮色 / Solarized Dark / Dracula,每套含完整 16 色 ANSI 调色板 + 前景/背景/光标色。**整套界面的颜色**(标签栏、状态栏、浮窗、按钮、文字层次)都从终端主题推导 —— 换主题时外框跟着换,不会出现「终端是紫的、外框是灰的」。设置面板的色块可即时切换,但只在本次运行有效;长期修改写配置文件 `theme`,`foreground` / `background` 可在任意主题之上覆盖。 | `crates/vg-core/src/theme.rs`、`crates/vg-app/src/style.rs`、`crates/vg-app/src/settings_ui.rs` |
| 回滚区 | 默认每个分屏 10000 行(`scrolling_history`)。配置文件 `scrollback` 或设置面板(500–200000)可调,面板改动立即应用到已打开的分屏(缩小会马上丢掉最老的行)。 | `crates/vg-app/src/terminal/session.rs`、`crates/vg-core/src/settings.rs` |
| 清屏 | <kbd>⌘K</kbd> 发送换页符(Ctrl-L),由 shell 重绘清屏。 | `crates/vg-app/src/terminal/session.rs` |
| 选择与复制 | 鼠标拖选;双击选词、三击选行(400 ms、4 px 内算连击);单击没拖动则清掉选区。macOS <kbd>⌘C</kbd> 复制;Linux / Windows 上 egui 把 Ctrl+C 也变成复制事件,所以**带 Shift(Ctrl+Shift+C)才是复制**,单独 Ctrl+C 仍是中断(`^C`)。<kbd>⌘A</kbd> 全选历史与屏幕。程序通过 OSC 52 写剪贴板也会到系统剪贴板。 | `crates/vg-app/src/terminal/view.rs` |
| 粘贴 | <kbd>⌘V</kbd>(其他平台 Ctrl+V)。终端开着括号粘贴模式时包成 `\e[200~…\e[201~`,并先剔除内容里的结束标记,防止粘贴内容「逃出」括号直接执行;否则把换行转成 `\r`。粘贴前滚到底部。 | `crates/vg-app/src/terminal/view.rs` |
| 键盘 | xterm 风格编码:方向键 / Home / End 区分应用光标模式(DECCKM),带修饰键时用 `CSI 1;m X`;PageUp / Delete / F1–F12;Ctrl+字母 → 控制字符,Ctrl+Space → NUL;非 macOS 上 Alt+字母作为 Meta(ESC 前缀),macOS 的 Option 组合交给系统产生字符;⌘ 组合一律不发终端。可打印字符走文本事件。 | `crates/vg-app/src/terminal/keys.rs` |
| 输入法 | IME 候选框跟随终端光标;组字中的拼音画在光标处并加下划线;组字期间空格不触发按住说话。 | `crates/vg-app/src/terminal/view.rs` |
| 滚轮 | 按像素 / 行 / 页累积成整行滚动历史;在备用屏(less、vim)且程序没开鼠标模式时,滚轮转成上下方向键。 | `crates/vg-app/src/terminal/view.rs` |
| 进程退出 | shell 退出的分屏**自动关闭**(写完会话记录、挂断 PTY),空了的标签页一并关掉;全部关完就退出 App。 | `crates/vg-app/src/app.rs`(`pump`)、`crates/vg-app/src/workspace.rs`(`retain`) |

## 6. 标签页

| 功能 | 说明 |
|---|---|
| 新建 / 关闭 | <kbd>⌘T</kbd> 新建;<kbd>⌘W</kbd> 先关活动分屏,无分屏则关标签页。标签上的 ✕ 或鼠标中键也能关。关掉最后一个标签页即退出 App(这是明确的操作,不再弹确认)。 |
| 切换 | <kbd>⌘⇧]</kbd> / <kbd>⌘⇧[</kbd> 前后切换(首尾循环),<kbd>⌘1</kbd>…<kbd>⌘8</kbd> 直达,<kbd>⌘9</kbd> 最后一个(与 Ghostty 一致)。 |
| 智能标题 | 标签名由各分屏的**当前目录**推导(主目录显示为 `~`,多个分屏用 `-` 连接)。目录不靠 shell 集成,而是直接向操作系统查 shell 进程的 cwd(macOS `proc_pidinfo`,Linux `/proc/<pid>/cwd`),监视线程每 0.5 秒刷新;查不到时保留上次的值,别让标签名闪回。拿不到目录时退回终端标题;没有窗格显示 `shell`。窗口标题是「<活动分屏的终端标题> — VoiceGhostty」。 |
| 重命名 | 双击标签自定义名称;<kbd>Esc</kbd> 取消、<kbd>回车</kbd> 或失焦提交,留空则恢复自动标题。 |
| 标签栏 | 每个标签:序号、状态灯(只画非灰的,最多 4 个,忙的闪烁、完成的带光晕)、标题(截到 26 字)、关闭按钮(当前标签常驻,其他标签悬停才出现);宽度夹在 120–240 之间;悬停显示各分屏的终端标题;末尾 `+` 新建。底边一条细线在当前标签处断开,标签页和终端连成一片。 |

源文件:`crates/vg-app/src/workspace.rs`、`crates/vg-app/src/chrome.rs`(`tab_bar`)、`crates/vg-app/src/app.rs`(`tab_title`)

## 7. 分屏

每个标签页持有一棵**分屏树**:`Node` 要么是 `Leaf`(一个终端),要么是 `Branch`(一行或一列子节点)。Branch
可以嵌套,所以格数任意、布局任意 —— 一列终端旁边配一个大格、2×2 网格,都可以。树是唯一真相源,格子列表和各格的矩形
每帧由它推导;兄弟之间等分,没有拖动调整大小。

- <kbd>⌘D</kbd> 把**当前活动格**向右分、<kbd>⌘⇧D</kbd> 向下分,都可以重复按。沿着这一格已经所在的轴再分,是加一个
  **兄弟节点**,所以连按三次 <kbd>⌘D</kbd> 得到三等宽的三列,而不是 ½ + ¼ + ¼ 那种越来越窄的嵌套。沿另一条轴分,
  则把这个 leaf 换成一个两子节点的 branch —— 网格就是这么形成的。新格子获得焦点,并且**在原窗格当前所在的目录里**开 shell
  (直接向操作系统查原窗格 shell 的 cwd,刚 `cd` 完立刻分屏也是新目录;目录已不存在时回到家目录)。<kbd>⌘T</kbd> 新标签页仍从家目录开始。
- <kbd>⌘W</kbd> 关掉当前格并把焦点移到视觉上的前一格(关的是最左边时给右边那个);关到标签页只剩一格时,关的是
  整个标签页。branch 只剩一个子节点时会塌缩成那个子节点,所以关格子不会留下空壳结构。
- <kbd>⌘⌥←↑↓→</kbd> 按方向切换焦点(Ghostty `goto_split` 语义),在树上求解:向上找到最近的、轴与该方向一致的
  祖先分支,跨到那一侧的兄弟,再下降到它朝向你的那条边 —— 所以向左移动时,进入邻居子树是从**它的右侧**进。嵌套
  分屏下同样有效;那个方向没有格子时,焦点原地不动。
- <kbd>⌘⌃1…8</kbd> / <kbd>⌘⌃←→</kbd> / <kbd>⌘⌃T</kbd> **把当前格送到别的标签页** —— 用的就是该标签页自己
  <kbd>⌘N</kbd> 的数字,只差一个修饰键。shell 不中断、历史一起过去、状态灯也不会闪:会话(PTY、终端状态、记录器)
  存在按 id 索引的表里,分屏树只存 id,所以「移动」不过是从一棵树摘下、插进另一棵。落点是目标标签页当前格的右侧
  (那边按 <kbd>⌘D</kbd> 会放的位置)。把一个标签页的最后一格移走,那个(已空的)标签页会随之消失 —— 于是这个
  操作同时也是「合并标签页」。<kbd>⌘⌃←→</kbd> 首尾循环;<kbd>⌘⌃T</kbd> 把当前格移出成紧挨着的新标签页,没分屏时
  无事可做。
- egui 是即时模式 UI,不存在「一个视图被两个父级同时持有」的问题,所以旧版那套摘下 → 等拆除 → 装回的握手在这里
  不需要,移动就是一次树修改。
- 点击某一格,**语音、搜索、执行**全部路由到它 —— 鼠标按下(不等松开)就把焦点和当前标签页切过去,拖选开始时焦点
  就该过来。
- 非活动格用半透明遮罩压暗,而不是画边框。格与格之间的分隔线是 1 pt 间距透出的底层背景色。
- 分屏时标签栏按各格的视觉顺序(左→右 / 上→下)给每个非灰的格子各画一个状态点,一眼看出哪格在跑、哪格跑完了。

源文件:`crates/vg-app/src/split.rs`、`crates/vg-app/src/workspace.rs`、`crates/vg-app/src/app.rs`(`terminals`)

## 8. 状态灯(忙 / 完成)

三态圆点 —— 灰(空闲)、黄(执行中)、绿(已完成,该你了) —— 显示在标签页上;分屏时每个非灰的格子各一个点,按视觉顺序排列,
最多画 4 个,所以你能直接看出是**哪一格**在等你,而不只是「里面有一格在等」。

- **精确模式。** 一旦见过 shell 集成写的命令边界,就改由它驱动:命令开始 → 黄,命令结束 → 绿,绿灯一直保持到你去看它或
  下一条命令开始。边界**不走 OSC 133**(alacritty_terminal 会丢弃不认识的 OSC),而是 zsh 钩子把 `C <n>` / `D <n>` 写进
  每个窗格自己的状态文件(见第 9 节),监视线程每 0.5 秒读一次。带序号而不是只看 C/D:两次轮询之间跑完的快命令只会
  留下 `D n`,序号变了就知道中间发生过一条命令,照样黄一下再变绿;`D 0` 只说明 shell 已到第一个提示符、集成在工作。
- **交互式 TUI 是例外。** 对 shell 来说,`claude`(以及 vim、less)是一条一直跑到你退出为止的命令,所以只看命令边界会让灯
  整场都黄着。这类程序通过 tty 行规程识别:`sleep 4` 这种普通命令执行期间 `ICANON` 是**开**的(zsh 在 exec 之前恢复了
  规范模式),而自己读键盘的程序会把它关掉(对 pty 主端 `tcgetattr` 能读到从端的 termios)。命令在 raw 模式下运行时,灯改看
  输出活跃度:在刷屏 = 在干活(黄),静默 ≥ 2 秒 = 该你了(绿)。提示符下 zle 也是 raw,但这个判据只在「有命令在运行」时才查,
  所以那种情况不会误判。
- **启发式兜底。** 没有 shell 集成(非 zsh)时:终端响铃、或「有过输出后静默 ≥ 2 秒」算完成;近期有输出算忙。
- 你正在使用的那个单独终端保持中性 —— 终端不该用自己的灯来烦自己 —— 但仅限 VoiceGhostty 在前台、且该标签页没有分屏时。
  切到别的应用后,这个分屏和其他分屏一样正常推进:它跑完正是你离开时要等的事;切回前台它会重新变灰。分屏时每格各自
  独立报告;其他标签页里的格子始终照常推进。
- **转绿提示音**(设置 → 终端 → 完成提示音,默认开,旁边有「试听」)。不用音频文件也不依赖系统音效:用 cpal 输出流
  现场合成 —— E6 / B6 / E7 三个泛音叠加指数衰减,5 ms 起音防「咔哒」,长 0.9 秒,近似玻璃声,各平台一致。限流为每 5 秒
  最多响一次 —— 一个任务在 完成→忙→完成 之间抖动、或分屏里两格同时跑完时,听上去只是一次提醒。
- **铃比灯严格。** 只有被明确*宣告*的完成才响 —— 终端响铃,或命令结束标记 —— 交互式程序里靠静默推断出来的完成
  不响。TUI 在两次工具调用之间停顿,和 TUI 真的跑完了长得一模一样;对这种推断也响铃,会把 5 秒限流变成整个
  任务时长里的节拍器。这类停顿仍然会把灯点绿:一个可以无视的点不花什么代价,一声必须去无视的响动却要占注意力。
- 回退分支不做这个区分,也不能做。raw 模式只有在**命令运行期间**才等于「交互式程序」 —— 在提示符下 zle 同样把
  tty 置为 raw —— 而没有命令边界就没有「命令正在运行」这个状态可用来设限。所以没有 shell 集成时,静默就响,不管是不是 TUI。
- 推进在**独立的监视线程**里,每 0.5 秒一轮,不在 UI 线程 —— 最需要提示音的时候恰恰是窗口在后台、egui 不重绘的时候。
  同一轮里顺带刷新工作目录(标签标题靠它保持最新)、每 10 轮写一次会话记录、每 4 轮读一次 claude 用量。
  PTY IO 线程只用原子量记「最近输出时间 / 输出序号 / 响过铃」,监视线程读,不加锁。

源文件:`crates/vg-core/src/status.rs`(状态机与标记解析,纯逻辑、有单测)、`crates/vg-app/src/terminal/session.rs`(`PaneShared::tick`)、`crates/vg-app/src/monitor.rs`、`crates/vg-app/src/terminal/proc.rs`(`is_raw_mode`)、`crates/vg-voice/src/chime.rs`

## 9. zsh Shell 集成

通过生成在 `~/.config/voiceghostty/shell-rs/` 的 **ZDOTDIR 包装层**安装:其中的 `.zshenv` / `.zprofile` / `.zshrc` / `.zlogin`
先 source 你真正的 `~/.z*`,再追加集成内容。每次开新窗格都重新生成。因为跑在 `.zshrc` 里,所以**第一个提示符画出来之前**
一切就已就位 —— 没有启动闪烁,也完全不动你自己的提示符。(目录叫 `shell-rs/` 而不是 `shell/`,是为了不和旧 Swift 版
写的那份互相覆盖。)

- **输入行着色** —— `line-pre-redraw` ZLE 钩子给你正在敲的命令行整行上色。每次重绘都用内建 `read` 重读
  `~/.config/voiceghostty/input-color`(不起子进程),所以在设置里改颜色,所有已打开标签页立刻生效;文件读不到时用默认
  `#5CC8FF`。历史输出和 `claude` 这类全屏 TUI 完全不受影响。
- **命令边界** —— `preexec` / `precmd` 钩子把 `C <n>` / `D <n>` 用 zsh 内建重定向(无子进程)写进 `$VG_STATE_FILE`,驱动
  精确状态灯。同时仍然发出 **OSC 133** `C` / `D` 标记和 **OSC 7** 当前目录 —— VoiceGhostty 自己不用,但换到别的终端这份
  `.zshrc` 也能用。
- 导出 **`COLORTERM=truecolor`**。
- 状态文件在 `~/.config/voiceghostty/run/<App 进程号>-<窗格号>.state`,窗格关闭时删除;App 启动时清掉进程号已不存在的
  残留(上次被强杀来不及删的)。

只对 zsh 做注入,其他 shell 原样运行(于是没有精确状态灯和输入着色,靠第 8 节的启发式兜底)。

源文件:`crates/vg-core/src/shell.rs`(`prepare_zdotdir`)、`crates/vg-core/src/settings.rs`(`write_input_color_file`)、`crates/vg-app/src/terminal/session.rs`、`crates/vg-app/src/monitor.rs`(`clean_stale_state_files`)

## 10. 回滚区搜索

<kbd>⌘F</kbd> 在终端区右上角浮出搜索框。按**字面文本**搜(正则元字符全部转义);查询词全是小写时忽略大小写,含大写则
区分(alacritty 的 smart case)。边敲边搜,第一次总是从屏幕最底部**往上**找 —— 你要找的多半是刚才看到的;
<kbd>回车</kbd> 继续往上找更早的,<kbd>⇧回车</kbd> 往下找更新的,框里的 ▲ / ▼ 按钮同义。命中项用**选区**标出并滚动到可见,
所以 <kbd>⌘C</kbd> 能直接复制它;找不到显示「没有结果」。<kbd>Esc</kbd> 或 ✕ 关闭并清掉选区。搜索作用于当前活动分屏,每个
分屏各记自己的查询词;搜索框开着时键盘归它,终端不抢。

源文件:`crates/vg-app/src/terminal/search.rs`、`crates/vg-app/src/chrome.rs`(`search_box`)

## 11. 会话历史文件

每个分屏的记录追加到 `~/.config/voiceghostty/history/<文件夹名>-<yyyyMMdd-HHmmss>.log`,其中 `<文件夹名>`
取文件创建那一刻分屏所在的目录(分屏一律从主目录启动,所以等到真有输出才建文件,通常就命名成你 `cd` 进去的
那个项目;还在主目录时叫 `home`)。设置面板可开关,默认开启,对此后新开的分屏生效。

- **以终端网格为真相源,而非 pty 原始字节流。** 一行进入网格时,alacritty 已经把所有转义序列应用完了 ——
  所以重绘型 TUI(claude 的 spinner、被反复重画的消息块)落盘的是你真正看到的那一行最终状态,而不是几十行
  画了一半的残骸。宽字符的占位格丢掉、行尾空白去掉。
- **只写已经滚出屏幕的行**,因为还在屏幕上的行随时可能被重画。分屏关闭 / App 退出时的最后一次写入会把屏幕内
  剩余部分(去掉末尾空行)也补上。
- **增量追加,不是缓冲区快照** —— 即使 `scrollback` 已经把最老的行从内存里裁掉,文件里仍保留完整会话。
- **定位靠锚点。** alacritty 不提供「累计滚出多少行」:历史没满时,历史行数的增量就是新行数(精确);历史满了开始轮转、
  或者行数反而变少(`clear`、改窗口宽度导致重排)时,在当前历史里找上次写过的最后 8 行作为锚点,从它之后接着写;
  锚点前半截已被轮转出去时,剩下的后半截一定正好在历史最顶上,也能对上。锚点整个被挤出历史(两次写入之间输出冲过了
  整个回滚区,例如 `cat` 一个巨大文件),缺口记为 `[… lines dropped: scrollback limit reached …]`,而不是悄悄丢掉。
- 备用屏幕缓冲区(vim、less)跳过不记:它没有回滚区、行号自成体系,记下来只会混入垃圾。
- 每 5 秒追加一次(每 10 个监视周期),分屏关闭或 App 退出时再写最后一次。文件打不开(没权限等)就静默放弃,不每次都报。

源文件:`crates/vg-app/src/terminal/history.rs`、`crates/vg-app/src/terminal/session.rs`(`flush_history`)

## 12. Claude 上下文占用

分屏里跑着 claude 时,状态栏右端显示它的上下文占用:`claude` + 一条细进度条 + `65K / 1M` + `7%`。claude 一退出就消失。

数字**不是刮屏幕刮来的**,而是读 claude 自己的记录:Claude Code 把每条记录按行追加到
`~/.claude/projects/<把 cwd 的 / 换成 ->/<session-id>.jsonl`,其中每条 assistant 记录都带着那次请求的
`usage`。上下文 = `input + cache_creation + cache_read + output`(`cache_read` 占大头,但它同样是上下文)。

- **不花钱、不打扰。** 不额外调 API,不需要 claude 配合,claude 改版换皮也打不断 —— 它换的是画面,不是这个文件。
- **落后一次请求。** 记录是响应结束后才写的,所以**正在飞的那次请求不算在内**;悬停提示里写明了这一点。
- **认得出是不是 claude。** 用 `tcgetpgrp(pty 主端)` 拿前台进程组,再看可执行文件**路径**(macOS `proc_pidpath`,Linux
  `/proc/<pid>/exe`)里有没有 "claude" —— 原生安装包 exec 的是 `~/.local/share/claude/versions/<版本号>`,进程名是个光秃秃的
  版本号,只有路径还留着线索。cwd 也向前台进程要,而不是问 shell。npm/node 装法 exec 的是 "node",认不出来。Windows 上
  没有这套探测,功能相应缺席。
- **项目目录名两种写法都试。** 有的 claude 版本连 `.` 也换成 `-`,所以 `-Users-me-my.app` 和 `-Users-me-my-app` 都找,
  取其中最近写过的 `.jsonl`。
- **窗口大小看模型身份。** 记录里的模型附件带 `[1m]` 后缀(`claude-opus-5-5[1m]`)就按 1M 算,否则 200K;读数要是
  超过了假定的窗口,那本身就说明这是个没见过的长上下文模型 —— 信读数,而不是把进度条钉在 100%。
- **增量读,和写历史文件同一套路子。** 第一次扫全文(模型身份在文件开头),之后每次只读新追加的字节;
  claude 正写到一半的那行会留到下次再拼;文件比游标短(轮转/重写)就从头来。逐行先做子串筛选(`"usage"` / `"modelId"`)
  再解析 JSON —— 多 MB 的记录逐条解析比其余开销加起来还大。子 Agent(`isSidechain`)的回合不计 —— 它们烧的是自己的窗口。
- **每 2 秒一次**(每 4 个监视周期)。颜色:70% 以下强调色、70–90% 橙、90% 以上红(压缩大约在 90% 发生)。
- 同一个项目开两个分屏各跑一个 claude 时,两边读到的是**同一个文件**(最近写入的那个)—— 记录里没有 pid,
  没法把会话和分屏对上。鼠标悬停能看到 token 数、模型、会话 id 和文件路径,至少读数是可追溯的。

源文件:`crates/vg-core/src/claude_usage.rs`、`crates/vg-app/src/terminal/proc.rs`(`claude_working_directory`)、`crates/vg-app/src/terminal/session.rs`、`crates/vg-app/src/widgets.rs`(`claude_meter`)

## 13. Claude Code 加载器(🧩)

<kbd>⌘L</kbd> 或状态栏的 🧩 按钮打开。一个居中浮窗、**三个页签 —— 技能 / Agent / 插件**。目标项目是当前活动分屏的工作目录
(查不到时面板直说「无法确定当前终端的工作目录」);悬停目录名能看到三类东西各去哪儿。设置里添加的文件夹不再被问
「你是什么」,只被问「你有什么」,所以同一个文件夹可以同时贡献它的技能、Agent 和插件,各自进对应的页签。文件夹想加多少
加多少,页签内部仍按文件夹分组。

三个页签是同一张列表的三个视图,不是三个独立面板:在一个页签里勾的东西,切到别的页签仍然保留,
**「应用」一次提交全部三个页签**(按钮只在有改动时可点);页签标题带条数,没去过的页签值不值得点一眼一目了然。
空页签会说清楚是缺了哪个标记,而不是干脆消失。

### 13.1 一个文件夹会被识别出什么

按类型分别扫描(`crates/vg-core/src/loader.rs`),依据是 Claude Code 自己认的那些标记:

| 类型 | 靠什么认出来 | 去哪儿找 |
|---|---|---|
| 插件 | `.claude-plugin/marketplace.json`(列出它 `plugins[]` 里的每一个,名字读不出来的 marketplace 用文件夹名),否则 `.claude-plugin/plugin.json` | 文件夹根目录 |
| 技能 | 含 `SKILL.md` 的子文件夹 | `<文件夹>/skills/` 和 `<文件夹>/`,**且该文件夹没声明插件** |
| Agent | 以 YAML front matter(`---`)开头的 `.md` 文件,只读前 512 字节判断 | `<文件夹>/agents/` 和 `<文件夹>/`,**且该文件夹没声明插件** |

- **声明了插件的文件夹只贡献插件** —— 完全不去扫它的技能和 Agent。里面那些东西是插件自己的,随插件一起加载;
  再单独列出来,只会诱使你把同一样东西加载两遍(还是脱离插件、被复制进 `.claude/` 的那一份)。判定看清单文件
  而不是插件个数:一个什么都没列的 marketplace 也仍然是插件文件夹。清单损坏(不是合法 JSON)则当作没有插件,
  文件夹里的技能和 Agent 照常可加载。
- 其余文件夹里,正是 `SKILL.md` 和 front matter 这两个条件,把构建目录和 `README.md` 挡在列表之外。
- **老库兜底**:一个文件夹里哪儿都没有 `SKILL.md` 时,按老式技能库处理 —— 每个子文件夹算技能;哪儿都没有 front matter 时,
  每个顶层 `.md`(除 `README.md`、`LICENSE.md`、`CLAUDE.md` 这类文档外)算 Agent,所以在这些约定之前攒的库照样列得出来。
- 隐藏文件和文件夹(`.` 开头)一律跳过;列表按名字排序。

### 13.2 一个名字只归一个文件夹

技能、Agent、插件在项目里各自只有一个名字位置(`.claude/skills/<名字>`、`.claude/agents/<名字>.md`、
`enabledPlugins` 里的一个 key),所以两个文件夹提供同名项时不可能都加载。**设置列表里靠前的文件夹拥有这个
名字**;靠后的文件夹再提供同名项,就是一行勾不动的条目,标着「重名」,悬停说明是谁已经提供了它、以及怎么改
(在设置里移除那个文件夹或调整顺序)。面板顶部汇总当前页签跳过了多少项,设置面板则在你添加文件夹的当下就提示
新加的文件夹里有几项与上面的重名 —— 不会偷偷二选一,也不会覆盖谁。

### 13.3 应用时发生什么

- **技能和 Agent** 复制进 `cwd/.claude/skills/` 和 `cwd/.claude/agents/`(递归复制整个技能文件夹);取消勾选只删掉那一项,
  目录里别的东西一概不动。已存在的目标绝不覆盖,单项失败记进结果、不中断其余项。
- **插件**不复制。勾选它,项目的 `settings.local.json` 里会同时出现两样:`enabledPlugins` 的开关,和
  `extraKnownMarketplaces` 里指向那个文件夹的 `directory` 型注册。取消勾选则移除开关;某个市场底下一个插件
  都没启用时,连它的注册一起撤掉(只撤路径确实指向我们那个文件夹的),文件里不会攒下没人用的路径。写
  `settings.local.json` 而不是 `settings.json`,是因为注册必须写文件夹的**绝对路径** —— 那是本机的事实,放进随仓库共享的
  文件里对别人没有意义。
- 只重写 `enabledPlugins` 和 `extraKnownMarketplaces` 两个 key,文件里其他设置原样保留;也不会动不是这个
  面板注册进去的市场条目。先写临时文件再改名,写一半崩溃不会留下损坏的设置。文件本来不存在且没有要写的内容时什么
  都不写 —— 光是翻一翻面板不会给项目留下空壳文件。`settings.local.json` 是手改坏的非法 JSON 时,报错而不是覆盖它。
- **成功后往终端里敲重载命令**:有技能/Agent 变更发 `/reload-skills`,有插件变更发 `/reload-plugins`,两条都有时间隔 2 秒
  依次发送(见第 4 节),让正在跑的 Claude 会话立刻认到。状态行写明「已同步:复制 N,删除 M · 插件启用 X,停用 Y,已发送 …」,
  然后按磁盘上的实际状态重新扫描刷新勾选。
- **单个插件文件夹**(只有 `plugin.json`)勾不动,标「缺少 marketplace」:项目设置引用插件靠的是 `插件名@市场名`,这种
  文件夹没声明市场。悬停有说明,让你改为添加收录了它的 marketplace 文件夹。
- 项目里已有、但没有任何已添加文件夹提供的条目照样列出来(已勾选),文件夹后来被移除也关得掉;取消勾选即
  永久移除。

### 13.4 列表本身

- 项目里已加载的条目默认就是勾上的;整行可点。
- 每个页签内部**按文件夹分组**,顺序同设置里的顺序,标题是文件夹名(两个文件夹末段同名时显示完整路径)。
  末尾一组「**仅项目内**」:项目里有、但没有任何已添加文件夹提供的条目。
- 每组可折叠:点标题栏收起/展开,或用右上角按钮**全部展开** / **全部收起**(多于一组时才出现,只作用于当前页签,
  其他页签的折叠状态不受影响)。收起的标题仍显示 `已勾选 / 总数`,折叠状态跨面板开关记忆
  (`settings.json` 里的 `loader-groups-collapsed`,按 `<页签>/<文件夹>` 记,所以同一个文件夹可以在这个页签
  收着、在那个页签开着)。
- 行末只标「为什么勾不动」(重名、缺少 marketplace)—— 来自哪个文件夹是分组标题的事,每行再写一遍是噪音;悬停看来源
  路径或原因。
- 重新扫描按钮、实时状态行,以及对不存在的路径和读不了的设置文件给出**警告而非报错** —— 其余文件夹和已加载的条目照常列出。

源文件:`crates/vg-core/src/loader.rs`(扫描、归属、应用,纯逻辑、有单测)、`crates/vg-app/src/loader_panel.rs`(界面)、`crates/vg-app/src/app.rs`(`loader_window`)、`crates/vg-app/src/settings_ui.rs`(`library_section`)

## 14. Claude 快捷启动(✨ 常用项目)

状态栏的 ✨ 按钮弹出菜单。用原生目录选择器(rfd)保存常用项目目录;点一下即在**当前活动分屏**执行 `cd '<目录>' && claude`
并回车(路径用单引号包裹并转义,含空格也安全;Windows 用 PowerShell 的 `''` 转义)。列表项显示文件夹名、悬停看完整命令、
行末垃圾桶逐条移除,持久化在 `settings.json` 的 `claude-dirs`。菜单按文件夹名小写排序;存储的数组仍保持添加顺序,重复添加
同一目录会被忽略。

源文件:`crates/vg-app/src/chrome.rs`(`projects_menu`)、`crates/vg-core/src/shell.rs`(`launch_claude_command`、`quote`)

## 15. 设置面板(⚙️)

<kbd>⌘,</kbd> 或状态栏齿轮按钮,居中浮窗,「改动立即生效」。

| 分组 | 设置项 | 作用 |
|---|---|---|
| 语音 | 语音引擎 | `自动` / `内置 whisper` / `HTTP 接口` / `外部命令` / `关闭`,每项带一句提示(见第 2 节)。 |
| 语音 | 识别语言 | `中文` / `English` / `自动检测`,只影响语音识别(见第 16 节)。 |
| 语音 | 自动标点 | 听写结果是否带标点,默认开;主要口述 shell 命令时建议关掉。 |
| 终端 | 完成提示音 | 状态灯转绿时响一声,默认开;旁边 ▶ 试听。即时作用于监视线程。 |
| 终端 | 保存会话记录 | 会话历史开关,默认开,对此后新开的分屏生效;旁边文件夹按钮打开记录目录。 |
| 终端 | 历史行数 | 500–200000 的拖拽数值框,超范围自动夹回;立即应用到已打开的分屏。 |
| 外观 | 主题 | 四个色块(终端底色 + 三个 ANSI 色点 + 前景色一道),点选即换整套界面,**本次运行有效**;长期修改写配置文件。 |
| 外观 | 命令输入颜色 | 取色器;写入 `~/.config/voiceghostty/input-color`,由 zsh 钩子在所有已打开标签页实时读取生效。 |
| 加载文件夹 | 文件夹列表 | 🧩 面板的来源目录。**添加文件夹…** 可一次选多个;每行悬停出现「上移(优先级更高)」和「移除」;序号即重名时的优先级;添加当下提示新文件夹里有几项与上面的重名。可以全部移除且会记住 —— 没有文件夹时,🧩 面板只列出项目里已加载的条目。 |

底部一行说明「API 地址、外部识别命令、模型等更多选项写在 `~/.config/voiceghostty/config`」,点路径在文件管理器里打开所在目录;
右下角显示版本与提交号(`VoiceGhostty v0.1.0 (abc1234+)`,见第 18 节),悬停看构建时间。

这里改的设置存 `~/.config/voiceghostty/settings.json`(键:`default-language`、`voice-engine`、`adds-punctuation`、`scrollback`、
`save-history`、`input-color`、`done-sound`、`skill-library-dirs`、`loader-groups-collapsed`、`claude-dirs`)。与配置文件共有的键,
GUI 设置优先;没在 GUI 里设过的(`null`)回退配置文件。配置文件仍是进阶/兜底路径。每次保存设置也顺手把 `input-color` 文件写一遍。

所有浮窗(设置、🧩 加载器、快捷键表、退出确认)都是 egui 的居中窗口:没有默认标题栏,自绘左对齐标题 + 右上角 ✕,
<kbd>Esc</kbd> 关闭。

源文件:`crates/vg-app/src/settings_ui.rs`、`crates/vg-core/src/settings.rs`、`crates/vg-app/src/kit.rs`(窗口、设置行、开关等组件)、`crates/vg-app/src/style.rs`(设计规范)

## 16. 界面语言

界面**只有中文**:所有面向用户的字符串直接写在调用处,没有字符串表,也没有语言切换。设置里的「识别语言」(`中文` / `English` /
`自动检测`)只决定语音识别按哪种语言听(`voice-language`),不影响界面。来自 `vg-core` / `vg-voice` 的底层错误文案(如
`Invalid API Key`、`No speech engine configured`)是英文,会原样显示在状态栏;LLM 提示词是英文,但要求解释用用户说话的语言。

源文件:`crates/vg-app/src/chrome.rs`、`crates/vg-app/src/settings_ui.rs`、`crates/vg-app/src/widgets.rs`(`language_label`)

## 17. 配置文件

`~/.config/voiceghostty/config`(Windows 上是 `%APPDATA%\voiceghostty\config`),极简 `key = value`,`#` 注释,未知键、坏行和
空值直接忽略(不报错)。布尔值 `off` / `false` / `no` / `0` 为假,其他为真。**共 23 个键:**

| 键 | 默认值 | 用途 |
|---|---|---|
| `font-family` | 平台默认等宽 | 终端字体族(在系统字体目录按文件名找) |
| `font-size` | `13` | 同时也是 <kbd>⌘0</kbd> 的重置目标;须大于 0 |
| `theme` | `dark` | `dark` / `light` / `solarized-dark` / `dracula`,未知值回退 `dark` |
| `foreground` | — | `#rrggbb`,覆盖主题前景色 |
| `background` | — | `#rrggbb`,覆盖主题背景色 |
| `scrollback` | `10000` | 每个分屏保留的历史行数;设置面板的值优先 |
| `llm-provider` | `auto` | `auto` / `claude` / `ollama`(旧写法 `local`、`apple` 也映射到 `ollama`) |
| `llm-model` | `claude-opus-5-5` | Claude 模型 ID |
| `llm-api-key` | — | 不填则读 `$ANTHROPIC_API_KEY` |
| `correction` | `on` | 填 `off` 关闭自然语言模式的矫正(听写模式本来就逐字直出) |
| `llm-local-model` | `qwen3:1.7b` | Ollama 模型:矫正,以及没有 Claude Key 时生成命令 |
| `llm-local-url` | `http://127.0.0.1:11434` | Ollama 服务地址 |
| `skill-library-dir` | `~/.claude/skills` | 加载器来源目录(支持 `~`)。多个目录:用英文逗号分隔,或重复写这个键(第一次出现替换默认值,之后追加)—— 顺序即 🧩 面板的分组顺序和重名优先级;设置面板的列表优先 |
| `voice-engine` | `auto` | `auto` / `whisper` / `http` / `command` / `off`(别名:`local`=whisper,`api`/`openai`=http,`cmd`/`exec`=command,`none`/`disabled`=off);设置面板的值优先 |
| `voice-language` | `zh` | `zh` / `en` / `auto`,`zh-CN` 这类写法取横线前的部分;设置面板的值优先 |
| `voice-punctuation` | `on` | 是否要标点;设置面板的值优先 |
| `voice-api-url` | `https://api.openai.com/v1/audio/transcriptions` | HTTP 引擎地址;指向本机时自动免 Key 并绕过代理 |
| `voice-api-key` | — | 不填则依次读 `$VOICE_API_KEY`、`$OPENAI_API_KEY` |
| `voice-api-model` | `whisper-1` | HTTP 引擎的模型名 |
| `voice-command` | — | 外部命令模板,`{wav}` `{lang}` 占位 |
| `voice-model` | `<配置目录>/models/` 里第一个 `ggml-*.bin` | 内置 whisper 的 ggml 模型路径(支持 `~`) |
| `voice-device` | 系统默认 | 麦克风设备名 |
| `voice-hold-ms` | `250` | 按住空格多久算「按住说话」 |

源文件:`crates/vg-core/src/config.rs`、`crates/vg-core/src/paths.rs`

## 18. App 与窗口行为

- eframe 原生窗口,启动尺寸 960×600,最小 400×240;窗口标题随活动分屏的终端标题变化(「<标题> — VoiceGhostty」)。
- 布局:顶部标签栏(38 pt)、底部状态栏(34 pt)、中间终端区。底部状态栏**始终占位**(没消息时是空的),且永远只有一行(过长截断,
  完整内容在悬停提示里)。有消息才出现、消息一长就换行的状态条,会连带把终端的行数改掉 —— SIGWINCH 正巧落在某一帧中间时,
  全屏 TUI 就会被画错一行,直到它自己重绘才恢复。结果消息 15 秒后淡出,错误一直留到下一条。
- 状态栏从左到右:语音模式分段控件、语音胶囊(第 1 节)、上一次操作的结果;右端:✨ 常用项目、🧩 加载器、⌨ 快捷键表、⚙️ 设置,
  以及 claude 用量表(第 12 节)。
- **关窗口先确认**:系统的关闭请求被拦下,弹出模态框「退出 VoiceGhostty?」并写明有几个窗格里的 shell
  会被挂断,<kbd>回车</kbd> 退出、<kbd>Esc</kbd> 取消。例外:用 <kbd>⌘W</kbd> / ✕ 关掉最后一个分屏或标签页是明确的操作,
  直接退出不再确认;所有 shell 自己退出了也一样。
- 退出时:各窗格写完会话记录、向 PTY 发 Shutdown;然后释放内置 whisper 的模型(不释放的话 ggml 的 Metal 设备在 exit 时先析构
  会直接 abort)。
- 启动顺序:清理残留状态文件 → 读 `settings.json` 叠加到配置文件之上 → 装字体 → 应用主题 → 后台预载 whisper 模型 →
  (开着矫正时)后台预热 Ollama → 开第一个标签页。`alacritty_terminal::tty::setup_env()` 让 shell 继承正确的 locale。
- 日志走 `env_logger`,`RUST_LOG` 默认 `warn`;whisper.cpp / ggml 的日志被接进同一套,不刷 stderr。
- **跨平台**:快捷键分 macOS 与 Linux/Windows 两套(第 19 节);Windows 上用 PowerShell,且没有 PTY 探测(`tcgetattr` /
  `tcgetpgrp`)和进程表查询,所以 TUI 识别、按进程查目录、claude 用量在 Windows 上缺席,标签标题退回终端标题。
- **打包**(`make-app.sh`):`cargo build --release -p vg-app --features whisper-metal`(`VG_FEATURES` 可覆盖,留空则不编 whisper、
  不需要 cmake)→ 杀掉正在跑的旧实例(否则 `open` 只会把旧进程切到前台)→ 用 `packaging/macos/Info.plist` 与
  `AppIcon.icns` 拼出 `build/VoiceGhostty.app` → 写入版本信息 → ad-hoc 签名 → 启动;`--no-open` 只构建打包(CI 用)。
  麦克风权限弹窗依赖 bundle 里的 `NSMicrophoneUsageDescription`,所以 `cargo run` 的裸二进制拿不到权限。`LSMinimumSystemVersion` 13.0。
- **版本号单一来源**是 `Cargo.toml` 的 workspace `version`。`vg-app` 的 `build.rs` 把 git 提交号注入环境变量 `VG_GIT_COMMIT`
  (打包脚本和 CI 直接设好它,末尾 `+` 表示带未提交改动;没设时 build.rs 自己问 git)和构建时刻 `VG_BUILD_EPOCH`;设置窗口
  底部显示 `v0.1.0 (abc1234+)`,悬停看版本 / 提交 / 构建时间。打包脚本同时把版本写进 `CFBundleShortVersionString`、
  构建时间戳写进 `CFBundleVersion`、提交号写进自定义键 `VGGitCommit`。市场版本号跨构建不变,单看它分不出新旧;
  提交号才能说明跑的 .app 里有没有某个修复。
- 界面截图测试(`shots.rs`,默认跳过):用真 App + 真 shell 经 wgpu 离屏渲染出 PNG,逐张检查视觉效果。

源文件:`crates/vg-app/src/main.rs`、`crates/vg-app/src/app.rs`、`crates/vg-app/src/chrome.rs`(`status_bar`、`quit_dialog`)、`crates/vg-app/build.rs`、`crates/vg-app/src/version.rs`、`make-app.sh`、`packaging/macos/Info.plist`

## 19. 快捷键

快捷键表只有一处定义(`crates/vg-app/src/shortcuts.rs`),按键处理和 <kbd>⌘/</kbd> 的帮助窗口都读它,不会对不上。
macOS 用 ⌘ 系;Linux / Windows 上单 Ctrl 组合要留给终端(Ctrl+T / W / D 在 shell 里都有用),所以基本用 Ctrl+Shift,分屏移动再加 Alt。

| 动作 | macOS | Linux / Windows |
|---|---|---|
| 说话(松开停止;快速点按仍输入空格) | 按住 <kbd>空格</kbd> | 按住 <kbd>空格</kbd> |
| 开始 / 停止录音 | <kbd>⌘⇧M</kbd> | <kbd>Ctrl+Shift+M</kbd> |
| 切换听写 / 自然语言 | <kbd>⌘⇧N</kbd> | <kbd>Ctrl+Shift+N</kbd> |
| 新建标签页 | <kbd>⌘T</kbd> | <kbd>Ctrl+Shift+T</kbd> |
| 关闭分屏,无分屏则关标签页 | <kbd>⌘W</kbd> | <kbd>Ctrl+Shift+W</kbd> |
| 下一个 / 上一个标签页 | <kbd>⌘⇧]</kbd> / <kbd>⌘⇧[</kbd> | <kbd>Ctrl+PageDown</kbd> / <kbd>Ctrl+PageUp</kbd> |
| 跳到第 N 个 / 最后一个标签页 | <kbd>⌘1</kbd>…<kbd>⌘8</kbd> / <kbd>⌘9</kbd> | <kbd>Alt+1</kbd>…<kbd>Alt+8</kbd> / <kbd>Alt+9</kbd> |
| 向右分屏 / 向下分屏 | <kbd>⌘D</kbd> / <kbd>⌘⇧D</kbd> | <kbd>Ctrl+Shift+D</kbd> / <kbd>Ctrl+Shift+E</kbd> |
| 按方向切换分屏焦点 | <kbd>⌘⌥←↑↓→</kbd> | <kbd>Ctrl+Alt+←↑↓→</kbd> |
| 把当前分屏送到第 N 个标签页 | <kbd>⌘⌃1</kbd>…<kbd>⌘⌃8</kbd> | <kbd>Ctrl+Alt+1</kbd>…<kbd>Ctrl+Alt+8</kbd> |
| 把当前分屏送到上一个 / 下一个标签页 | <kbd>⌘⌃←</kbd> / <kbd>⌘⌃→</kbd> | <kbd>Ctrl+Alt+Shift+←</kbd> / <kbd>Ctrl+Alt+Shift+→</kbd> |
| 把当前分屏移出成新标签页 | <kbd>⌘⌃T</kbd> | <kbd>Ctrl+Alt+Shift+T</kbd> |
| 字号放大 / 缩小 / 重置 | <kbd>⌘+</kbd> <kbd>⌘=</kbd> / <kbd>⌘-</kbd> / <kbd>⌘0</kbd> | <kbd>Ctrl+Shift++</kbd> <kbd>Ctrl+Shift+=</kbd> / <kbd>Ctrl+Shift+-</kbd> / <kbd>Ctrl+Shift+0</kbd> |
| 清屏 | <kbd>⌘K</kbd> | <kbd>Ctrl+Shift+K</kbd> |
| 搜索回滚内容 | <kbd>⌘F</kbd> | <kbd>Ctrl+Shift+F</kbd> |
| 全选 | <kbd>⌘A</kbd> | <kbd>Ctrl+Shift+A</kbd> |
| 复制选区 / 粘贴 | <kbd>⌘C</kbd> / <kbd>⌘V</kbd> | <kbd>Ctrl+Shift+C</kbd> / <kbd>Ctrl+V</kbd>(单独 <kbd>Ctrl+C</kbd> 仍是中断) |
| 🧩 加载器 | <kbd>⌘L</kbd> | <kbd>Ctrl+Shift+L</kbd> |
| 设置 | <kbd>⌘,</kbd> | <kbd>Ctrl+Shift+,</kbd> |
| 快捷键表 | <kbd>⌘/</kbd> | <kbd>Ctrl+Shift+/</kbd> |

鼠标:双击标签改名、中键关标签、点击分屏切焦点、双击 / 三击终端选词 / 选行。
