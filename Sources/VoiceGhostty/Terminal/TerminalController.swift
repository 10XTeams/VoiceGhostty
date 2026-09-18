import AppKit
import Darwin
import SwiftTerm

/// Query a process's current working directory (does not rely on shell integration / OSC 7).
/// Use it on the shell process: even when a TUI like claude runs in the foreground, the shell's cwd is still "the folder you were in before entering it".
func processCurrentDirectory(_ pid: pid_t) -> String? {
    guard pid > 0 else { return nil }
    var info = proc_vnodepathinfo()
    let size = Int32(MemoryLayout<proc_vnodepathinfo>.size)
    guard proc_pidinfo(pid, PROC_PIDVNODEPATHINFO, 0, &info, size) == size else { return nil }
    return withUnsafeBytes(of: &info.pvi_cdir.vip_path) { buf in
        buf.baseAddress.map { String(cString: $0.assumingMemoryBound(to: CChar.self)) }
    }
}

/// Executable path of a process (`proc_pidpath`), used to recognise what a pane is running.
///
/// Deliberately the *path* and not `proc_name`: the native Claude Code installer execs
/// `~/.local/share/claude/versions/<version>`, so the process name is a bare version number ("2.1.276")
/// while the path still says "claude". (`ps -o comm` shows argv[0], which is why it prints "claude" and
/// the name lookup does not.)
func processExecutablePath(_ pid: pid_t) -> String? {
    guard pid > 0 else { return nil }
    var buffer = [CChar](repeating: 0, count: Int(MAXPATHLEN) * 4)
    guard proc_pidpath(pid, &buffer, UInt32(buffer.count)) > 0 else { return nil }
    return String(cString: buffer)
}

/// A subclass of LocalProcessTerminalView:
/// - The click callback (onFocus) lets SessionStore sync the "active split"
/// - PTY output (onOutput) / terminal bell (onBell) callbacks are used to judge "busy/done" status
///   (becomeFirstResponder is public and cannot be overridden, so mouseDown is used to detect focus;
///    dataReceived / bell are both open and can be safely intercepted.)
/// Note: these all fire on the **main thread**. SwiftTerm's `LocalProcessTerminalView` creates its
/// `LocalProcess(delegate:)` without a queue, which defaults to `DispatchQueue.main`, so pty reads are
/// already hopped to the main queue before `dataReceived` runs.
final class FocusReportingTerminalView: LocalProcessTerminalView {
    var onFocus: (() -> Void)?
    var onOutput: (() -> Void)?
    var onBell: (() -> Void)?

    override func mouseDown(with event: NSEvent) {
        onFocus?()
        super.mouseDown(with: event)
    }

    /// Send plain keyboard/IME text straight to the pty when the foreground app has enabled the kitty
    /// keyboard protocol without `report-all-keys`.
    ///
    /// SwiftTerm encodes *every* `insertText` as a kitty CSI-u event once an app requests
    /// `report-alternates` (flag 0b100), and drops the associated text while doing so — text only rides
    /// along when `report-all-keys` is set too. For a key whose base-layout glyph differs from what the
    /// IME committed, the character is then lost outright.
    ///
    /// A Chinese IME commits full-width punctuation (，。？！；：、《》…) directly, with no candidate
    /// window, so the key event still describes the physical key: `charactersIgnoringModifiers` is "，"
    /// while the ASCII base layout for keyCode 43 is ",". SwiftTerm sees a base-layout alternate, emits
    /// `CSI 65292:…;…u` carrying no text, and the receiving app — which only knows ASCII key codes —
    /// discards it. Han characters are unaffected: they arrive through the composition-commit path,
    /// which has no key event behind it and therefore no alternates.
    ///
    /// Claude Code negotiates exactly these flags (`CSI > 5 u` = disambiguate | report-alternates),
    /// which is why full-width punctuation vanished inside it yet typed fine at the shell prompt — zsh
    /// never enables the protocol, so input took SwiftTerm's ordinary `send(txt:)` path.
    ///
    /// Per the kitty spec, text-producing keys keep being sent as plain UTF-8 unless `report-all-keys`
    /// is set; `report-alternates` only enriches events that were already escape-encoded. Bypassing
    /// SwiftTerm here restores that behaviour rather than papering over it. Pastes are untouched — they
    /// go through SwiftTerm's internal `isPaste: true` entry point, not this override.
    private func shouldBypassKittyEncoding(_ text: String) -> Bool {
        let flags: KittyKeyboardFlags = terminal?.keyboardEnhancementFlags ?? KittyKeyboardFlags()
        guard !flags.isEmpty, !flags.contains(.reportAllKeys), !text.isEmpty else { return false }
        // C0/C1 scalars keep their normal encoding; only printable text takes the fast path.
        guard !text.unicodeScalars.contains(where: { $0.value < 0x20 || (0x7F ... 0x9F).contains($0.value) }) else {
            return false
        }
        // Control/Command chords are keys, not text, and stay with SwiftTerm's encoder.
        let modifiers = NSApp.currentEvent?.modifierFlags ?? []
        return !modifiers.contains(.control) && !modifiers.contains(.command)
    }

