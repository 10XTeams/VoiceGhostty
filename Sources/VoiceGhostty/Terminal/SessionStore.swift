import AppKit
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
    /// Set by the last status advance when this pane *became* done, so the store can chime on the transition
    /// rather than for every tick the light stays green.
    private(set) var justTurnedDone = false

    // The following are plain variables written by the background read thread and polled by the main thread (values are idempotent, races are harmless)
    private var lastOutputAt = Date.distantPast
    private var hasActivity = false
    private var bellPending = false
    // OSC 133 shell integration (precise mode): exact command boundaries, preferred over the output heuristic when present.
    private var cmdRunning = false          // between OSC 133 C (start) and D (end)
    private var cmdEndedPending = false     // a command just finished → surface "done" once
    private var osc133Seen = false          // latched once any 133 marker arrives → switch to precise mode

    /// Transcript file for this pane; nil when history saving is off (decided once, at creation).
    private let logger: SessionLogger?

    init(config: Config) {
        controller = TerminalController(config: config)
        logger = AppSettings.historyEnabled ? SessionLogger() : nil
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
    // A fresh command starts with a clean activity slate, so a TUI's light is judged on its own output
    // rather than on whatever the previous command left behind.
    func noteCommandStart() { cmdRunning = true; osc133Seen = true; hasActivity = false }
    func noteCommandEnd()   { osc133Seen = true; if cmdRunning { cmdRunning = false; cmdEndedPending = true } }

    /// Poll and update the current directory (queries the shell process cwd); returns true on change so the caller can refresh the tab bar.
    @discardableResult
    func refreshDirectory() -> Bool {
        // Ignore nil (shell process not queryable yet / momentarily): never clobber a known directory with nil,
        // otherwise the tab title would flicker back to the OSC process title.
        guard let dir = controller.currentShellDirectory, dir != currentDirectory else { return false }
        currentDirectory = dir
        logger?.currentDirectory = dir      // names the transcript file, if it hasn't been created yet
        return true
    }

    // MARK: - Transcript

    /// Append whatever has scrolled off screen to this pane's history file.
    func flushHistory() { logger?.flush(controller.terminal) }

    /// Last write before the pane goes away: takes the on-screen lines too, then closes the file.
    func finishHistory() { logger?.finish(controller.terminal) }

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
        justTurnedDone = false
        return changed
    }

    /// Poll advancement. Returns whether the status changed (so the caller can decide whether to refresh the tab bar).
    /// - Precise mode (OSC 133 shell integration seen): C → busy (yellow), D → done (green), done persists until reset / next command.
    ///   Exception: an interactive TUI (see `isInteractiveApp`) is one long "command" to the shell, so its
    ///   light comes from the output heuristic instead — otherwise it would be pinned yellow the whole session.
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
                if isInteractiveApp {
                    advanceByOutput(silence: silence)   // streaming = working, gone quiet = your turn
                } else {
                    status = .busy
                }
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
            } else {
                advanceByOutput(silence: silence)
            }
        }
        justTurnedDone = old != .done && status == .done
        return status != old
    }

    /// Whether the running command is an interactive full-screen app (claude, vim, less) rather than a plain
    /// command: those keep the tty in raw mode for their whole session, so "a command is running" says nothing
    /// about whether work is happening. Unknown (nil) counts as a plain command — keep trusting OSC 133.
    private var isInteractiveApp: Bool { controller.isRawMode == true }

    /// Output-activity heuristic: recent output → busy; had output and then went quiet for `silence` → done.
    /// No output seen at all leaves the status alone (nothing to report yet).
    private func advanceByOutput(silence: TimeInterval) {
        guard hasActivity else { return }
        if Date().timeIntervalSince(lastOutputAt) >= silence {
            status = .done
            hasActivity = false      // Once done, don't judge again until the next output
        } else {
            status = .busy
        }
    }
}

