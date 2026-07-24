import SwiftUI
import AppKit

struct ContentView: View {
    @EnvironmentObject private var store: SessionStore
    @StateObject private var speech = SpeechRecognizer()
    @State private var mode: VoiceMode = .dictation
    @State private var statusText = ""
    @State private var searchText = ""
    @State private var searchSummary = ""
    @State private var showShortcuts = false
    @State private var showSkillPanel = false
    /// 常用项目目录(一键 cd 并启动 Claude),持久化在 UserDefaults
    @State private var claudeDirs: [String] =
        UserDefaults.standard.stringArray(forKey: "claude-dirs") ?? []
    @FocusState private var searchFocused: Bool
    /// 按住空格说话的键盘监听(长按 ≥0.25s 录音,快速点按仍输入空格)
    private let pushToTalk = PushToTalkMonitor()

    var body: some View {
        VStack(spacing: 0) {
            topBar               // 标签栏 + 语音控件,同一行
            Divider()
            terminalArea
            if store.searchVisible {
                Divider()
                searchBar
            }
            // 状态/错误只在需要时冒出一条,平时终端占满到底
            if speech.errorMessage != nil || !statusText.isEmpty {
                Divider()
                statusBar
            }
        }
        .onAppear {
            speech.onFinalResult = { text in handleRecognized(text) }
            // 按住空格说话:按下即录,松开即停
            pushToTalk.onHoldStart = { speech.startRecording() }
            pushToTalk.onHoldEnd = { speech.stop() }
            pushToTalk.start()
            // 后台预热听写矫正模型(冷载入十几秒,挪到用户开口之前),失败静默
            Task.detached(priority: .background) {
                let config = Config.load()
                guard config.correctionEnabled else { return }
                await OllamaClient.warmUp(model: config.llmLocalModel, baseURL: config.llmLocalURL)
            }
        }
        .onDisappear { pushToTalk.stop() }
        .onChange(of: store.searchVisible) { visible in
            if visible { searchFocused = true } else { clearSearch() }
        }
    }

    // MARK: - 终端区(活动标签的内容)

    @ViewBuilder
    private var terminalArea: some View {
        if let tab = store.activeTab {
            TabContentView(tab: tab)
                .id(tab.id)
        } else {
            Color.black
        }
    }

    // MARK: - 搜索栏(⌘F)

    private var searchBar: some View {
        HStack(spacing: 8) {
            Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
            TextField("搜索滚动区 — 回车下一个,⇧回车上一个", text: $searchText)
                .textFieldStyle(.plain)
                .focused($searchFocused)
                .onSubmit { findNext() }
                .onChange(of: searchText) { _ in updateSummary() }
            if !searchSummary.isEmpty {
                Text(searchSummary).font(.caption).foregroundStyle(.secondary)
            }
            Button { findPrevious() } label: { Image(systemName: "chevron.up") }
                .keyboardShortcut(.upArrow, modifiers: [])
                .disabled(searchText.isEmpty)
            Button { findNext() } label: { Image(systemName: "chevron.down") }
                .disabled(searchText.isEmpty)
            Button("完成") { store.searchVisible = false }
                .keyboardShortcut(.cancelAction)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(.bar)
    }

    // MARK: - 顶栏(标签栏 + 语音控件同一行)

    private var topBar: some View {
        HStack(spacing: 8) {
            TabBarView(store: store)
                .frame(maxWidth: .infinity, alignment: .leading)   // 标签栏占弹性宽度
            voiceControls
                .padding(.trailing, 10)
        }
        .background(.bar)
    }

    private var voiceControls: some View {
        HStack(spacing: 8) {
            Button {
                speech.toggle()
            } label: {
                Image(systemName: speech.isRecording ? "mic.fill" : "mic")
            }
            .keyboardShortcut("m", modifiers: [.command, .shift])
            .help(speech.isRecording ? "停止录音 (⌘⇧M)" : "开始录音 (⌘⇧M);或按住空格说话")
            .tint(speech.isRecording ? .red : nil)

            Picker("", selection: $mode) {
                ForEach(VoiceMode.allCases) { m in
                    Image(systemName: m.icon).tag(m)
                }
            }
            .pickerStyle(.segmented)
            .frame(width: 84)
            .labelsHidden()
            .help("识别模式:听写(text.cursor) / 自然语言(sparkles)")

            if speech.isRecording {
                Text(speech.liveTranscript.isEmpty ? "聆听中…" : speech.liveTranscript)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.head)
                    .frame(maxWidth: 200, alignment: .leading)
            }

            Button {
                showShortcuts.toggle()
            } label: {
                Image(systemName: "keyboard")
            }
            .help("查看快捷键")
            .popover(isPresented: $showShortcuts, arrowEdge: .bottom) {
                ShortcutsHelpView()
            }

            Button {
                showSkillPanel.toggle()
            } label: {
                Image(systemName: "puzzlepiece.extension")
            }
            .help("Skill 动态加载:勾选技能同步到当前目录的 .claude/skills")
            .popover(isPresented: $showSkillPanel, arrowEdge: .bottom) {
                SkillPanelView(currentDirectory: store.activeSession?.currentDirectory,
                               onDone: { showSkillPanel = false },
                               runInTerminal: { cmd in
                                   guard let session = store.activeSession else { return }
                                   let view = session.controller.terminalView
                                   view.window?.makeFirstResponder(view)
                                   // 用 \r(Enter 键)而非 \n:claude 等 TUI 在 raw 模式下认 \r
                                   session.controller.type(cmd + "\r")
                               })
            }

            claudeMenu
        }
        .controlSize(.small)
    }

