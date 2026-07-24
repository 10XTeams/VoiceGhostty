import SwiftUI
import SwiftTerm

enum SplitAxis { case horizontal, vertical }
enum SplitFocus { case left, right, up, down }

/// 会话活动状态(极简三态)。灰=常态、黄=忙碌、绿=完成待输入。
enum SessionStatus {
    case normal, busy, done

    var priority: Int { self == .done ? 2 : (self == .busy ? 1 : 0) }

    /// 状态点颜色;常态为不显眼的灰(SwiftUI.Color,避免与 SwiftTerm.Color 歧义)
    var dotColor: SwiftUI.Color {
        switch self {
        case .normal: return .secondary.opacity(0.35)
        case .busy:   return .yellow
        case .done:   return .green   // 完成=绿灯(就绪/该你了,红绿灯语义)
        }
    }
}

/// 一个 zsh 会话(一个分屏格)。
final class TerminalSession: ObservableObject, Identifiable {
    let id = UUID()
    let controller: TerminalController
    @Published var title = "zsh"
    @Published var currentDirectory: String?
    /// 进程退出后置 true,UI 可据此显示灰态(暂不自动关闭,避免误伤)
    @Published var isTerminated = false
    /// 忙碌/完成状态(仅主线程改;由 SessionStore 的轮询推进)
    @Published var status: SessionStatus = .normal

    /// 是否当前聚焦的会话:聚焦时不显示忙碌/完成(你正看着,无需提示)
    private(set) var isFocused = false
    // 以下三个由后台读线程写、主线程轮询读的普通变量(值幂等,竞争无害)
    private var lastOutputAt = Date.distantPast
    private var hasActivity = false
    private var bellPending = false

    init(config: Config) {
        controller = TerminalController(config: config)
        controller.onTitleChange = { [weak self] t in
            DispatchQueue.main.async { self?.title = t.isEmpty ? "zsh" : t }
        }
        controller.onDirectoryChange = { [weak self] d in
            DispatchQueue.main.async { self?.currentDirectory = d }
        }
        controller.onTerminated = { [weak self] in
            DispatchQueue.main.async { self?.isTerminated = true }
        }
    }

    // MARK: - 状态信号(后台线程,只碰普通变量)

    func noteOutput() { lastOutputAt = Date(); hasActivity = true }
    func noteBell()   { bellPending = true }

    /// 轮询更新当前目录(查 shell 进程 cwd);变化时返回 true 供上层刷新标签栏。
    @discardableResult
    func refreshDirectory() -> Bool {
        let dir = controller.currentShellDirectory
        guard dir != currentDirectory else { return false }
        currentDirectory = dir
        return true
    }

    // MARK: - 状态推进(主线程)

    func setFocused(_ focused: Bool) {
        isFocused = focused
        if focused {                 // 聚焦即清零,回常态
            status = .normal
            hasActivity = false
            bellPending = false
        }
    }

    /// 轮询推进:铃声→完成;有活动且静默≥阈值→完成;否则忙碌。聚焦会话不动。
    /// 返回状态是否发生变化(供上层决定是否刷新标签栏)。
    @discardableResult
    func refreshStatus(silence: TimeInterval) -> Bool {
        guard !isFocused else { return false }
        let old = status
        if bellPending {
            bellPending = false
            hasActivity = false
            status = .done
        } else if hasActivity {
            if Date().timeIntervalSince(lastOutputAt) >= silence {
                status = .done
                hasActivity = false      // 完成后不再重复判定,直到下次输出
            } else {
                status = .busy
            }
        }
        return status != old
    }
}

/// 一个标签:含 1~2 个分屏格。
final class TerminalTab: ObservableObject, Identifiable {
    let id = UUID()
    @Published var panes: [TerminalSession]
    @Published var activePaneID: UUID
    @Published var splitAxis: SplitAxis = .horizontal
    /// 用户手动改的标签名;为空则回退到进程标题
    @Published var customTitle: String?

    init(pane: TerminalSession) {
        panes = [pane]
        activePaneID = pane.id
    }

    var activePane: TerminalSession {
        panes.first { $0.id == activePaneID } ?? panes[0]
    }
    var isSplit: Bool { panes.count > 1 }

    /// 标签栏显示的名字:手动改名优先;否则用各分屏格的文件夹名(左到右用 - 连接);
    /// 再退到进程标题。
    var displayTitle: String {
        if let custom = customTitle, !custom.isEmpty { return custom }
        let folders = panes.compactMap { Self.folderName($0.currentDirectory) }
        if !folders.isEmpty { return folders.joined(separator: "-") }
        let title = activePane.title
        return title.isEmpty ? "zsh" : title
    }

    /// 路径 → 文件夹名;home 显示为 ~
    private static func folderName(_ path: String?) -> String? {
        guard let path, !path.isEmpty else { return nil }
        if path == NSHomeDirectory() { return "~" }
        let name = (path as NSString).lastPathComponent
        return name.isEmpty ? "/" : name
    }

