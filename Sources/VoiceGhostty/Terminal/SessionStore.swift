import SwiftUI
import SwiftTerm

enum SplitAxis { case horizontal, vertical }
enum SplitFocus { case left, right, up, down }

/// Session activity status (minimal three-state). Gray = normal, yellow = busy, green = done and awaiting input.
enum SessionStatus {
    case normal, busy, done

    var priority: Int { self == .done ? 2 : (self == .busy ? 1 : 0) }

    /// Status-dot color; normal is an unobtrusive gray (SwiftUI.Color, to avoid ambiguity with SwiftTerm.Color)
    var dotColor: SwiftUI.Color {
        switch self {
        case .normal: return .secondary.opacity(0.35)
        case .busy:   return .yellow
        case .done:   return .green   // Done = green light (ready / your turn, traffic-light semantics)
        }
    }
}

/// A single zsh session (one split pane).
final class TerminalSession: ObservableObject, Identifiable {
    let id = UUID()
    let controller: TerminalController
    @Published var title = "zsh"
    /// Seeded to the shell's known start directory (home) so the tab shows "~" immediately, instead of
    /// briefly falling back to the OSC process title (e.g. the computer name) before the first cwd poll.
    @Published var currentDirectory: String? = NSHomeDirectory()
    /// Set to true after the process exits; the UI can show a gray state (not auto-closed for now, to avoid accidents)
    @Published var isTerminated = false
    /// Busy/done status (only mutated on the main thread; driven by SessionStore's polling)
    @Published var status: SessionStatus = .normal

    // The following are plain variables written by the background read thread and polled by the main thread (values are idempotent, races are harmless)
    private var lastOutputAt = Date.distantPast
    private var hasActivity = false
    private var bellPending = false
    // OSC 133 shell integration (precise mode): exact command boundaries, preferred over the output heuristic when present.
    private var cmdRunning = false          // between OSC 133 C (start) and D (end)
    private var cmdEndedPending = false     // a command just finished → surface "done" once
    private var osc133Seen = false          // latched once any 133 marker arrives → switch to precise mode

    init(config: Config) {
        controller = TerminalController(config: config)
        controller.onTitleChange = { [weak self] t in
            DispatchQueue.main.async { self?.title = t.isEmpty ? "zsh" : t }
        }
        controller.onDirectoryChange = { [weak self] d in
            guard let d else { return }   // keep the last-known directory rather than blanking to the process title
            DispatchQueue.main.async { self?.currentDirectory = d }
        }
        controller.onTerminated = { [weak self] in
            DispatchQueue.main.async { self?.isTerminated = true }
        }
    }

    // MARK: - Status signals (background thread, only touches plain variables)

    func noteOutput() { lastOutputAt = Date(); hasActivity = true }
    func noteBell()   { bellPending = true }
    func noteCommandStart() { cmdRunning = true; osc133Seen = true }
    func noteCommandEnd()   { osc133Seen = true; if cmdRunning { cmdRunning = false; cmdEndedPending = true } }

    /// Poll and update the current directory (queries the shell process cwd); returns true on change so the caller can refresh the tab bar.
    @discardableResult
    func refreshDirectory() -> Bool {
        // Ignore nil (shell process not queryable yet / momentarily): never clobber a known directory with nil,
        // otherwise the tab title would flicker back to the OSC process title.
        guard let dir = controller.currentShellDirectory, dir != currentDirectory else { return false }
        currentDirectory = dir
        return true
    }

    // MARK: - Status advancement (main thread)

    /// Reset to the neutral (gray) state — used for the single terminal you're actively using, so it doesn't nag itself with its own light.
    /// (`cmdRunning` is left untouched — it reflects real state; only the display-oriented flags are cleared.)
    @discardableResult
    func resetStatus() -> Bool {
        let changed = status != .normal || hasActivity || bellPending || cmdEndedPending
        status = .normal
        hasActivity = false
        bellPending = false
        cmdEndedPending = false
        return changed
    }

    /// Poll advancement. Returns whether the status changed (so the caller can decide whether to refresh the tab bar).
    /// - Precise mode (OSC 133 shell integration seen): C → busy (yellow), D → done (green), done persists until reset / next command.
    /// - Fallback heuristic (no shell integration): bell or "had output then silent ≥ threshold" → done; recent output → busy.
    @discardableResult
    func refreshStatus(silence: TimeInterval) -> Bool {
        let old = status
        if osc133Seen {
            if bellPending {
                bellPending = false
                cmdEndedPending = false
                status = .done
            } else if cmdRunning {
                status = .busy
            } else if cmdEndedPending {
                cmdEndedPending = false
                status = .done
            }
            // else: keep the current status (a finished command stays green until you attend it or start a new one)
        } else {
            if bellPending {
                bellPending = false
                hasActivity = false
                status = .done
            } else if hasActivity {
                if Date().timeIntervalSince(lastOutputAt) >= silence {
                    status = .done
                    hasActivity = false      // Once done, don't judge again until the next output
                } else {
                    status = .busy
                }
            }
        }
        return status != old
    }
}