    // MARK: - Claude 快捷启动菜单(选目录 → 在活动格 cd 进去并启动 claude)

    private var claudeMenu: some View {
        Menu {
            if claudeDirs.isEmpty {
                Text("还没有常用目录,先「添加目录…」")
            }
            ForEach(claudeDirs, id: \.self) { dir in
                Button {
                    launchClaude(in: dir)
                } label: {
                    Label(dirDisplayName(dir), systemImage: "folder")
                }
                .help(dir)
            }
            Divider()
            Button {
                addClaudeDir()
            } label: {
                Label("添加目录…", systemImage: "plus")
            }
            if !claudeDirs.isEmpty {
                Menu("删除") {
                    ForEach(claudeDirs, id: \.self) { dir in
                        Button(role: .destructive) {
                            removeClaudeDir(dir)
                        } label: {
                            Label(dirDisplayName(dir), systemImage: "trash")
                        }
                    }
                }
            }
        } label: {
            Image(systemName: "sparkles.square.filled.on.square")
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .help("常用项目:cd 到目录并启动 Claude(发送到当前活动格)")
    }

    /// 菜单里显示 ~/ 缩写路径的最后一段,悬停 help 看全路径
    private func dirDisplayName(_ dir: String) -> String {
        (dir as NSString).lastPathComponent
    }

    private func addClaudeDir() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.prompt = "添加"
        panel.message = "选择一个项目目录(点击菜单项时会 cd 进去并启动 claude)"
        guard panel.runModal() == .OK, let path = panel.url?.path else { return }
        guard !claudeDirs.contains(path) else { return }
        claudeDirs.append(path)
        saveClaudeDirs()
    }

    private func removeClaudeDir(_ dir: String) {
        claudeDirs.removeAll { $0 == dir }
        saveClaudeDirs()
    }

    private func saveClaudeDirs() {
        UserDefaults.standard.set(claudeDirs, forKey: "claude-dirs")
    }

    /// 在当前活动格执行 cd + claude。
    /// 注:这里是用户点击菜单的明确意图,与语音安全底线(绝不自动回车)不冲突。
    private func launchClaude(in dir: String) {
        guard let session = store.activeSession else { return }
        // 单引号包裹 + 转义,防止路径里的空格/特殊字符拆命令
        let quoted = "'" + dir.replacingOccurrences(of: "'", with: "'\\''") + "'"
        let view = session.controller.terminalView
        view.window?.makeFirstResponder(view)
        session.controller.run("cd \(quoted) && claude")
        store.focusActive()
    }

    // MARK: - 状态条(仅错误 / 自然语言解释时出现)

    private var statusBar: some View {
        Group {
            if let err = speech.errorMessage {
                Text(err).foregroundStyle(.red)
            } else {
                Text(statusText).foregroundStyle(.secondary)
            }
        }
        .font(.caption)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 10)
        .padding(.vertical, 4)
        .background(.bar)
    }

    // MARK: - 语音结果处理(双模式在这里分叉,均直接送入终端光标处)

    private func handleRecognized(_ text: String) {
        switch mode {
        case .dictation:
            let config = Config.load()
            guard config.correctionEnabled else {
                insertIntoTerminal(text)
                statusText = ""
                return
            }
            // 本地小模型矫正(去语气词/纠错别字);任何失败都静默回落原文,绝不阻塞听写
            statusText = "矫正中…"
            Task {
                let corrected = (try? await OllamaClient.correct(text,
                                                                 model: config.llmLocalModel,
                                                                 baseURL: config.llmLocalURL)) ?? text
                await MainActor.run { insertIntoTerminal(corrected) }
            }

        case .naturalLanguage:
            statusText = "正在转换…"
            let cwd = store.activeSession?.currentDirectory
            Task {
                do {
                    let r = try await NL2Command.translate(text, currentDirectory: cwd)
                    await MainActor.run {
                        insertIntoTerminal(r.command)
                        statusText = r.explanation   // 显示一句解释,降低误执行风险
                    }
                } catch {
                    await MainActor.run { statusText = error.localizedDescription }
                }
            }
        }
    }

    /// 把识别结果送到活动会话的终端光标处。
    /// ★ 安全底线:剥除换行(防止一句里混进 \n 触发执行),且绝不自动回车。
    private func insertIntoTerminal(_ raw: String) {
        let sanitized = raw
            .replacingOccurrences(of: "\r", with: " ")
            .replacingOccurrences(of: "\n", with: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard let session = store.activeSession else { return }
        guard !sanitized.isEmpty else {
            statusText = "没听清,请再说一次"
            return
        }
        // 先把终端设为第一响应者(同步),再送入文字
        let view = session.controller.terminalView
        view.window?.makeFirstResponder(view)
        session.controller.type(sanitized)
        store.focusActive()
        statusText = ""   // 文字已进终端光标处,那里就是反馈
    }

    // MARK: - 搜索

    private func findNext() {
        guard let view = store.activeSession?.controller.terminalView, !searchText.isEmpty else { return }
        _ = view.findNext(searchText)
        updateSummary()
    }

    private func findPrevious() {
        guard let view = store.activeSession?.controller.terminalView, !searchText.isEmpty else { return }
        _ = view.findPrevious(searchText)
        updateSummary()
    }

    private func updateSummary() {
        guard let view = store.activeSession?.controller.terminalView, !searchText.isEmpty else {
            searchSummary = ""
            return
        }
        let (index, total) = view.searchMatchSummary(searchText)
        searchSummary = total == 0 ? "无匹配" : "\(index)/\(total)"
    }

    private func clearSearch() {
        store.activeSession?.controller.terminalView.clearSearch()
        searchText = ""
        searchSummary = ""
    }
}
