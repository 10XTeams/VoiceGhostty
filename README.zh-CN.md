# VoiceGhostty

> [English →](README.md)

Mac 原生语音终端:SwiftUI + SwiftTerm + Apple Speech 框架。

## 架构

```
麦克风 (AVAudioEngine)
   → 语音识别 (SFSpeechRecognizer, 设备端)
   → 文本
   ├─ [听写模式] 本地小模型矫正(Ollama + qwen3:1.7b,去语气词/纠错别字,
   │             服务没起或超时则静默用原文)
   └─ [自然语言模式] LLM 生成命令 + 中文解释
        ├─ Claude API(配置了 llm-api-key 时优先,structured outputs 保证 JSON)
        └─ Apple 端侧模型(FoundationModels,无 Key 时回落,需 macOS 26+)
   → 剥离换行 → 直接填到终端光标处(★ 绝不自动回车执行 ★)
   → 用户在 shell 里回车 → SwiftTerm → PTY → zsh
```

## 构建运行

```bash
./make-app.sh
```

> 权限弹窗(麦克风/语音识别)依赖 app bundle 里的 Info.plist,
> 所以用 `make-app.sh` 打包运行,不要直接 `swift run`。

## 使用

### 语音
- **按住空格说话**(≥0.25 秒),松开即停;快速点按空格仍是正常输入空格
- `⌘D` 或点"说话"开始录音,再按一次停止
- 识别结果**直接填到终端光标处**(剥离换行、不自动回车),用 shell 行编辑,你自己回车执行(路由到当前活动会话)
- **中英自动识别**:同一路音频同时喂给 zh-CN / en-US 双识别器竞速,
  含汉字选中文结果,纯英文比置信度;中英混说(如"帮我 ls 一下")也能识别

### 终端(P0 已实现)
- **标签**:`⌘T` 新建、`⌘W` 关闭、`⌘⇧]`/`⌘⇧[` 前后切换、`⌘1…⌘9` 直达
- **分屏**:`⌘⇧D` 左右、`⌘⇧E` 上下(每标签最多 2 格),`⌘⌥→` 切换焦点;
  点击某一格即把语音/执行路由到它,活动格有高亮边框
- **字号**:`⌘+` 放大、`⌘-` 缩小、`⌘0` 恢复默认
- **主题**:菜单「显示 → 主题」内置 暗色 / 亮色 / Solarized 暗 / Dracula
- **搜索**:`⌘F` 打开滚动区搜索,回车下一个、⇧回车上一个,显示 `当前/总数`
- **复制/粘贴/全选**:系统「编辑」菜单(`⌘C`/`⌘V`/`⌘A`)
- **Claude 快捷启动**:工具栏 ✨ 下拉菜单,保存常用项目目录,点一下即在当前活动格
  执行 `cd <目录> && claude`;「添加目录…」新增、「删除」子菜单移除,列表持久保存

### 配置文件
`~/.config/voiceghostty/config`(`key = value`,`#` 注释),支持 10 个键:
```
font-family = Menlo
font-size   = 14
theme       = dracula     # dark | light | solarized-dark | dracula
foreground  = #f8f8f2     # 可选,覆盖主题前景
background  = #282a36     # 可选,覆盖主题背景

# 自然语言模式的 LLM
llm-provider = auto           # auto | claude | apple(auto:有 Key 走 Claude,否则端侧)
llm-model    = claude-opus-4-8
llm-api-key  = sk-ant-...     # 不填则读 ANTHROPIC_API_KEY 环境变量

# 听写矫正(本地离线小模型,去语气词/纠错别字)
correction      = on          # on | off
llm-local-model = qwen3:1.7b  # 矫正用的 Ollama 模型
llm-local-url   = http://127.0.0.1:11434
```
> 从 Finder 启动的 app 拿不到 shell 的环境变量,所以推荐把 Key 写进配置文件
> (记得 `chmod 600 ~/.config/voiceghostty/config`)。

### 听写矫正模型(可选,断网可用)
```bash
brew install ollama
brew services start ollama    # 后台服务,开机自启
ollama pull qwen3:1.7b        # ~1.4GB,矫正任务足够
```
装好后无需任何配置,听写默认过一遍矫正(app 启动时自动预热模型);
Ollama 没在跑或矫正超时(15 秒)会静默使用原文,绝不阻塞听写。

## 路线图

- [x] 1. SwiftTerm 起 zsh,可敲命令
- [x] 2. 语音听写 → 待确认区 → 回车执行
- [x] 3. 待确认区 UI(可编辑、手动确认)
- [x] **P0. 标签 / 分屏 / 字号 / 主题 / 搜索 / 配置文件**
- [x] 4. 自然语言 → 命令 + 解释:Apple 端侧模型 FoundationModels(`NL2Command.swift`,需 macOS 26 + Apple Intelligence)
- [x] P1 #5. 接 Claude API 做 NL2Command(`ClaudeClient.swift`,structured outputs;`llm-provider` 切换)
- [x] P1 #8(部分). 听写矫正:本地小模型去语气词/纠错别字(Ollama + qwen3:1.7b,`OllamaClient.swift`)
- [ ] P1. 命令上下文感知、危险命令分级、语音修正、TTS 朗读
- [ ] 远期: 终端内核迁移 libghostty(性能)

> 完整待办见 `backlog.htm`。
