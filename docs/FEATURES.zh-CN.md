# VoiceGhostty — 功能清单

按源码扫描整理的完整功能清单。每一项都标出了实现文件,所以这份文档同时也是代码地图。

[English →](FEATURES.md)

**技术栈:** SwiftUI + [SwiftTerm](https://github.com/migueldeicaza/SwiftTerm) + Apple `Speech` / `AVFoundation`,
可选 `FoundationModels`(macOS 26+)。约 3.4k 行 Swift、20 个文件,跑起来不依赖任何外部服务。

---

## 1. 语音采集

| 功能 | 说明 | 源文件 |
|---|---|---|
| 按住说话 | 按住 <kbd>空格</kbd> ≥ 0.25 秒开始录音,松开即停。低于阈值的快速点按会原样补发为普通空格,不影响正常输入;按住期间吞掉系统按键重复。 | `Voice/PushToTalk.swift` |
| 开关录音 | <kbd>⌘⇧M</kbd> 或工具栏麦克风按钮,录音中按钮变红。 | `ContentView.swift` |
| 实时字幕 | 说话过程中,部分识别结果实时显示在麦克风旁(单行,从头截断)。 | `Voice/SpeechRecognizer.swift` |
| 人声门限(VAD) | RMS 音量越过阈值后才真正建立识别任务;同时回放 6 帧预滚缓冲,保证第一个音节不丢。这样可以避免开头静音触发识别器 1110 错误。 | `Voice/SpeechRecognizer.swift` |
| 权限 | 首次使用时申请语音识别与麦克风权限,被拒时给出指名到具体「系统设置」面板的提示。用世代计数器取消权限流程中被用户中止的那次录音。 | `Voice/SpeechRecognizer.swift` |
| 端侧识别 | `SFSpeechRecognizer` 本地运行,音频不上传。 | `Voice/SpeechRecognizer.swift` |
| 自动标点 | 实时请求与兜底请求都设 `addsPunctuation`(Apple 默认是关的,所以否则听写出来是一整串没有句读的文字)。设置面板可开关,默认开,在每次识别任务创建时读取 —— 改完下一句话就生效。两处请求保持一致,避免中英仲裁把标点换进换出。 | `Voice/SpeechRecognizer.swift`、`Config/AppSettings.swift` |

## 2. 中英双语识别

**任意时刻只跑一个识别器** —— 两个端侧识别器并发会互相抢占,随机导致只有一个出结果。

- **主语言**取自设置(默认 `zh-CN`),说话过程中只跑它的实时识别;
- 全程音频同时录进缓冲区;
- 停止后对主语言结果做**仲裁**:中文为主时,只有转写里含汉字才可信(否则 zh 识别器会把英文音译成错汉字);英文为主时,非空即可信;
- 仲裁不通过,则用**备用语言**对完整录音**单独跑一次** —— 此时主任务已结束,不存在抢占;
- 每个阶段都有 2 秒兜底定时器,保证一定会给出结果。

源文件:`Voice/SpeechRecognizer.swift`

## 3. 两种语音模式

由工具栏分段选择器切换(`Voice/VoiceMode.swift`)。

### 听写模式(`text.cursor`)

识别结果**原样**落到光标处,不经过任何改写。听写要敲的多半是命令片段、路径、文件名,小模型在这类文本上改坏的概率高于改对,所以这条链路刻意保持逐字。唯一的加工是插入前把换行替换成空格(见第 4 节安全模型)。

### 自然语言模式(`sparkles`)

说意图,得到一条 shell 命令 + 一句解释(显示在状态栏)。

识别文本先过一遍**本地矫正**,再送给 LLM —— 口水词和同音错字先清掉,LLM 拿到的是干净意图。

| 功能 | 说明 | 源文件 |
|---|---|---|
| 本地小模型 | Ollama `http://127.0.0.1:11434`,默认 `qwen3:1.7b`。去语气词、去结巴重复,尽力纠正同音错字。用 few-shot 示例约束 —— 只给规则的话,1.7B 级模型会随机删实词。 | `Command/OllamaClient.swift` |
| 绝不阻塞 | 服务没起、超时(15 秒)、解析失败 → 静默使用原文,直接送 LLM。矫正只能是锦上添花,绝不能吞掉用户说的话。 | `Command/OllamaClient.swift` |
| 安全阀 | 矫正后文本相对原文缩到 50% 以下或膨胀超过 150%,判定模型跑飞,回退原文。 | `Command/OllamaClient.swift` |
| 预热 | App 启动时后台发一次 `/api/generate` 把模型载入内存(`keep_alive: 30m`),把十几秒的冷加载挪到你开口之前;失败静默忽略。 | `ContentView.swift`、`Command/OllamaClient.swift` |
| 绕过系统代理 | URLSession 用空的 `connectionProxyDictionary`,防止系统代理(Clash/Surge 等)劫持本机回环请求。 | `Command/OllamaClient.swift` |
| 结构化输出 | Ollama `format` JSON Schema 语法约束,回复只含一个 `text` 字段 —— 这也是真正压住 qwen3 思考输出的手段。 | `Command/OllamaClient.swift` |

| 后端 | 说明 | 源文件 |
|---|---|---|
| Claude API | 直接 `POST /v1/messages`(Swift 无官方 SDK)。用 **structured outputs**(`output_config.format: json_schema`)由 API 层保证 `{command, explanation}` 结构;`effort: low` 压低语音场景的延迟。HTTP 错误与 `stop_reason: refusal` 都翻译成一句人话。 | `Command/ClaudeClient.swift` |
| Apple 端侧模型 | `FoundationModels` 的 `SystemLanguageModel`(macOS 26 + Apple Intelligence)。先查可用性,每种不可用原因对应一条具体提示。代码在 `#if canImport` 后面,且让模型直接输出纯 JSON 而非用 `@Generable` 宏,以便在只有命令行工具(CLT)的环境下也能编译。 | `Command/NL2Command.swift` |
| 后端选择 | `llm-provider = auto`(有 Key 走 Claude,否则端侧)/ `claude` / `apple`。API Key 优先读配置文件,其次 `$ANTHROPIC_API_KEY`。 | `Command/NL2Command.swift` |
| 上下文 | 把当前活动分屏的工作目录注入系统提示,提升命令准确率。 | `Command/NL2Command.swift` |
| 容错解析 | 端侧路径从回复里取最外层 `{…}` 解析;找不到 JSON 就把整段当命令。 | `Command/NL2Command.swift` |

## 4. 安全模型

- **绝不自动执行。** 识别结果只是「敲」到终端光标处,回车由你来按。
- **剥离换行。** 插入前把 `\r` / `\n` 替换成空格,句子里混进的换行不会触发执行。
- **空结果会提示**(「没听清,请再说一遍」),不静默丢弃。
- **先看解释。** 自然语言模式在状态栏给出一句解释,提示词约定要求对危险操作做出标注。
- 全app只有两处会真正回车:Claude 快捷启动、技能面板的 `/reload-skills` —— 都是用户显式点击。

源文件:`ContentView.swift`(`insertIntoTerminal`)

## 5. 终端内核

| 功能 | 说明 | 源文件 |
|---|---|---|
| 真 PTY shell | SwiftTerm `LocalProcessTerminalView` 以**登录 shell**(`-zsh`)启动 `$SHELL`,起始目录固定为用户主目录(否则 `.app` 会从 `/` 启动)。 | `Terminal/TerminalController.swift` |
| 字体 | 默认系统等宽(SF Mono),也可在配置里指定字体族。<kbd>⌘+</kbd> / <kbd>⌘=</kbd> / <kbd>⌘-</kbd> / <kbd>⌘0</kbd> 实时缩放,限制在 8–48pt,并同步到所有会话。 | `Terminal/SessionStore.swift`、`Config/Config.swift` |
| 主题 | 暗色 / 亮色 / Solarized Dark / Dracula,每套含完整 16 色 ANSI 调色板 + 前景/背景/光标色。菜单「View → Theme」实时切换;配置文件可在任意主题之上覆盖前景/背景。 | `Config/Theme.swift` |
| 回滚区 | 默认每个分屏 10000 行,而 SwiftTerm 自己的默认值是 500 —— 跑 claude 这种刷屏的会话几秒就冲掉了。可通过配置文件(`scrollback`)或设置面板(500–200000)调整,面板改动会应用到已打开的分屏。在 shell 打印任何内容之前就抬高,所以启动阶段的输出不会丢。每行存储约几 KB,且每个分屏各算一份。 | `Terminal/TerminalController.swift`、`Config/AppSettings.swift` |
| 清屏 | <kbd>⌘K</kbd> 发送换页符,由 shell 重绘清屏。 | `Terminal/SessionStore.swift` |
| 复制/粘贴/全选 | <kbd>⌘C</kbd> / <kbd>⌘V</kbd> / <kbd>⌘A</kbd> 走系统「编辑」菜单(SwiftTerm 已实现响应链方法)。 | — |
| 进程退出 | 分屏上显示「进程已退出」标记,**故意不自动关闭**会话,避免误关。 | `Terminal/TerminalView.swift` |

## 6. 标签页

| 功能 | 说明 |
|---|---|
| 新建 / 关闭 | <kbd>⌘T</kbd> 新建;<kbd>⌘W</kbd> 先关活动分屏,无分屏则关标签页。始终至少保留一个标签页。 |
| 切换 | <kbd>⌘⇧]</kbd> / <kbd>⌘⇧[</kbd> 前后切换,<kbd>⌘1</kbd>…<kbd>⌘8</kbd> 直达,<kbd>⌘9</kbd> 最后一个(与 Ghostty 一致)。 |
| 智能标题 | 标签名由各分屏的**当前目录**推导(主目录显示为 `~`,多个分屏用 `-` 连接),实时轮询更新 —— 不用 OSC 进程标题,因为有些 shell 会把它设成电脑名。 |
| 重命名 | 双击标签(或右键菜单)自定义名称;<kbd>Esc</kbd> 取消、失焦提交、留空则恢复自动标题。 |
| 标签栏 | 横向可滚动,含序号、状态点、分屏标识、放大过热区的关闭按钮和 `+` 新建按钮。 |

源文件:`Terminal/SessionStore.swift`、`Terminal/TerminalView.swift`、`VoiceGhosttyApp.swift`

## 7. 分屏

每个标签页持有一棵**分屏树**:`SplitNode` 要么是 `leaf`(一个终端),要么是 `branch`(一行或一列子节点)。branch
可以嵌套,所以格数任意、布局任意 —— 一列终端旁边配一个大格、2×2 网格,都可以。树是唯一真相源,格子列表由它推导。

- <kbd>⌘D</kbd> 把**当前活动格**向右分、<kbd>⌘⇧D</kbd> 向下分,都可以重复按。沿着这一格已经所在的轴再分,是加一个
  **兄弟节点**,所以连按三次 <kbd>⌘D</kbd> 得到三等宽的三列,而不是 ½ + ¼ + ¼ 那种越来越窄的嵌套。沿另一条轴分,
  则把这个 leaf 换成一个两子节点的 branch —— 网格就是这么形成的。
- <kbd>⌘W</kbd> 关掉当前格并把焦点移到邻格;关到标签页只剩一格时,关的是整个标签页。branch 只剩一个子节点时会
  塌缩成那个子节点,所以关格子不会留下空壳结构。
- <kbd>⌘⌥←↑↓→</kbd> 按方向切换焦点(Ghostty `goto_split` 语义),在树上求解:向上找到最近的、轴与该方向一致的
  祖先分支,跨到那一侧的兄弟,再下降到它朝向你的那条边 —— 所以向左移动时,进入邻居子树是从**它的右侧**进。嵌套
  分屏下同样有效;那个方向没有格子时,焦点原地不动。
- <kbd>⌘⌃1…8</kbd> / <kbd>⌘⌃←→</kbd> / <kbd>⌘⌃T</kbd> **把当前格送到别的标签页** —— 用的就是该标签页自己
  <kbd>⌘N</kbd> 的数字,只差一个修饰键。shell 不中断、历史一起过去、状态灯也不会闪:`TerminalSession` 自己持有
  pty 和终端视图,分屏树只是引用它,所以"移动"不过是摘下来再插回去。把一个标签页的最后一格移走,那个(已空的)
  标签页会随之消失 —— 于是这个操作同时也是"合并标签页"。`Terminal ▸ Move Split to Tab` 会按名字列出各标签页 ——
  光靠快捷键你分不清哪个是哪个。
- 这次移动是**摘下 → 等拆除完成 → 装回**,而且必须如此。`TerminalHostView` 交回的是 controller 那个**永久**的
  `hostContainer`,它成立的前提是"一个格子的 representable 永不重复",而所有标签页都是常驻挂载的。若在同一次更新里
  把该格的 id 从一个标签页的 `ForEach` 里去掉、同时加进另一个,SwiftUI 完全可以先构建目的地再拆除源头 —— 于是迟到的
  拆除会把目的地刚认领的视图从层级里摘走。这正是那批 `EXC_BAD_ACCESS` 崩溃追到的"两个 representable 抢一个终端视图"。
- 这个"等"是真信号而不是猜:`dismantleNSView` 会告诉 store 旧的 representable 已经没了。**不能**改成"下一个 main
  queue 回合再装回" —— 执行拆除的那趟布局是排在已入队的 main queue block **之后**的,那样装回反而会赢下它本要避开的
  竞态。另有 0.25 秒兜底,应对 SwiftUI 干脆不拆的情况:一个卡在所有树之外的格子,shell 还活着。
- 在两棵树之间的这段时间由 `SessionStore.panesInFlight` 持有 —— 按格子 id 建表而不是单槽位,因为两次移动可能重叠,
  单槽位会丢掉前一个格子的最后一个引用,ARC 会在命令跑到一半时把它的 shell 一起收走。落地时对 `inserting` 的结果是
  **检查**而非信任:锚点格子若在等待期间被关掉,它会原样返回整棵树,而这种静默失败会留下一个没有视图的活 shell。
- 点击某一格,**语音、搜索、执行**全部路由到它 —— 焦点通过 `mouseDown` 捕获并同步回会话状态。
- 非活动格用半透明遮罩压暗,而不是画边框。格与格之间的分隔线是 1pt 间距透出的底层背景色。
- 分屏时每格各自显示状态点,一眼看出哪个在跑、哪个跑完了;标签栏上也按同样顺序给每格各画一个点。
- 渲染沿树递归(`SplitNodeView`),`ForEach` 以各节点稳定 id 为键,所以兄弟节点增删时各格身份不会错乱。

源文件:`Terminal/SessionStore.swift`、`Terminal/TerminalView.swift`

## 8. 状态灯(忙 / 完成)

三态圆点 —— 灰(空闲)、黄(执行中)、绿(已完成,该你了) —— 显示在每个标签页上;分屏时每格也各有一个。分屏的标签页在标签栏上按各格的视觉顺序(左→右 / 上→下)一格一个点,所以你能直接看出是**哪一格**在等你,而不只是"里面有一格在等"。超过 6 格时点会挤掉标签名,这时退回单个点、取其中最重要的状态。

- **精确模式。** 一旦收到任何 OSC 133 标记,命令边界就来自 shell 集成:`C` = 命令开始 → 黄,`D` = 命令结束 → 绿,绿灯一直保持到你去看它。
- **交互式 TUI 是例外。** 对 shell 来说,`claude`(以及 vim、less)是一条一直跑到你退出为止的命令,所以只看
  OSC 133 会让灯整场都黄着。这类程序通过 tty 行规程识别:`sleep 4` 这种普通命令执行期间 `ICANON` 是**开**的
  (zsh 在 exec 之前恢复了规范模式),而自己读键盘的程序会把它关掉(对 pty 主端 `tcgetattr` 能读到从端的
  termios)。命令在 raw 模式下运行时,灯改看输出活跃度:在刷屏 = 在干活(黄),静默 ≥ 2 秒 = 该你了(绿)。
  提示符下 zle 也是 raw,但这个判据只在「有命令在运行」时才查,所以那种情况不会误判。
- **启发式兜底。** 没有 shell 集成时:终端响铃、或「有过输出后静默 ≥ 2 秒」算完成;近期有输出算忙。
- 你正在使用的那个单独终端保持中性 —— 终端不该用自己的灯来烦自己 —— 但仅限 VoiceGhostty 在前台时。切到别的
  应用后,这个分屏和其他分屏一样正常推进:它跑完正是你离开时要等的事;切回前台它会重新变灰。分屏时每格各自
  独立报告。
- **转绿提示音**(设置 → 终端,默认开)。灯转绿时播放系统音 `Glass`,并限流为每 5 秒最多响一次 —— 这样
  一个任务在 完成→忙→完成 之间抖动、或分屏里两格同时跑完时,听上去只是一次提醒。
- **铃比灯严格。** 只有被明确*宣告*的完成才响 —— 终端响铃,或 OSC 133 D —— 交互式程序里靠静默推断出来的完成
  不响。TUI 在两次工具调用之间停顿,和 TUI 真的跑完了长得一模一样;对这种推断也响铃,会把 5 秒限流变成整个
  任务时长里的节拍器。这类停顿仍然会把灯点绿:一个可以无视的点不花什么代价,一声必须去无视的响动却要占注意力。
- 回退分支不做这个区分,也不能做。`isRawMode` 只有在**命令运行期间**才等于"交互式程序" —— 在提示符下 zle 同样把
  tty 置为 raw —— 而没有 OSC 133 就没有"命令正在运行"这个状态可用来设限。在那里查它会把判断整个反过来:恰好在命令
  结束的那一刻不响,却在命令跑到一半时响。所以没有 shell 集成时,静默就响,不管是不是 TUI。
- 每 0.5 秒轮询一次,同时刷新工作目录 —— 标签标题就是靠它保持最新的。

源文件:`Terminal/SessionStore.swift`、`Terminal/TerminalController.swift`、`Terminal/DoneChime.swift`

## 9. zsh Shell 集成

通过生成在 `~/.config/voiceghostty/shell/` 的 **ZDOTDIR 包装层**安装:其中的 dotfiles 先 source 你真正的
`~/.zshenv` / `~/.zprofile` / `~/.zshrc` / `~/.zlogin`,再追加集成内容。因为跑在 `.zshrc` 里,所以**第一个提示符
画出来之前**一切就已就位 —— 没有启动闪烁,也完全不动你自己的提示符。

- **输入行着色** —— `line-pre-redraw` ZLE 钩子给你正在敲的命令行整行上色。每次重绘都用内建 `read` 重读
  `~/.config/voiceghostty/input-color`(不起子进程),所以在设置里改颜色,所有已打开标签页立刻生效。历史输出和
  `claude` 这类全屏 TUI 完全不受影响。
- **OSC 133 标记** —— `preexec` / `precmd` 钩子发出命令起止标记,驱动精确状态灯。
- 导出 **`COLORTERM=truecolor`**。

只对 zsh 做注入,其他 shell 原样运行。

源文件:`Terminal/TerminalController.swift`(`prepareZDotDir`)、`Config/AppSettings.swift`

## 10. 回滚区搜索

<kbd>⌘F</kbd> 在终端下方打开搜索栏:<kbd>Return</kbd> 下一个、<kbd>⇧Return</kbd> 上一个、<kbd>↑</kbd> 上一个,
实时显示 `当前/总数`(无结果显示「无匹配」),<kbd>Esc</kbd> 或「完成」关闭并清除高亮。搜索作用于当前活动分屏。

源文件:`ContentView.swift`

## 11. 会话历史文件

每个分屏的记录追加到 `~/.config/voiceghostty/history/<文件夹名>-<yyyyMMdd-HHmmss>.log`,其中 `<文件夹名>`
取文件创建那一刻分屏所在的目录(分屏一律从主目录启动,所以等到真有输出才建文件,通常就命名成你 `cd` 进去的
那个项目)。设置面板可开关,默认开启。

- **以终端缓冲区为真相源,而非 pty 原始字节流。** 一行进入缓冲区时,SwiftTerm 已经把所有转义序列应用完了 ——
  所以重绘型 TUI(claude 的 spinner、被反复重画的消息块)落盘的是你真正看到的那一行最终状态,而不是几十行
  画了一半的残骸。
- **只写已经滚出屏幕的行**,因为还在屏幕上的行随时可能被重画。分屏关闭 / App 退出时的最后一次写入会把屏幕内
  剩余部分也补上。
- **增量追加,不是缓冲区快照** —— 即使 `scrollback` 已经把最老的行从内存里裁掉,文件里仍保留完整会话。若两次
  写入之间输出真的冲过了缓冲区(5 秒内超过 `scrollback` 行,例如 `cat` 一个巨大文件),缺口会记为
  `[… N line(s) dropped …]`,而不是悄悄丢掉。
- 备用屏幕缓冲区(vim、less)跳过不记:它没有回滚区、行号自成体系,记下来只会混入垃圾并破坏绝对行号锚点。
  `clear` 会重置行号,这一情况会被检测到并重新对齐。
- 每 5 秒追加一次(每 10 个状态轮询周期),分屏关闭或 App 退出时再写最后一次。

源文件:`Terminal/SessionLogger.swift`、`Terminal/SessionStore.swift`

## 12. Claude 上下文占用

分屏里跑着 claude 时,状态栏右端显示它的上下文占用:一条细进度条 + `111K / 1M · 11%`。claude 一退出就消失。

数字**不是刮屏幕刮来的**,而是读 claude 自己的记录:Claude Code 把每条记录按行追加到
`~/.claude/projects/<把 cwd 的 / 换成 ->/<session-id>.jsonl`,其中每条 assistant 记录都带着那次请求的
`usage`。上下文 = `input + cache_creation + cache_read + output`(`cache_read` 占大头,但它同样是上下文)。

- **不花钱、不打扰。** 不额外调 API,不需要 claude 配合,claude 改版换皮也打不断 —— 它换的是画面,不是这个文件。
- **落后一次请求。** 记录是响应结束后才写的,所以**正在飞的那次请求不算在内**。
- **认得出是不是 claude。** 用 `tcgetpgrp(pty 主端)` 拿前台进程组,再用 `proc_pidpath` 看可执行文件路径里有没有
  "claude" —— 原生安装包 exec 的是 `~/.local/share/claude/versions/<版本号>`,进程名是个光秃秃的版本号
  ("2.1.276"),只有路径还留着线索。cwd 也向前台进程要,而不是问 shell。npm/node 装法 exec 的是 "node",认不出来。
- **窗口大小看模型身份。** 记录里的模型附件带 `[1m]` 后缀(`claude-opus-5[1m]`)就按 1M 算,否则 200K;读数要是
  超过了假定的窗口,那本身就说明这是个没见过的长上下文模型 —— 信读数,而不是把进度条钉在 100%。
- **增量读,和写历史文件同一套路子。** 第一次扫全文(模型身份在文件开头),之后每次只读新追加的字节;
  claude 正写到一半的那行会留到下次再拼。子 Agent(`isSidechain`)的回合不计 —— 它们烧的是自己的窗口。
- **每 2 秒一次**(每 4 个状态轮询周期)。颜色:70% 以下灰、70–90% 橙、90% 以上红。
- 同一个项目开两个分屏各跑一个 claude 时,两边读到的是**同一个文件**(最近写入的那个)—— 记录里没有 pid,
  没法把会话和分屏对上。鼠标悬停能看到会话 id 和文件路径,至少读数是可追溯的。
- 它是自己的 `ObservableObject`,不是分屏对象上的 `@Published`:分屏被 `PaneView`(终端宿主)观察着,
  token 数一动就唤醒终端宿主是划不来的。

源文件:`Terminal/ClaudeUsageWatcher.swift`、`Terminal/TerminalController.swift`、`ContentView.swift`

## 13. Auto Clear(设置里开关)

claude 说它可以被清空了,就替你清空,并接着起下一段活 —— 上下文快满时「交接完、翻篇、继续」那一圈,不需要你守在旁边按回车。

四个条件同时成立才动手:

1. 设置里 **Auto Clear** 打开(**默认关闭**);
2. 这个分屏的前台进程是 claude;
3. 状态栏那个上下文读数 **> 150K tokens** —— 窗口还宽敞的时候清空只是白扔;
4. claude 的输出当前显示着 `***Clear Ready***`。

然后它依次输入:`/clear` ⏎ ……`/Opera:next` ⏎。

- **标记是故意宽松匹配的。** Claude Code 会渲染 markdown,`***Clear Ready***` 到屏幕上可能已经变成没有星号的样式文字。
  匹配前先把 `*` 去掉、空白折叠、转小写,原样和渲染后两种写法都吃得下。反过来 "ready to clear"、"Running /clear now"
  这类不会误判。
- **以屏幕为真相源,不看 pty 字节流** —— 和会话历史文件同一个理由:进了缓冲区的行,转义序列已经应用完,
  被拆成几次读取、或者画了一半又重画的标记,在这里就是普通的一行。
- **一个标记只会触发一次。** 触发后进入「不待命」,直到某次扫描发现标记已经不在屏幕上(`/clear` 自己就会造成这件事)
  才重新待命;另外 30 秒内绝不二次触发,防的是「标记莫名其妙留在屏幕上 → 分屏自己反复清空」这种循环。
- **只扫屏幕上的那一屏**(外加十行余量,给两次轮询之间刚滚出去的行),不往历史里翻 —— 否则上一轮的标记会一直被翻出来。
- **输入是分步的,不是一口气贴进去。** 命令和回车之间隔 0.4 秒:claude 的命令菜单随输入实时过滤,回车跟得太紧会被
  菜单里上一刻高亮的那条接走。`/clear` 之后留 1.5 秒再输入下一条命令。回车发的是 `\r` —— 终端上 Return 键发的就是它,
  claude 自己读键盘,看不到那套会接受 `\n` 的行规程。
- **触发时状态栏会说一声**(8 秒后自己退场),所以一个突然自己清空了的分屏永远不会是个谜。
- 上下文读数落后一次请求(见上一节),所以它用的是**含有该标记的那次回复**、或者更前一次的大小。

这是全 App **唯一**替你按回车的地方。语音那两条链路(听写、自然语言)一律只把文字送到光标处就停手,回车永远是你按的
(见第 4 节安全模型)—— 所以这个功能默认关闭,而且要在设置里明确打开。

源文件:`Terminal/ClaudeAutoClear.swift`、`Terminal/SessionStore.swift`、`Config/AppSettings.swift`

## 14. Claude Code 加载器(🧩)

一个面板、**三个页签 —— 技能 / Agent / 插件**。设置里添加的文件夹不再被问「你是什么」,只被问「你有什么」,
所以同一个文件夹可以同时贡献它的技能、Agent 和插件,各自进对应的页签。文件夹想加多少加多少,页签内部仍按
文件夹分组。

三个页签是同一张列表的三个视图,不是三个独立面板:在一个页签里勾的东西,切到别的页签仍然保留,
**「应用」一次提交全部三个页签**;页签标题带条数,没去过的页签值不值得点一眼一目了然。空页签会说清楚
是缺了哪个标记,而不是干脆消失。

### 14.1 一个文件夹会被识别出什么

按类型分别扫描(`Skills/LoaderLibrary.swift`),依据是 Claude Code 自己认的那些标记:

| 类型 | 靠什么认出来 | 去哪儿找 |
|---|---|---|
| 插件 | `.claude-plugin/marketplace.json`(列出它 `plugins[]` 里的每一个),否则 `.claude-plugin/plugin.json` | 文件夹根目录 |
| 技能 | 含 `SKILL.md` 的子文件夹 | `<文件夹>/skills/` 和 `<文件夹>/`,**且该文件夹没声明插件** |
| Agent | 以 YAML front matter 开头的 `.md` 文件 | `<文件夹>/agents/` 和 `<文件夹>/`,**且该文件夹没声明插件** |

- **声明了插件的文件夹只贡献插件** —— 完全不去扫它的技能和 Agent。里面那些东西是插件自己的,随插件一起加载;
  再单独列出来,只会诱使你把同一样东西加载两遍(还是脱离插件、被复制进 `.claude/` 的那一份)。判定看清单文件
  而不是插件个数:一个什么都没列的 marketplace 也仍然是插件文件夹。
- 其余文件夹里,正是 `SKILL.md` 和 front matter 这两个条件,把构建目录和 `README.md` 挡在列表之外。
- **老库兜底**:一个文件夹里哪儿都没有 `SKILL.md` 时,按老式技能库处理 —— 每个子文件夹算技能,每个顶层 `.md`
  (除 `README.md` 这类文档外)算 Agent,所以在这些约定之前攒的库照样列得出来。
- 设置面板每行文件夹后面直接写出识别结果(`1 插件 · 18 技能 · 22 Agent`),面板里会有什么,在添加它的地方
  就看得见。

### 14.2 一个名字只归一个文件夹

技能、Agent、插件在项目里各自只有一个名字位置(`.claude/skills/<名字>`、`.claude/agents/<名字>.md`、
`enabledPlugins` 里的一个 key),所以两个文件夹提供同名项时不可能都加载。**设置列表里靠前的文件夹拥有这个
名字**;靠后的文件夹再提供同名项,就是一行勾不动的条目,标着「重名」,悬停说明是谁已经提供了它。面板顶部
汇总跳过了多少项,设置面板则在你添加文件夹的当下就把重名的那个标出来 —— 不会偷偷二选一,也不会覆盖谁。

### 14.3 应用时发生什么

- **技能和 Agent** 复制进 `cwd/.claude/skills/` 和 `cwd/.claude/agents/`;取消勾选只删掉那一项,目录里别的
  东西一概不动。已存在的目标绝不覆盖,单项失败也不会中断其余项。成功变更后会往终端里敲 `/reload-skills`,
  让正在跑的 Claude 会话立刻认到新文件。
- **插件**不复制。勾选它,项目的 `settings.local.json` 里会同时出现两样:`enabledPlugins` 的开关,和
  `extraKnownMarketplaces` 里指向那个文件夹的 `directory` 型注册。取消勾选则移除开关;某个市场底下一个插件
  都没启用时,连它的注册一起撤掉,文件里不会攒下没人用的路径。写 `settings.local.json` 而不是
  `settings.json`,是因为注册必须写文件夹的**绝对路径** —— 那是本机的事实,放进随仓库共享的文件里对别人
  没有意义。插件变更需要**重启 claude 会话**才生效(不像技能有 `/reload-skills`),状态行会提示。
- 只重写 `enabledPlugins` 和 `extraKnownMarketplaces` 两个 key,文件里其他设置原样保留;也不会动不是这个
  面板注册进去的市场条目。文件本来不存在且没有要写的内容时什么都不写 —— 光是翻一翻面板不会给项目留下空壳
  文件。`settings.local.json` 是手改坏的非法 JSON 时,报错而不是覆盖它。
- **单个插件文件夹**(只有 `plugin.json`)勾不动:项目设置引用插件靠的是 `插件名@市场名`,这种文件夹没声明
  市场。悬停有说明,让你改为添加收录了它的 marketplace 文件夹。
- 项目里已有、但没有任何已添加文件夹提供的条目照样列出来(已勾选),文件夹后来被移除也关得掉;取消勾选即
  永久移除。

### 14.4 列表本身

- 项目里已加载的条目默认就是勾上的。
- 每个页签内部**按文件夹分组**,顺序同设置里的顺序,标题是文件夹名(两个文件夹末段同名时显示完整路径)。
  末尾一组「**仅项目内**」:项目里有、但没有任何已添加文件夹提供的条目。
- 每组可折叠:点标题栏收起/展开,或用标题右侧两个按钮**全部展开** / **全部收起**(只作用于当前页签,
  其他页签的折叠状态不受影响)。收起的标题仍显示 `已勾选/总数`,折叠状态跨面板开关记忆
  (`UserDefaults` 里的 `loader-groups-collapsed`,按 `<页签>/<文件夹>` 记,所以同一个文件夹可以在这个页签
  收着、在那个页签开着)。
- 行末只标「为什么勾不动」(重名、无 marketplace)—— 来自哪个文件夹是分组标题的事,每行再写一遍是噪音。
- 重新扫描按钮、实时状态行,以及对不存在的路径给出**警告而非报错** —— 其余文件夹和已加载的条目照常列出。

源文件:`Skills/LoaderPanelView.swift`、`Skills/LoaderLibrary.swift`、`Skills/ClaudePluginStore.swift`

## 15. Claude 快捷启动(✨)

用原生目录选择器保存常用项目目录;点一下即在**当前活动分屏**执行 `cd '<目录>' && claude`(路径用单引号包裹并转义,
含空格也安全)。列表项显示文件夹名、悬停看完整路径、可逐条移除,持久化在 `UserDefaults`。
菜单按文件夹名 A→Z 排序(`localizedStandardCompare`,大小写不敏感且 `proj2` 在 `proj10` 前);存储的数组仍保持添加顺序。

源文件:`ContentView.swift`

## 16. 设置面板(⚙️)

一个齿轮按钮,两个分页 —— 可编辑设置 + 完整快捷键清单。

| 设置项 | 作用 |
|---|---|
| 默认语言 | `中文` / `English`。同时切换**语音识别主语言**和**整个界面语言**。 |
| 自动加标点 | 听写结果是否带标点。默认开,下一次说话即生效。 |
| 命令输入颜色 | 取色器选择;写入 `~/.config/voiceghostty/input-color`,由 zsh 钩子在所有已打开标签页实时读取生效。 |
| 回滚行数 | 500–200000,按 <kbd>回车</kbd> 生效,已打开的分屏也会应用。超范围或填了非数字会夹到实际存下的值并回填输入框,所以框里显示的永远是真值。 |
| Auto Clear | claude 输出 `***Clear Ready***` 且上下文 > 150K 时,替你输入 `/clear` 回车、`/Opera:next` 回车。默认关闭,详见第 13 节。 |
| 把每个会话保存成文件 | 会话历史开关,同时显示保存目录并提供**打开**按钮。对此后新开的分屏生效 —— 是否记录在分屏创建时决定。 |
| 技能库文件夹 | 🧩 面板列出技能的来源目录。**添加…** 可一次选多个文件夹,每行可单独移除,🧩 面板按此顺序分组。可以全部移除且会记住 —— 没有技能库时,🧩 面板只列出项目里已加载的技能。 |

这里改的设置(存 `UserDefaults`)对共有的键优先于配置文件;配置文件仍是进阶/兜底路径。

三个工具栏面板(设置、技能、Claude 菜单)都以**窗口内覆盖层**的形式绘制在右上角,而不是原生 popover ——
这样绝不会溢出窗口边缘,点击面板外即关闭。

源文件:`SettingsView.swift`、`ShortcutsHelpView.swift`、`Config/AppSettings.swift`、`ContentView.swift`

## 17. 双语界面

所有面向用户的字符串都在调用处内联两种语言 —— `loc("Done", "完成")` —— 没有需要同步维护的字符串表。视图观察共享的
`Loc` 对象,所以在设置里切语言时整个界面立即重绘,与识别语言同步切换。

源文件:`Config/Loc.swift`

## 18. 配置文件

`~/.config/voiceghostty/config`,极简 `key = value`,`#` 注释,未知键和坏行直接忽略(不报错)。**共 13 个键:**

| 键 | 默认值 | 用途 |
|---|---|---|
| `font-family` | 系统等宽 | 终端字体族 |
| `font-size` | `12` | 同时也是 <kbd>⌘0</kbd> 的重置目标 |
| `theme` | `dark` | `dark` / `light` / `solarized-dark` / `dracula` |
| `foreground` | — | `#rrggbb`,覆盖主题前景色 |
| `background` | — | `#rrggbb`,覆盖主题背景色 |
| `llm-provider` | `auto` | `auto` / `claude` / `apple` |
| `llm-model` | `claude-opus-4-8` | Claude 模型 ID |
| `llm-api-key` | — | 不填则读 `$ANTHROPIC_API_KEY` |
| `correction` | `on` | 填 `off` 关闭自然语言模式的矫正(听写模式本来就逐字直出) |
| `llm-local-model` | `qwen3:1.7b` | 矫正用的 Ollama 模型 |
| `llm-local-url` | `http://127.0.0.1:11434` | Ollama 服务地址 |
| `skill-library-dir` | `~/.claude/skills` | 技能库来源目录(支持 `~`)。多个目录:用英文逗号分隔,或重复写这个键 —— 顺序即 🧩 面板的分组顺序 |

源文件:`Config/Config.swift`

## 19. App 与窗口行为

- 底部状态条**始终占位**(没消息时是空的),且永远只有一行(过长截断,完整内容在悬停提示里)。有消息才出现、消息一长就换行的状态条,会连带把终端的行数改掉 —— SIGWINCH 正巧落在某一帧中间时,全屏 TUI 就会被画错一行,直到它自己重绘才恢复。
- 强制 `.regular` 激活策略,让 SPM 可执行文件表现得像真正的前台 app。
- 启动时按屏幕可视区域(不含菜单栏与 Dock)的 80% 居中开窗;最小 800×500。
- 关闭最后一个窗口即退出。
- <kbd>⌘W</kbd> **故意**替换系统「文件 → 关闭」:否则菜单顺序靠前的系统项会抢走该快捷键,直接关掉整个窗口而不是一个标签页。
- `make-app.sh` 负责 release 构建、杀掉旧实例、打包含 `Info.plist` 与图标的 `.app`、ad-hoc 签名并启动。权限弹窗依赖 app bundle,所以 `swift run` 不够。

源文件:`VoiceGhosttyApp.swift`、`make-app.sh`、`Resources/Info.plist`

## 20. 快捷键

| 按键 | 动作 |
|---|---|
| 按住 <kbd>空格</kbd> | 说话(松开停止;快速点按仍输入空格) |
| <kbd>⌘⇧M</kbd> | 开始 / 停止录音 |
| <kbd>⌘T</kbd> | 新建标签页 |
| <kbd>⌘W</kbd> | 关闭分屏,无分屏则关标签页 |
| <kbd>⌘⇧]</kbd> / <kbd>⌘⇧[</kbd> | 下一个 / 上一个标签页 |
| <kbd>⌘1</kbd>…<kbd>⌘8</kbd> / <kbd>⌘9</kbd> | 跳到第 N 个 / 最后一个标签页 |
| <kbd>⌘D</kbd> / <kbd>⌘⇧D</kbd> | 向右分屏 / 向下分屏 |
| <kbd>⌘⌥←↑↓→</kbd> | 按方向切换分屏焦点 |
| <kbd>⌘⌃1</kbd>…<kbd>⌘⌃8</kbd> | 把当前分屏送到第 N 个标签页 |
| <kbd>⌘⌃←</kbd> / <kbd>⌘⌃→</kbd> | 把当前分屏送到上一个 / 下一个标签页 |
| <kbd>⌘⌃T</kbd> | 把当前分屏移出成新标签页 |
| <kbd>⌘+</kbd> <kbd>⌘=</kbd> / <kbd>⌘-</kbd> / <kbd>⌘0</kbd> | 字号放大 / 缩小 / 重置 |
| <kbd>⌘K</kbd> | 清屏 |
| <kbd>⌘F</kbd> | 搜索回滚内容 |
| <kbd>⌘C</kbd> / <kbd>⌘V</kbd> / <kbd>⌘A</kbd> | 复制 / 粘贴 / 全选 |
| <kbd>⌘Q</kbd> | 退出 —— 先弹确认框 |
