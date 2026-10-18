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
            // Always in the layout, empty or not. This line comes and goes on its own during voice use
            // ("converting…", an explanation, "didn't catch that"), and a row that appears and disappears
            // resizes the terminal under whatever is running in it — a SIGWINCH landing mid-frame is what
            // leaves a full-screen TUI's input line drawn a row off until it next repaints itself.
            Divider()
            statusBar
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
            .help(loc("Loader: the folders added in Settings are scanned for plugins, skills and agents, and whatever you check is loaded into the current directory",
                      "加载器:自动识别设置里添加的文件夹中的插件、技能、Agent,勾选后加载到当前目录"))

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
        .modifier(ArrowCursor())
    }

    private var settingsOverlay: some View {
        floatingPanel(dismiss: { showSettings = false }) {
            SettingsView(onDone: { showSettings = false })
        }
    }

    private var skillOverlay: some View {
        floatingPanel(dismiss: { showSkillPanel = false }) {
            LoaderPanelView(currentDirectory: store.activeSession?.currentDirectory,
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
                ForEach(sortedClaudeDirs, id: \.self) { dir in
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

    /// The menu lists directories A→Z by their displayed name (the last path component), so the order
    /// stays predictable no matter when each one was added. The stored array keeps its insertion order.
    private var sortedClaudeDirs: [String] {
        claudeDirs.sorted {
            let a = dirDisplayName($0), b = dirDisplayName($1)
            let byName = a.localizedStandardCompare(b)
            return byName == .orderedSame ? $0.localizedStandardCompare($1) == .orderedAscending
                                          : byName == .orderedAscending
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

    // MARK: - Status bar (errors / natural-language explanations; a reserved strip when there is nothing to say)

    private var statusBar: some View {
        // One line, always exactly one line: a message long enough to wrap would resize the terminal just
        // as surely as the bar appearing does, so it truncates and keeps the full text in the tooltip.
        HStack(spacing: 10) {
            Group {
                if let err = speech.errorMessage {
                    Text(err).foregroundStyle(.red).help(err)
                } else if !statusText.isEmpty {
                    Text(statusText).foregroundStyle(.secondary).help(statusText)
                } else {
                    Text(" ")   // Holds the row's height open so the terminal never changes size
                }
            }
            .lineLimit(1)
            .truncationMode(.tail)
            .frame(maxWidth: .infinity, alignment: .leading)

            // Context meter for the active pane, on the trailing edge. Its own view observing its own
            // watcher, so a moved token count repaints this strip and nothing else.
            if let session = store.activeSession {
                ClaudeContextMeter(watcher: session.claudeUsage)
            }
        }
        .font(.caption)
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

// MARK: - Panel cursor

/// Keeps the arrow cursor over an in-window panel, so its buttons are not hovered with a text I-beam.
///
/// The panels are SwiftUI overlays drawn above the terminal, but the terminal is a real `NSView` and
/// AppKit resolves cursor rects per view — a SwiftUI overlay is not one. SwiftTerm claims the terminal's
/// whole bounds for `.iBeam` (`MacTerminalView.resetCursorRects`), so that claim covers the panel too.
///
/// Overriding it is not on the table: SwiftTerm declares `resetCursorRects` and `cursorUpdate` as
/// `public override`, not `open`, so neither can be overridden from this module. Pushing the cursor while
/// the pointer is inside the panel is the way in, and it holds: AppKit only re-applies a rect's cursor
/// when the pointer crosses that rect's boundary, which never happens while moving around inside the
/// panel — the panel sits well within the terminal's single full-bounds rect.
private struct ArrowCursor: ViewModifier {
    /// Push and pop have to pair up exactly, so the pushed state is tracked rather than inferred: an
    /// unmatched pop unwinds someone else's cursor, and an unmatched push leaves the arrow stuck on.
    @State private var pushed = false

    func body(content: Content) -> some View {
        content
            .onHover { inside in inside ? push() : pop() }
            // A panel can be dismissed while the pointer is still inside it, and that is a disappearance,
            // not a hover-out — without this the push would never be matched.
            .onDisappear(perform: pop)
    }

    private func push() {
        guard !pushed else { return }
        pushed = true
        NSCursor.arrow.push()
    }

    private func pop() {
        guard pushed else { return }
        pushed = false
        NSCursor.pop()
    }
}


/// Context-window usage of the Claude Code session in the active pane: a bar, the numbers, and nothing
/// at all when claude isn't running there.
///
/// The reading comes from claude's own transcript and therefore lags the live session by one request —
/// it is the size the *last* request carried, which is what the next one starts from.
private struct ClaudeContextMeter: View {
    @ObservedObject var watcher: ClaudeUsageWatcher
    @ObservedObject private var loc = Loc.shared

    /// Warn before the window is a problem, not once it already is: compaction lands around 90%, so
    /// orange is the "plan the next prompt" band and red the "it is about to compact" one.
    private func tint(_ fraction: Double) -> SwiftUI.Color {
        if fraction >= 0.9 { return .red }
        if fraction >= 0.7 { return .orange }
        return .secondary
    }

    var body: some View {
        if let usage = watcher.usage {
            let color = tint(usage.fraction)
            HStack(spacing: 6) {
                Text("claude").foregroundStyle(.tertiary)
                Capsule()
                    .fill(.quaternary)
                    .frame(width: 42, height: 4)
                    .overlay(alignment: .leading) {
                        Capsule()
                            .fill(color)
                            .frame(width: max(2, 42 * usage.fraction), height: 4)
                    }
                Text("\(usage.short) · \(usage.percent)%")
                    .foregroundStyle(color)
                    .monospacedDigit()
            }
            .lineLimit(1)
            .fixedSize()
            .help(tooltip(usage))
        }
    }

    private func tooltip(_ usage: ClaudeContextUsage) -> String {
        var lines = [
            loc("Context: \(usage.tokens) / \(usage.limit) tokens (\(usage.percent)%)",
                "上下文:\(usage.tokens) / \(usage.limit) tokens(\(usage.percent)%)"),
            loc("As of claude's last request — a request in flight isn't counted yet.",
                "统计到 claude 上一次请求 —— 正在进行的那次还没计入。"),
        ]
        if let model = usage.model { lines.append(loc("Model: ", "模型:") + model) }
        if let session = usage.sessionID { lines.append(loc("Session: ", "会话:") + session) }
        lines.append(usage.transcript)
        return lines.joined(separator: "\n")
    }
}