/// A tab: contains 1-2 split panes.
final class TerminalTab: ObservableObject, Identifiable {
    let id = UUID()
    @Published var panes: [TerminalSession]
    @Published var activePaneID: UUID
    @Published var splitAxis: SplitAxis = .horizontal
    /// A tab name the user manually set; if empty, falls back to the process title
    @Published var customTitle: String?

    init(pane: TerminalSession) {
        panes = [pane]
        activePaneID = pane.id
    }

    var activePane: TerminalSession {
        panes.first { $0.id == activePaneID } ?? panes[0]
    }
    var isSplit: Bool { panes.count > 1 }

    /// The name shown in the tab bar: a manual rename takes priority; otherwise the folder name of each split
    /// pane (left to right, joined with -); if no directory is known yet, plain "zsh".
    /// Deliberately does NOT fall back to the OSC process title — some shells set it to the computer name
    /// (e.g. "my computer"), which we never want surfacing in a tab.
    var displayTitle: String {
        if let custom = customTitle, !custom.isEmpty { return custom }
        let folders = panes.compactMap { Self.folderName($0.currentDirectory) }
        if !folders.isEmpty { return folders.joined(separator: "-") }
        return "zsh"
    }

    /// Path → folder name; home is shown as ~
    private static func folderName(_ path: String?) -> String? {
        guard let path, !path.isEmpty else { return nil }
        if path == NSHomeDirectory() { return "~" }
        let name = (path as NSString).lastPathComponent
        return name.isEmpty ? "/" : name
    }

    /// The tab dot takes the most important status among the split panes (done > busy > normal)
    var aggregateStatus: SessionStatus {
        panes.map(\.status).max { $0.priority < $1.priority } ?? .normal
    }
}

/// Global session state: the tab collection, active tab/split, font size, theme, and search-bar visibility.
/// Voice "execute" always routes to activeSession.
final class SessionStore: ObservableObject {
    @Published var tabs: [TerminalTab] = []
    @Published var activeTabID: UUID?
    @Published private(set) var config: Config
    @Published var fontSize: CGFloat
    @Published var searchVisible = false

    /// The default font size from the config file, used by ⌘0 to reset
    private let defaultFontSize: CGFloat

    /// Status polling: every 0.5s advance each session's busy/done; output-silence threshold is 2s
    private var statusTimer: Timer?
    private let silenceThreshold: TimeInterval = 2.0