    /// Normalize IME input to `NSString` before SwiftTerm sees it.
    ///
    /// `NSTextInputClient.insertText` may be handed either an `NSString` or an `NSAttributedString`,
    /// but SwiftTerm's implementation is a bare `if let str = string as? NSString` — an attributed
    /// string falls off the end and the characters are dropped without a trace.
    override func insertText(_ string: Any, replacementRange: NSRange) {
        let text = (string as? NSAttributedString)?.string ?? (string as? String)

        if let text, shouldBypassKittyEncoding(text) {
            // `super.insertText` is what normally retires the composition: it drops the marked-text
            // storage and takes the underlined preedit overlay off screen. Skipping it would leave the
            // pinyin the IME had shown ("zhong") floating over the terminal after the Han characters
            // were already committed, until the *next* composition happened to redraw the overlay.
            unmarkText()
            send(txt: text)
            return
        }

        if let attributed = string as? NSAttributedString {
            super.insertText(attributed.string as NSString, replacementRange: replacementRange)
        } else {
            super.insertText(string, replacementRange: replacementRange)
        }
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

/// Holds a SwiftTerm terminal view, responsible for starting the PTY/zsh, applying config, and writing commands.
/// One TerminalController == one zsh session (one tab or one split pane).
final class TerminalController: NSObject, ObservableObject, LocalProcessTerminalViewDelegate {
    let terminalView: FocusReportingTerminalView

    /// The one and only superview `terminalView` ever has, created with the controller and never
    /// re-parented afterwards.
    ///
    /// SwiftUI's representable hands this exact instance back from `makeNSView`, so the AppKit view
    /// tree under a pane is only ever mutated by SwiftUI itself, at the times it expects. The previous
    /// design gave SwiftUI a fresh container each time and re-parented the terminal into it, which meant
    /// two live representables for one pane could hand the view back and forth *between* layout passes —
    /// leaving SwiftUI's StackLayout cache pointing at children that had moved, and crashing in
    /// `StackLayout.makeChildren` with EXC_BAD_ACCESS.
    let hostContainer: NSView

    /// Current font size (read/written when SessionStore scales uniformly)
    private(set) var config: Config

    /// The shell process's current working directory (used by polling, does not rely on OSC 7)
    var currentShellDirectory: String? {
        guard let process = terminalView.process else { return nil }
        return processCurrentDirectory(process.shellPid)
    }

    /// True when the tty line discipline is in raw mode (ICANON off), i.e. whatever is in the foreground
    /// reads keystrokes itself instead of letting the kernel do line editing. nil when unknown.
    ///
    /// The pty primary reflects the secondary's termios, so a single tcgetattr tells apart the two cases
    /// the status lights confuse (measured on macOS): a plain command like `sleep 4` runs with ICANON *on*
    /// (zsh restores canonical mode before exec'ing it), while an interactive TUI — claude, vim, less —
    /// turns it off for as long as it owns the keyboard. At the prompt zle also runs raw, but the status
    /// code only asks while a command is running, so that case never comes up.
    var isRawMode: Bool? {
        guard let fd = terminalView.process?.childfd, fd >= 0 else { return nil }
        var attrs = termios()
        guard tcgetattr(fd, &attrs) == 0 else { return nil }
        return attrs.c_lflag & tcflag_t(ICANON) == 0
    }

    /// pid of the pty's foreground process group — whatever currently owns the keyboard. The pty primary
    /// answers `tcgetpgrp` on behalf of the secondary, the same way it answers `tcgetattr` above.
    var foregroundPID: pid_t? {
        guard let fd = terminalView.process?.childfd, fd >= 0 else { return nil }
        let pgid = tcgetpgrp(fd)
        return pgid > 0 ? pgid : nil
    }

    /// The directory a Claude Code session is running in inside this pane, or nil when the foreground
    /// process is something else (a shell prompt, vim, a build).
    ///
    /// Asks the foreground process itself rather than the shell: identical in the ordinary `cd x && claude`
    /// case (a TUI does not chdir, and the shell keeps the directory it launched from), but still right when
    /// claude was started from a subshell. A tool call claude makes does not disturb this — it spawns its
    /// children without a pty of their own, so the foreground process group stays claude's.
    var claudeWorkingDirectory: String? {
        guard let pid = foregroundPID, let path = processExecutablePath(pid) else { return nil }
        // Matches the native install path (…/.local/share/claude/versions/<v>) and a plain `claude`
        // binary alike. An npm/node install execs "node", which this does not recognise.
        guard path.lowercased().contains("claude") else { return nil }
        return processCurrentDirectory(pid)
    }

    /// Fired when SwiftUI tears down the representable hosting this pane — see `TerminalHostView`.
    /// `SessionStore.movePane` arms it to learn that a detached pane has really been let go, before it
    /// re-attaches the pane under a different tab.
    var onRepresentableDismantled: (() -> Void)?

    /// Focus/title/working-directory/process-exit callbacks, taken over by SessionStore
    var onFocus: (() -> Void)? {
        get { terminalView.onFocus }
        set { terminalView.onFocus = newValue }
    }
    /// PTY has output (background thread); / terminal bell (background thread)
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
    /// OSC 133 shell-integration command boundaries (background thread): C = command started, D = command finished.
    var onCommandStart: (() -> Void)?
    var onCommandEnd: (() -> Void)?

    init(config: Config) {
        self.config = config
        let bounds = NSRect(x: 0, y: 0, width: 800, height: 400)
        terminalView = FocusReportingTerminalView(frame: bounds)
        hostContainer = NSView(frame: bounds)
        super.init()

        // Permanent parent/child link, established once here so nothing else ever has to move the view.
        hostContainer.autoresizesSubviews = true
        terminalView.autoresizingMask = [.width, .height]
        hostContainer.addSubview(terminalView)

        applyAppearance()
        terminalView.processDelegate = self

        // Raise the history ceiling off SwiftTerm's 500-line default before the shell prints anything,
        // so nothing is lost between startup and the first user action.
        terminalView.getTerminal().changeScrollback(AppSettings.scrollback)

        // OSC 133 shell integration: precise command start/end (see the hooks injected in installShellSetup).
        // The handler runs on the parser (background) thread — it may only fan out to the callbacks, which mutate plain vars.
        terminalView.getTerminal().registerOscHandler(code: 133) { [weak self] data in
            switch data.first {
            case UInt8(ascii: "C"): self?.onCommandStart?()
            case UInt8(ascii: "D"): self?.onCommandEnd?()
            default: break
            }
        }

        // Use the user's default shell, started as a login shell (execName prefixed with "-"),
        // with the starting directory fixed to the user's home directory (otherwise the .app starts from /, and cwd would be the root directory)
        let shell = ProcessInfo.processInfo.environment["SHELL"] ?? "/bin/zsh"
        let shellName = (shell as NSString).lastPathComponent

        // For zsh, install our shell integration (prompt name, input-color hook, OSC 133) via a ZDOTDIR wrapper so
        // it takes effect *before the first prompt is drawn* — no startup flash of the original prompt.
        var environment: [String]? = nil
        if shellName == "zsh", let zdotdir = Self.prepareZDotDir() {
            environment = Terminal.getEnvironmentVariables(termName: "xterm-256color") + ["ZDOTDIR=\(zdotdir)"]
        }

        terminalView.startProcess(executable: shell,
                                  args: [],
                                  environment: environment,
                                  execName: "-\(shellName)",
                                  currentDirectory: NSHomeDirectory())
    }

    // MARK: - Shell integration (ZDOTDIR wrapper)

    /// Write a ZDOTDIR wrapper (`~/.config/voiceghostty/shell/`) whose dotfiles first source the user's real
    /// `~/.z*` files, then append VoiceGhostty's integration, and return its path (nil on failure).
    /// Running inside `.zshrc` means everything is in place *before the first prompt is drawn* — no startup flash.
    ///
    /// Integration installed (the user's own prompt is left untouched):
    /// - **Input highlighting** — a `line-pre-redraw` hook colors the command line you type; it re-reads the
    ///   color file each redraw (builtin `read`, no subprocess), so a Settings color change applies live.
    /// - **OSC 133** — `preexec`/`precmd` emit command start/end markers that drive the precise status lights.
    private static func prepareZDotDir() -> String? {
        AppSettings.writeInputColorFile()   // ensure the color file exists before the hook reads it
        let dir = (NSHomeDirectory() as NSString).appendingPathComponent(".config/voiceghostty/shell")
        let colorFile = AppSettings.inputColorFilePath
        let fm = FileManager.default
        do {
            try fm.createDirectory(atPath: dir, withIntermediateDirectories: true)

            let zshenv = """
            [ -f "$HOME/.zshenv" ] && source "$HOME/.zshenv"
            ZDOTDIR='\(dir)'
            """
            let zprofile = #"[ -f "$HOME/.zprofile" ] && source "$HOME/.zprofile""#
            let zlogin   = #"[ -f "$HOME/.zlogin" ] && source "$HOME/.zlogin""#
            let zshrc = """
            [ -f "$HOME/.zshrc" ] && source "$HOME/.zshrc"
            # --- VoiceGhostty shell integration (auto-generated; regenerated each launch) ---
            export COLORTERM=truecolor
            autoload -Uz add-zsh-hook add-zle-hook-widget
            _vg_ic(){ local c; IFS= read -r c < '\(colorFile)' 2>/dev/null; region_highlight=("0 ${#BUFFER} fg=${c:-#5CC8FF}") }
            add-zle-hook-widget line-pre-redraw _vg_ic
            _vg_pe(){ print -n '\\e]133;C\\e\\\\' }
            _vg_pc(){ print -n '\\e]133;D\\e\\\\' }
            add-zsh-hook preexec _vg_pe
            add-zsh-hook precmd _vg_pc
            """
            try zshenv.write(toFile: dir + "/.zshenv", atomically: true, encoding: .utf8)
            try zprofile.write(toFile: dir + "/.zprofile", atomically: true, encoding: .utf8)
            try zlogin.write(toFile: dir + "/.zlogin", atomically: true, encoding: .utf8)
            try zshrc.write(toFile: dir + "/.zshrc", atomically: true, encoding: .utf8)
            return dir
        } catch {
            return nil
        }
    }

    // MARK: - Appearance (font + theme)

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

    /// Set the font size (keeping the font family), called by ⌘+/⌘-
    func setFontSize(_ size: CGFloat) {
        config.fontSize = size
        terminalView.font = config.font
    }

    /// Change how many scrolled-off lines this pane keeps. Applied live, so a Settings change reaches
    /// panes that are already open; shrinking it drops the oldest lines immediately.
    func setScrollback(_ lines: Int) {
        terminalView.getTerminal().changeScrollback(lines)
    }

    /// The terminal buffer, for the transcript logger (which reads finalized lines out of it).
    var terminal: Terminal { terminalView.getTerminal() }

    /// Switch theme (keeping the font size)
    func setTheme(_ themeName: String) {
        config.themeName = themeName
        applyAppearance()
    }

    // MARK: - Writing (the only execution entry points, all called after user confirmation)

    /// Write a command into the terminal and press Enter to execute
    func run(_ command: String) {
        terminalView.send(txt: command + "\n")
    }

    /// Only type the text into the terminal, without pressing Enter
    func type(_ text: String) {
        terminalView.send(txt: text)
    }

    /// Clear the screen: send Ctrl-L (form feed), and the shell redraws and clears the screen
    func clearScreen() {
        terminalView.send(txt: "\u{0C}")
    }

    // MARK: - Shutdown

    /// Hang up the session: SIGTERM to the shell plus closing the pty primary, which SIGHUPs whatever
    /// foreground job it was running. Must be called explicitly when a pane/tab closes — merely dropping
    /// the session would leave the shell (and anything it started) running with nothing attached to it.
    func terminate() {
        terminalView.process?.terminate()
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