/// The split layout of a tab: either a single terminal (leaf) or a row/column of children (branch).
/// Branches nest, so a tab can hold any number of panes in any arrangement — a column of terminals
/// beside a single one, a 2×2 grid, and so on.
///
/// The tree is *data only* — no view mirrors its shape. `TabContentView` renders a flat list of panes
/// keyed by session id and asks `paneFrames(in:spacing:)` where to put each one, so re-arranging the
/// tree moves panes around without any view being torn down and rebuilt. `SplitNode.id` therefore no
/// longer participates in view identity; it exists purely so `inserting`/`removing` can find a node.
indirect enum SplitNode: Identifiable {
    case leaf(TerminalSession)
    case branch(id: UUID, axis: SplitAxis, children: [SplitNode])

    var id: UUID {
        switch self {
        case .leaf(let session):      return session.id
        case .branch(let id, _, _):   return id
        }
    }

    /// Every terminal in this subtree, in visual order (left→right / top→bottom).
    var leaves: [TerminalSession] {
        switch self {
        case .leaf(let session):            return [session]
        case .branch(_, _, let children):   return children.flatMap(\.leaves)
        }
    }

    /// Where each pane goes inside `rect`, with `spacing` points of gap between siblings (the gap shows
    /// the backdrop through, which is what draws the separator lines).
    ///
    /// ★ Contract: the result is in the same order as `leaves` — both walk children front-to-back — so
    /// `SplitLayout` can match frame *i* to subview *i*. Change one traversal and you must change both.
    func paneFrames(in rect: CGRect, spacing: CGFloat) -> [CGRect] {
        switch self {
        case .leaf:
            return [rect]

        case .branch(_, let axis, let children):
            guard !children.isEmpty else { return [] }
            let count = CGFloat(children.count)
            let gaps = spacing * (count - 1)
            return children.enumerated().flatMap { index, child -> [CGRect] in
                let offset = CGFloat(index)
                let slot: CGRect
                if axis == .horizontal {
                    let width = max(0, (rect.width - gaps) / count)
                    slot = CGRect(x: rect.minX + offset * (width + spacing), y: rect.minY,
                                  width: width, height: rect.height)
                } else {
                    let height = max(0, (rect.height - gaps) / count)
                    slot = CGRect(x: rect.minX, y: rect.minY + offset * (height + spacing),
                                  width: rect.width, height: height)
                }
                return child.paneFrames(in: slot, spacing: spacing)
            }
        }
    }

    /// Insert `newPane` next to the pane `targetID`, splitting along `axis`.
    /// Splitting along the axis the pane already sits on adds a *sibling*, so three ⌘D in a row give
    /// three evenly-sized columns instead of a lopsided ½ + ¼ + ¼ nest. A perpendicular split replaces
    /// the leaf with a two-child branch, which is what lets grids form.
    func inserting(_ newPane: TerminalSession, after targetID: UUID, axis: SplitAxis) -> SplitNode {
        switch self {
        case .leaf(let session):
            guard session.id == targetID else { return self }
            return .branch(id: UUID(), axis: axis, children: [.leaf(session), .leaf(newPane)])

        case .branch(let id, let branchAxis, let children):
            if branchAxis == axis, let idx = children.firstIndex(where: { $0.id == targetID }),
               case .leaf = children[idx] {
                var siblings = children
                siblings.insert(.leaf(newPane), at: idx + 1)
                return .branch(id: id, axis: branchAxis, children: siblings)
            }
            return .branch(id: id, axis: branchAxis,
                           children: children.map { $0.inserting(newPane, after: targetID, axis: axis) })
        }
    }

    /// Remove a terminal. A branch left holding a single child collapses into that child, so closing
    /// panes never leaves empty scaffolding behind. Returns nil once the subtree is empty.
    func removing(_ targetID: UUID) -> SplitNode? {
        switch self {
        case .leaf(let session):
            return session.id == targetID ? nil : self
        case .branch(let id, let axis, let children):
            let kept = children.compactMap { $0.removing(targetID) }
            if kept.isEmpty { return nil }
            if kept.count == 1 { return kept[0] }
            return .branch(id: id, axis: axis, children: kept)
        }
    }

    /// The pane adjacent to `targetID` in `direction`: walk up to the nearest ancestor split along that
    /// direction's axis, step to the sibling on that side, then descend to its facing edge.
    func neighbor(of targetID: UUID, direction: SplitFocus) -> TerminalSession? {
        guard let path = chain(to: targetID) else { return nil }
        let wantAxis: SplitAxis = (direction == .left || direction == .right) ? .horizontal : .vertical
        let delta = (direction == .right || direction == .down) ? 1 : -1
        for step in path.reversed() {
            guard case .branch(_, let axis, let children) = step.branch, axis == wantAxis else { continue }
            let idx = step.index + delta
            guard children.indices.contains(idx) else { continue }   // edge of this branch → keep walking up
            return children[idx].edgeLeaf(facing: direction)
        }
        return nil
    }

    /// The branch chain from here down to `targetID`, recording which child was taken at each level.
    private func chain(to targetID: UUID) -> [(branch: SplitNode, index: Int)]? {
        switch self {
        case .leaf(let session):
            return session.id == targetID ? [] : nil
        case .branch(_, _, let children):
            for (idx, child) in children.enumerated() {
                if let rest = child.chain(to: targetID) { return [(self, idx)] + rest }
            }
            return nil
        }
    }

    /// Descending into a neighbouring subtree, land on the pane closest to the edge you came from —
    /// moving left enters from the right-hand side, and so on.
    private func edgeLeaf(facing direction: SplitFocus) -> TerminalSession? {
        switch self {
        case .leaf(let session):
            return session
        case .branch(_, let axis, let children):
            let alongAxis = axis == .horizontal
                ? (direction == .left || direction == .right)
                : (direction == .up || direction == .down)
            guard alongAxis else { return children.first?.edgeLeaf(facing: direction) }
            let nearest = (direction == .left || direction == .up) ? children.last : children.first
            return nearest?.edgeLeaf(facing: direction)
        }
    }
}