    init() {
        let cfg = Config.load()
        config = cfg
        fontSize = cfg.fontSize
        defaultFontSize = cfg.fontSize
        addTab()
        statusTimer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
            self?.tickStatus()
        }
    }

    private func tickStatus() {
        var changed = false
        let activeID = activeSession?.id
        for tab in tabs {
            for pane in tab.panes {
                // Keep the single terminal you're actively using neutral (no self-nagging light).
                // In a split, every pane advances its own status so both show yellow (running) / green (done).
                if !tab.isSplit && pane.id == activeID {
                    if pane.resetStatus() { changed = true }
                } else {
                    if pane.refreshStatus(silence: silenceThreshold) { changed = true }
                }
                if pane.refreshDirectory() { changed = true }   // Update cwd → the tab name changes accordingly
            }
        }
        // Sessions are nested ObservableObjects, so when status/directory changes we must actively notify the tab bar (which observes the store) to redraw
        if changed { objectWillChange.send() }
    }

    /// Notify observers after a focus change so the tab bar / active-pane highlight (and the active tab's neutral dot) refresh.
    private func refreshFocusFlags() {
        objectWillChange.send()
    }

    // MARK: - Derived

    var activeTab: TerminalTab? {
        tabs.first { $0.id == activeTabID } ?? tabs.first
    }
    /// Target session for voice / execute / search
    var activeSession: TerminalSession? { activeTab?.activePane }

    // MARK: - Session factory (uniformly injects the current font size and takes over focus)

    private func makeSession() -> TerminalSession {
        var cfg = config
        cfg.fontSize = fontSize
        let session = TerminalSession(config: cfg)
        session.controller.onFocus = { [weak self, weak session] in
            guard let self, let session else { return }
            self.focus(session)
        }
        // Output/bell/command callbacks fire on the background thread and only mutate the session's plain variables (thread-safe)
        session.controller.onOutput = { [weak session] in session?.noteOutput() }
        session.controller.onBell   = { [weak session] in session?.noteBell() }
        session.controller.onCommandStart = { [weak session] in session?.noteCommandStart() }
        session.controller.onCommandEnd   = { [weak session] in session?.noteCommandEnd() }
        return session
    }

    // MARK: - Tab operations

    func addTab() {
        let tab = TerminalTab(pane: makeSession())
        tabs.append(tab)
        activeTabID = tab.id
        focusActive()
    }

    func closeActiveTab() {
        guard let tab = activeTab else { return }
        close(tab: tab)
    }

    private func close(tab: TerminalTab) {
        guard let idx = tabs.firstIndex(where: { $0.id == tab.id }) else { return }
        tabs.remove(at: idx)
        if tabs.isEmpty {
            addTab()                        // Always keep at least one tab
        } else {
            let next = tabs[min(idx, tabs.count - 1)]
            activeTabID = next.id
            focusActive()
        }
    }

    func selectTab(index: Int) {
        guard tabs.indices.contains(index) else { return }
        activeTabID = tabs[index].id
        focusActive()
    }

    func selectNextTab() { shiftTab(+1) }
    func selectPreviousTab() { shiftTab(-1) }

    /// ⌘9: jump to the last tab (matching Ghostty)
    func selectLastTab() {
        guard let last = tabs.last else { return }
        activeTabID = last.id
        focusActive()
    }

    private func shiftTab(_ delta: Int) {
        guard !tabs.isEmpty, let cur = tabs.firstIndex(where: { $0.id == activeTabID }) else { return }
        let next = (cur + delta + tabs.count) % tabs.count
        activeTabID = tabs[next].id
        focusActive()
    }

    // MARK: - Split operations (at most 2 panes per tab)

    func splitActiveTab(axis: SplitAxis = .horizontal) {
        guard let tab = activeTab, !tab.isSplit else { return }
        let pane = makeSession()
        tab.splitAxis = axis
        tab.panes.append(pane)
        tab.activePaneID = pane.id
        focusActive()
    }

    /// ⌘W: if split, close the active pane first; otherwise close the whole tab
    func closeActivePaneOrTab() {
        guard let tab = activeTab else { return }
        if tab.isSplit {
            tab.panes.removeAll { $0.id == tab.activePaneID }
            tab.activePaneID = tab.panes[0].id
            focusActive()
        } else {
            close(tab: tab)
        }
    }

    /// Move focus to the adjacent split pane by direction (⌘⌥←/→/↑/↓, matching Ghostty's goto_split).
    /// At most 2 panes per tab: a horizontal split recognizes left/right, a vertical split recognizes up/down, other directions are ignored.
    func focusSplit(_ direction: SplitFocus) {
        guard let tab = activeTab, tab.isSplit else { return }
        let wantSecond: Bool
        switch (tab.splitAxis, direction) {
        case (.horizontal, .right), (.vertical, .down): wantSecond = true
        case (.horizontal, .left),  (.vertical, .up):   wantSecond = false
        default: return   // A direction perpendicular to the split axis does nothing
        }
        let target = wantSecond ? tab.panes[1] : tab.panes[0]
        tab.activePaneID = target.id
        focusSession(target)
    }

    // MARK: - Focus

    /// Callback when a terminal view becomes first responder: sync the active tab + active pane
    private func focus(_ session: TerminalSession) {
        for tab in tabs where tab.panes.contains(where: { $0.id == session.id }) {
            activeTabID = tab.id
            tab.activePaneID = session.id
            refreshFocusFlags()
            return
        }
    }

    /// Give keyboard focus to the terminal view of the current active session
    func focusActive() {
        guard let session = activeSession else { return }
        focusSession(session)
    }

    private func focusSession(_ session: TerminalSession) {
        refreshFocusFlags()
        DispatchQueue.main.async {
            let view = session.controller.terminalView
            view.window?.makeFirstResponder(view)
        }
    }

    // MARK: - Font size / theme (applied to all sessions)

    private let minFontSize: CGFloat = 8
    private let maxFontSize: CGFloat = 48

    func increaseFontSize() { setFontSize(fontSize + 1) }
    func decreaseFontSize() { setFontSize(fontSize - 1) }
    func resetFontSize()    { setFontSize(defaultFontSize) }

    private func setFontSize(_ size: CGFloat) {
        let clamped = min(max(size, minFontSize), maxFontSize)
        fontSize = clamped
        forEachSession { $0.controller.setFontSize(clamped) }
    }

    func setTheme(_ themeName: String) {
        config.themeName = themeName
        forEachSession { $0.controller.setTheme(themeName) }
    }

    /// ⌘K: clear the screen (matching Ghostty's clear_screen)
    func clearActiveScreen() {
        activeSession?.controller.clearScreen()
    }

    private func forEachSession(_ body: (TerminalSession) -> Void) {
        for tab in tabs { for pane in tab.panes { body(pane) } }
    }
}
