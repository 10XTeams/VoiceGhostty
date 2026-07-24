import AppKit
import Darwin
import SwiftTerm

/// 查某个进程的当前工作目录(不依赖 shell 集成 / OSC 7)。
/// 对 shell 进程用它:即使前台跑着 claude 等 TUI,shell 的 cwd 仍是「进入前所在的文件夹」。
func processCurrentDirectory(_ pid: pid_t) -> String? {
    guard pid > 0 else { return nil }
    var info = proc_vnodepathinfo()
    let size = Int32(MemoryLayout<proc_vnodepathinfo>.size)
    guard proc_pidinfo(pid, PROC_PIDVNODEPATHINFO, 0, &info, size) == size else { return nil }
    return withUnsafeBytes(of: &info.pvi_cdir.vip_path) { buf in
        buf.baseAddress.map { String(cString: $0.assumingMemoryBound(to: CChar.self)) }
    }
}

/// LocalProcessTerminalView 的子类:
/// - 点击回调(onFocus)让 SessionStore 同步「活动分屏」
/// - PTY 输出(onOutput)/ 终端铃声(onBell)回调用于「忙碌/完成」状态判定
///   (becomeFirstResponder 是 public 不能重写,故用 mouseDown 判定焦点;
///    dataReceived / bell 均为 open,可安全拦截。)
/// 注意:onOutput / onBell 在后台读线程触发,回调里只可改普通变量,勿碰 @Published。
final class FocusReportingTerminalView: LocalProcessTerminalView {
    var onFocus: (() -> Void)?
    var onOutput: (() -> Void)?
    var onBell: (() -> Void)?

    override func mouseDown(with event: NSEvent) {
        onFocus?()
        super.mouseDown(with: event)
    }

    override func dataReceived(slice: ArraySlice<UInt8>) {
        super.dataReceived(slice: slice)
        onOutput?()
    }

    override func bell(source: Terminal) {
        super.bell(source: source)
        onBell?()
    }
}

/// 持有一个 SwiftTerm 终端视图,负责起 PTY/zsh、应用配置、写入命令。
/// 一个 TerminalController == 一个 zsh 会话(一个标签或一个分屏格)。
final class TerminalController: NSObject, ObservableObject, LocalProcessTerminalViewDelegate {
    let terminalView: FocusReportingTerminalView

    /// 当前字号(SessionStore 统一缩放时读写)
    private(set) var config: Config

    /// shell 进程的当前工作目录(轮询用,不依赖 OSC 7)
    var currentShellDirectory: String? {
        guard let process = terminalView.process else { return nil }
        return processCurrentDirectory(process.shellPid)
    }

    /// 焦点/标题/工作目录/进程退出回调,由 SessionStore 接管
    var onFocus: (() -> Void)? {
        get { terminalView.onFocus }
        set { terminalView.onFocus = newValue }
    }
    /// PTY 有输出(后台线程);/ 终端响铃(后台线程)
    var onOutput: (() -> Void)? {
        get { terminalView.onOutput }
        set { terminalView.onOutput = newValue }
    }
    var onBell: (() -> Void)? {
        get { terminalView.onBell }
        set { terminalView.onBell = newValue }
    }
    var onTitleChange: ((String) -> Void)?
    var onDirectoryChange: ((String?) -> Void)?
    var onTerminated: (() -> Void)?

    init(config: Config) {
        self.config = config
        terminalView = FocusReportingTerminalView(frame: NSRect(x: 0, y: 0, width: 800, height: 400))
        super.init()

        applyAppearance()
        terminalView.processDelegate = self

        // 用用户默认 shell,以 login shell 方式启动(execName 前缀 "-"),
        // 起始目录固定到用户主目录(否则 .app 从 / 启动,cwd 会是根目录)
        let shell = ProcessInfo.processInfo.environment["SHELL"] ?? "/bin/zsh"
        let shellName = (shell as NSString).lastPathComponent
        terminalView.startProcess(executable: shell,
                                  execName: "-\(shellName)",
                                  currentDirectory: NSHomeDirectory())
    }

    // MARK: - 外观(字体 + 主题)

    private func applyAppearance() {
        terminalView.font = config.font
        let theme = config.theme
        terminalView.nativeForegroundColor = theme.foreground
        terminalView.nativeBackgroundColor = theme.background
        terminalView.caretColor = theme.cursor
        if theme.ansi.count == 16 {
            terminalView.installColors(theme.ansi)
        }
    }

    /// 设置字号(保留字体家族),供 ⌘+/⌘- 调用
    func setFontSize(_ size: CGFloat) {
        config.fontSize = size
        terminalView.font = config.font
    }

    /// 切换主题(保留字号)
    func setTheme(_ themeName: String) {
        config.themeName = themeName
        applyAppearance()
    }

    // MARK: - 写入(唯一执行入口,均由用户确认后调用)

    /// 把一条命令写入终端并回车执行
    func run(_ command: String) {
        terminalView.send(txt: command + "\n")
    }

    /// 只把文字填进终端,不回车
    func type(_ text: String) {
        terminalView.send(txt: text)
    }

    /// 清屏:发送 Ctrl-L(form feed),shell 会重绘并清屏
    func clearScreen() {
        terminalView.send(txt: "\u{0C}")
    }

    // MARK: - LocalProcessTerminalViewDelegate

    func sizeChanged(source: LocalProcessTerminalView, newCols: Int, newRows: Int) {}

    func setTerminalTitle(source: LocalProcessTerminalView, title: String) {
        onTitleChange?(title)
    }

    func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {
        onDirectoryChange?(directory)
    }

    func processTerminated(source: TerminalView, exitCode: Int32?) {
        onTerminated?()
    }
}
