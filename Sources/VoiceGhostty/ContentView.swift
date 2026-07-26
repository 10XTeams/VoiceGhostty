import SwiftUI
import AppKit

struct ContentView: View {
    @EnvironmentObject private var store: SessionStore
    @ObservedObject private var loc = Loc.shared
    @StateObject private var speech = SpeechRecognizer()
    @State private var mode: VoiceMode = .dictation
    @State private var statusText = ""
    @State private var searchText = ""
    @State private var searchSummary = ""
    @State private var showSettings = false
    @State private var showSkillPanel = false
    @State private var showClaudeMenu = false
    /// Frequently used project directories (one-tap cd and launch Claude), persisted in UserDefaults
    @State private var claudeDirs: [String] =
        UserDefaults.standard.stringArray(forKey: "claude-dirs") ?? []
    @FocusState private var searchFocused: Bool
    /// Push-to-talk keyboard monitor (hold ≥0.25s to record; a quick tap still types a space)
    private let pushToTalk = PushToTalkMonitor()

    var body: some View {
        VStack(spacing: 0) {
            topBar               // Tab bar + voice controls on the same row
            Divider()
            terminalArea
            if store.searchVisible {
                Divider()
                searchBar
            }
            // Status/error surfaces only a single line when needed; otherwise the terminal fills to the bottom
            if speech.errorMessage != nil || !statusText.isEmpty {
                Divider()
                statusBar
            }
        }
        // Toolbar panels are drawn as in-window overlays (not floating popovers) so they can never spill
        // past the window edge, no matter how far right their trigger button sits.
        .overlay(alignment: .topTrailing) {
            if showSettings {
                settingsOverlay
            } else if showSkillPanel {
                skillOverlay
            } else if showClaudeMenu {
                claudeOverlay
            }
        }
        .onAppear {
            speech.onFinalResult = { text in handleRecognized(text) }
            // Push-to-talk: press to record, release to stop
            pushToTalk.onHoldStart = { speech.startRecording() }
            pushToTalk.onHoldEnd = { speech.stop() }
            pushToTalk.start()
            // Warm up the transcript-correction model in the background (cold load takes ~10s, moved ahead of the user speaking); fail silently
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

    // MARK: - Terminal area (content of the active tab)

    /// Every tab stays mounted; only the active one is visible.
    ///
    /// The obvious alternative — rendering just `store.activeTab` behind `.id(tab.id)` — rebuilds the whole
    /// pane subtree on every switch, so for one update cycle two representables exist for the same pane and
    /// both want the one terminal view. That fight is what left SwiftUI's layout cache holding freed
    /// children. Keeping tabs mounted also matches what they already do: a background tab's shell, status
    /// light and transcript all keep running regardless.
    private var terminalArea: some View {
        ZStack {
            Color.black
            ForEach(store.tabs) { tab in
                TabContentView(tab: tab, isVisible: tab.id == store.activeTab?.id)
            }
        }
    }

    // MARK: - Search bar (⌘F)

    private var searchBar: some View {
        HStack(spacing: 8) {
            Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
            TextField(loc("Search scrollback — Return for next, ⇧Return for previous",
                          "搜索回滚内容 — Return 下一个,⇧Return 上一个"), text: $searchText)
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
            Button(loc("Done", "完成")) { store.searchVisible = false }
                .keyboardShortcut(.cancelAction)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(.bar)
    }

    // MARK: - Top bar (tab bar + voice controls on the same row)

    private var topBar: some View {
        HStack(spacing: 8) {
            TabBarView(store: store)
                .frame(maxWidth: .infinity, alignment: .leading)   // Tab bar takes the flexible width
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
            .help(speech.isRecording ? loc("Stop recording (⌘⇧M)", "停止录音(⌘⇧M)")
                                     : loc("Start recording (⌘⇧M); or hold Space to talk", "开始录音(⌘⇧M);或按住空格说话"))
            .tint(speech.isRecording ? .red : nil)

            Picker("", selection: $mode) {
                ForEach(VoiceMode.allCases) { m in
                    Image(systemName: m.icon).tag(m)
                }
            }
            .pickerStyle(.segmented)
            .frame(width: 84)
            .labelsHidden()
            .help(loc("Recognition mode: Dictation (text.cursor) / Natural language (sparkles)",
                      "识别模式:听写(text.cursor)/ 自然语言(sparkles)"))

            if speech.isRecording {
                Text(speech.liveTranscript.isEmpty ? loc("Listening…", "聆听中…") : speech.liveTranscript)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.head)
                    .frame(maxWidth: 200, alignment: .leading)
            }

            Button {
                showSkillPanel.toggle()
            } label: {
                Image(systemName: "puzzlepiece.extension")
            }
            .help(loc("Dynamic skill loading: checked skills sync to the current directory's .claude/skills",
                      "动态加载技能:勾选的技能会同步到当前目录的 .claude/skills"))

            claudeMenu

            Button {
                showSettings.toggle()
            } label: {
                Image(systemName: "gearshape")
            }
            .help(loc("Settings & shortcuts", "设置与快捷键"))
        }
        .controlSize(.small)
    }

    // MARK: - In-window toolbar panels (kept fully inside the window bounds, unlike floating popovers)

    /// Wrap panel content in a dismiss-on-outside-tap, top-right-anchored floating card.
    @ViewBuilder
    private func floatingPanel<Content: View>(dismiss: @escaping () -> Void,
                                              @ViewBuilder content: () -> Content) -> some View {
        ZStack(alignment: .topTrailing) {
            // Full-window tap-catcher: click anywhere outside the panel to dismiss
            Color.black.opacity(0.001)
                .contentShape(Rectangle())
                .onTapGesture(perform: dismiss)
            content()
                .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12))
                .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.secondary.opacity(0.3)))
                .shadow(radius: 18, y: 6)
                .padding(.top, 44)   // Clear the toolbar so it reads as a dropdown from the trigger button
                .padding(.trailing, 10)
        }
    }

    private var settingsOverlay: some View {
        floatingPanel(dismiss: { showSettings = false }) {
            SettingsView(onDone: { showSettings = false })
        }
    }

    private var skillOverlay: some View {
        floatingPanel(dismiss: { showSkillPanel = false }) {
            SkillPanelView(currentDirectory: store.activeSession?.currentDirectory,
                           onDone: { showSkillPanel = false },
                           runInTerminal: { cmd in
                               guard let session = store.activeSession else { return }
                               let view = session.controller.terminalView
                               view.window?.makeFirstResponder(view)
                               // Use \r (Enter key) rather than \n: TUIs like claude recognize \r in raw mode
                               session.controller.type(cmd + "\r")
                           })
        }
    }

    // MARK: - Claude quick-launch button (pick a directory → cd into it in the active pane and launch claude)

    private var claudeMenu: some View {
        Button {
            showClaudeMenu.toggle()
        } label: {
            Image(systemName: "sparkles.square.filled.on.square")
        }
        .help(loc("Frequent projects: cd into a directory and launch Claude (sent to the current active pane)",
                  "常用项目:cd 进入目录并启动 Claude(发送到当前活动分屏)"))
    }

    /// In-window dropdown for the Claude quick-launch button (a native Menu would spill past the window edge).
    private var claudeOverlay: some View {
        floatingPanel(dismiss: { showClaudeMenu = false }) {
            VStack(alignment: .leading, spacing: 4) {
                if claudeDirs.isEmpty {
                    Text(loc("No saved directories yet — use \"Add Directory…\"",
                             "还没有保存的目录 —— 用「添加目录…」"))
                        .font(.caption).foregroundStyle(.secondary)
                        .padding(.horizontal, 8).padding(.vertical, 4)
                }
                ForEach(claudeDirs, id: \.self) { dir in
                    HStack(spacing: 6) {
                        Button {
                            launchClaude(in: dir)
                            showClaudeMenu = false
                        } label: {
                            Label(dirDisplayName(dir), systemImage: "folder")
                                .lineLimit(1)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                        .buttonStyle(.plain)
                        .help(dir)
                        Button(role: .destructive) {
                            removeClaudeDir(dir)
                        } label: {
                            Image(systemName: "trash").font(.caption)
                        }
                        .buttonStyle(.plain)
                        .foregroundStyle(.secondary)
                        .help(loc("Remove", "移除"))
                    }
                    .padding(.horizontal, 8).padding(.vertical, 4)
                }
                Divider()
                Button {
                    addClaudeDir()
                } label: {
                    Label(loc("Add Directory…", "添加目录…"), systemImage: "plus")
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .buttonStyle(.plain)
                .padding(.horizontal, 8).padding(.vertical, 4)
            }
            .padding(8)
            .frame(width: 260)
        }
    }

    /// Show the last path component in the menu; hover the help tooltip to see the full path
    private func dirDisplayName(_ dir: String) -> String {
        (dir as NSString).lastPathComponent
    }

    private func addClaudeDir() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.prompt = loc("Add", "添加")
        panel.message = loc("Choose a project directory (selecting its menu item will cd into it and launch claude)",
                            "选择一个项目目录(在菜单中点击它会 cd 进入并启动 claude)")
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

    /// Run cd + claude in the current active pane.
    /// Note: this is the user's explicit intent from clicking the menu, so it does not conflict with the voice safety rule (never auto-Return).
    private func launchClaude(in dir: String) {
        guard let session = store.activeSession else { return }
        // Wrap in single quotes + escape, so spaces/special characters in the path don't split the command
        let quoted = "'" + dir.replacingOccurrences(of: "'", with: "'\\''") + "'"
        let view = session.controller.terminalView
        view.window?.makeFirstResponder(view)
        session.controller.run("cd \(quoted) && claude")
        store.focusActive()
    }

    // MARK: - Status bar (appears only for errors / natural-language explanations)

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

    // MARK: - Voice result handling (the two modes branch here, both fed straight to the terminal cursor)

    private func handleRecognized(_ text: String) {
        switch mode {
        case .dictation:
            // Verbatim: what you said is what lands at the cursor. Dictation is often half command
            // fragments and paths, where a correction pass is more likely to damage than to help.
            // (insertIntoTerminal clears the status bar itself — don't clear it here, or the
            // "didn't catch that" hint would be wiped out immediately.)
            insertIntoTerminal(text)

        case .naturalLanguage:
            let config = Config.load()
            statusText = loc("Converting…", "转换中…")
            let cwd = store.activeSession?.currentDirectory
            Task {
                // Tidy the transcript with the local small model first (strip filler words / fix typos),
                // so the LLM gets a clean intent; any failure silently falls back to the raw transcript
                let cleaned: String
                if config.correctionEnabled {
                    cleaned = (try? await OllamaClient.correct(text,
                                                               model: config.llmLocalModel,
                                                               baseURL: config.llmLocalURL)) ?? text
                } else {
                    cleaned = text
                }
                do {
                    let r = try await NL2Command.translate(cleaned, currentDirectory: cwd)
                    await MainActor.run {
                        insertIntoTerminal(r.command)
                        statusText = r.explanation   // Show a one-line explanation to reduce the risk of running the wrong command
                    }
                } catch {
                    await MainActor.run { statusText = error.localizedDescription }
                }
            }
        }
    }

    /// Send the recognized result to the active session's terminal cursor.
    /// ★ Safety rule: strip newlines (so a stray \n in a sentence can't trigger execution), and never auto-Return.
    private func insertIntoTerminal(_ raw: String) {
        let sanitized = raw
            .replacingOccurrences(of: "\r", with: " ")
            .replacingOccurrences(of: "\n", with: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard let session = store.activeSession else { return }
        guard !sanitized.isEmpty else {
            statusText = loc("Didn't catch that, please say it again", "没听清,请再说一遍")
            return
        }
        // Make the terminal the first responder (synchronously) first, then send the text
        let view = session.controller.terminalView
        view.window?.makeFirstResponder(view)
        session.controller.type(sanitized)
        store.focusActive()
        statusText = ""   // The text is now at the terminal cursor — that is the feedback
    }

    // MARK: - Search

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
        searchSummary = total == 0 ? loc("No matches", "无匹配") : "\(index)/\(total)"
    }

    private func clearSearch() {
        store.activeSession?.controller.terminalView.clearSearch()
        searchText = ""
        searchSummary = ""
    }
}