    /// 标签圆点取各分屏格中最紧要的状态(完成 > 忙碌 > 常态)
    var aggregateStatus: SessionStatus {
        panes.map(\.status).max { $0.priority < $1.priority } ?? .normal
    }
}

/// 全局会话状态:标签集合、活动标签/分屏、字号、主题、搜索栏可见性。
/// 语音「执行」永远路由到 activeSession。
final class SessionStore: ObservableObject {
    @Published var tabs: [TerminalTab] = []
    @Published var activeTabID: UUID?
    @Published private(set) var config: Config
    @Published var fontSize: CGFloat
    @Published var searchVisible = false

    /// 配置文件里的默认字号,⌘0 归位用
    private let defaultFontSize: CGFloat

    /// 状态轮询:每 0.5s 推进各会话的忙碌/完成;输出静默阈值 2s
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
        for tab in tabs {
            for pane in tab.panes {
                if pane.refreshStatus(silence: silenceThreshold) { changed = true }
                if pane.refreshDirectory() { changed = true }   // 更新 cwd → 标签名跟着变
            }
        }
        // 会话是嵌套 ObservableObject,状态/目录变了要主动通知标签栏(观察的是 store)重绘
        if changed { objectWillChange.send() }
    }

    /// 聚焦会话唯一化:只有当前活动标签的活动分屏格为 focused,其余都不是。
    private func refreshFocusFlags() {
        let activeID = activeSession?.id
        for tab in tabs { for pane in tab.panes { pane.setFocused(pane.id == activeID) } }
        objectWillChange.send()   // 聚焦会话被清为常态,标签点需刷新
    }

    // MARK: - 派生

    var activeTab: TerminalTab? {
        tabs.first { $0.id == activeTabID } ?? tabs.first
    }
    /// 语音/执行/搜索的目标会话
    var activeSession: TerminalSession? { activeTab?.activePane }

    // MARK: - 会话工厂(统一注入当前字号并接管焦点)

    private func makeSession() -> TerminalSession {
        var cfg = config
        cfg.fontSize = fontSize
        let session = TerminalSession(config: cfg)
        session.controller.onFocus = { [weak self, weak session] in
            guard let self, let session else { return }
            self.focus(session)
        }
        // 输出/铃声回调在后台线程触发,只改会话的普通变量(线程安全)
        session.controller.onOutput = { [weak session] in session?.noteOutput() }
        session.controller.onBell   = { [weak session] in session?.noteBell() }
        return session
    }

    // MARK: - 标签操作

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
            addTab()                        // 永远保留至少一个标签
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

    /// ⌘9:跳到最后一个标签(对齐 Ghostty)
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

    // MARK: - 分屏操作(每个标签最多 2 格)

    func splitActiveTab(axis: SplitAxis = .horizontal) {
        guard let tab = activeTab, !tab.isSplit else { return }
        let pane = makeSession()
        tab.splitAxis = axis
        tab.panes.append(pane)
        tab.activePaneID = pane.id
        focusActive()
    }

    /// ⌘W:有分屏先关活动格,否则关整个标签
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

    /// 按方向把焦点切到相邻分屏格(⌘⌥←/→/↑/↓,对齐 Ghostty 的 goto_split)。
    /// 每标签最多 2 格:横向分屏认左右,纵向分屏认上下,其余方向忽略。
    func focusSplit(_ direction: SplitFocus) {
        guard let tab = activeTab, tab.isSplit else { return }
        let wantSecond: Bool
        switch (tab.splitAxis, direction) {
        case (.horizontal, .right), (.vertical, .down): wantSecond = true
        case (.horizontal, .left),  (.vertical, .up):   wantSecond = false
        default: return   // 与分屏轴垂直的方向不动
        }
        let target = wantSecond ? tab.panes[1] : tab.panes[0]
        tab.activePaneID = target.id
        focusSession(target)
    }

    // MARK: - 焦点

    /// 某个终端视图成为第一响应者时回调:同步活动标签 + 活动分屏
    private func focus(_ session: TerminalSession) {
        for tab in tabs where tab.panes.contains(where: { $0.id == session.id }) {
            activeTabID = tab.id
            tab.activePaneID = session.id
            refreshFocusFlags()
            return
        }
    }

    /// 把键盘焦点交给当前活动会话的终端视图
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

    // MARK: - 字号 / 主题(应用到所有会话)

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

    /// ⌘K:清屏(对齐 Ghostty 的 clear_screen)
    func clearActiveScreen() {
        activeSession?.controller.clearScreen()
    }

    private func forEachSession(_ body: (TerminalSession) -> Void) {
        for tab in tabs { for pane in tab.panes { body(pane) } }
    }
}