/// A tab: holds a split tree of one or more panes.
final class TerminalTab: ObservableObject, Identifiable {
    let id = UUID()
    @Published var root: SplitNode
    @Published var activePaneID: UUID
    /// A tab name the user manually set; if empty, falls back to the process title
    @Published var customTitle: String?

    init(pane: TerminalSession) {
        root = .leaf(pane)
        activePaneID = pane.id
    }

    /// All panes in visual order (derived from the tree — the tree is the single source of truth)
    var panes: [TerminalSession] { root.leaves }

    var activePane: TerminalSession {
        panes.first { $0.id == activePaneID } ?? panes[0]
    }
    var isSplit: Bool { panes.count > 1 }

    /// Split the active pane along `axis` and focus the new pane.
    func split(_ newPane: TerminalSession, axis: SplitAxis) {
        root = root.inserting(newPane, after: activePaneID, axis: axis)
        activePaneID = newPane.id
    }

    /// Close the active pane, moving focus to its neighbour.
    /// Returns the removed pane so the caller can shut its shell down; nil when it was the tab's only
    /// pane, in which case the caller closes the whole tab instead.
    func closeActivePane() -> TerminalSession? {
        let list = panes
        guard list.count > 1, let idx = list.firstIndex(where: { $0.id == activePaneID }) else { return nil }
        let closing = list[idx]
        guard let pruned = root.removing(closing.id) else { return nil }
        root = pruned
        activePaneID = list[idx > 0 ? idx - 1 : 1].id
        return closing
    }

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

    /// Transcripts are appended every `historyFlushEvery` status ticks (~5s) rather than on every tick —
    /// nothing is lost by batching, since the lines wait in the scrollback buffer either way.
    private var tickCount = 0
    private let historyFlushEvery = 10

    init() {
        let cfg = Config.load()
        config = cfg
        fontSize = cfg.fontSize
        defaultFontSize = cfg.fontSize
        addTab()
        statusTimer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
            self?.tickStatus()
        }
        // Quitting closes the pty fds anyway, but hang the shells up explicitly so a long-running job
        // can't briefly outlive the app. Transcripts are closed out first, while the buffers still exist.
        NotificationCenter.default.addObserver(forName: NSApplication.willTerminateNotification,
                                               object: nil, queue: .main) { [weak self] _ in
            self?.forEachSession {
                $0.finishHistory()
                $0.controller.terminate()
            }
        }
    }

    private func tickStatus() {
        let activeID = activeSession?.id
        tickCount += 1
        let flushHistory = tickCount % historyFlushEvery == 0
        for tab in tabs {
            var changed = false
            for pane in tab.panes {
                // Keep the single terminal you're actively using neutral (no self-nagging light) — but only
                // while you are actually here. With the app in the background, that pane is precisely the one
                // whose finish is worth reporting, so let it advance and chime; returning to the front resets
                // it to gray on the next tick, leaving the foreground behavior as it was.
                // In a split, every pane advances its own status so both show yellow (running) / green (done).
                if !tab.isSplit && pane.id == activeID && NSApp.isActive {
                    if pane.resetStatus() { changed = true }
                } else {
                    if pane.refreshStatus(silence: silenceThreshold) { changed = true }
                    if pane.justTurnedDone { DoneChime.play() }
                }
                if pane.refreshDirectory() { changed = true }   // Update cwd → the tab name changes accordingly
                if flushHistory { pane.flushHistory() }
            }
            // Panes are nested ObservableObjects, so a status/cwd change has to be announced by hand for
            // the tab's dot and title to redraw. Announce it on the *tab*, never on the store: the store
            // is an EnvironmentObject of ContentView, so waking it re-runs the entire window body —
            // terminal hosts included — twice a second, purely to repaint a 9pt circle.
            if changed { tab.objectWillChange.send() }
        }
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
        // Every shell in the tab, not just the active pane
        for pane in tab.panes {
            pane.finishHistory()
            pane.controller.terminate()
        }
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

    // MARK: - Split operations (any number of panes, nested to any depth)

    /// ⌘D / ⌘⇧D: split the *active pane* along `axis`. Repeatable — splitting again along the same axis
    /// adds another evenly-sized pane, along the other axis subdivides just that pane.
    func splitActiveTab(axis: SplitAxis = .horizontal) {
        guard let tab = activeTab else { return }
        tab.split(makeSession(), axis: axis)
        focusActive()
    }

    /// ⌘W: if split, close the active pane first; otherwise close the whole tab
    func closeActivePaneOrTab() {
        guard let tab = activeTab else { return }
        if let closed = tab.closeActivePane() {
            closed.finishHistory()
            closed.controller.terminate()
            focusActive()
        } else {
            close(tab: tab)
        }
    }

    /// Move focus to the adjacent pane by direction (⌘⌥←/→/↑/↓, matching Ghostty's goto_split).
    /// Resolved against the split tree, so it works across nested splits; a direction with no pane
    /// on that side leaves focus where it is.
    func focusSplit(_ direction: SplitFocus) {
        guard let tab = activeTab, tab.isSplit,
              let target = tab.root.neighbor(of: tab.activePaneID, direction: direction) else { return }
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

    /// Apply a new scrollback size from Settings to every open pane (new panes read the setting themselves).
    func setScrollback(_ lines: Int) {
        forEachSession { $0.controller.setScrollback(lines) }
    }

    /// ⌘K: clear the screen (matching Ghostty's clear_screen)
    func clearActiveScreen() {
        activeSession?.controller.clearScreen()
    }

    private func forEachSession(_ body: (TerminalSession) -> Void) {
        for tab in tabs { for pane in tab.panes { body(pane) } }
    }
}
