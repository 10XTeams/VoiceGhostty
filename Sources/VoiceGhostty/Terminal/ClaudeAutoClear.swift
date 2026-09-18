import Foundation
import SwiftTerm

extension Notification.Name {
    /// Posted (on the main thread) right after an Auto Clear sequence starts typing, so the status bar can
    /// say so. `object` is nil; the pane is not identified because only the active pane's strip shows it.
    static let claudeAutoClearFired = Notification.Name("voiceghostty.auto-clear-fired")
}

/// Auto Clear: lets a pane running Claude Code clear itself once claude says it is ready, and start the
/// next piece of work — the "context is nearly full, hand over and move on" loop, without you sitting there
/// to press Return.
///
/// It fires only when all of these hold:
/// 1. the setting is on (`AppSettings.autoClearEnabled`, off by default),
/// 2. claude is the pane's foreground process,
/// 3. the context reading is above `tokenThreshold` — a clear is pointless while the window is still roomy,
/// 4. claude's output currently shows the `***Clear Ready***` marker.
///
/// Then it types `/clear`, Return, `/Opera:next`, Return.
///
/// **The marker is matched loosely on purpose.** Claude Code renders markdown, so `***Clear Ready***` may
/// reach the screen as styled text with the asterisks gone. Matching strips `*`, folds whitespace and
/// lowercases, which catches both the raw and the rendered spelling.
///
/// **The screen is the source of truth, not the pty stream** — the same reasoning as `SessionLogger`: by the
/// time a line is in the buffer, SwiftTerm has applied every escape sequence, so a marker split across
/// reads or painted in pieces is one ordinary line here.
///
/// **It cannot fire twice for one marker.** A sequence disarms it until a scan sees the marker gone (which
/// `/clear` itself brings about), and a cooldown covers the case where the marker somehow survives the
/// clear — the failure to avoid is a pane that clears itself in a loop.
final class ClaudeAutoClear {
    /// Matched against a normalized line: asterisks dropped, whitespace folded, lowercased.
    static let marker = "clear ready"
    /// Below this, a clear would throw away a context that is not in anyone's way yet.
    static let tokenThreshold = 150_000
    static let clearCommand = "/clear"
    static let followUpCommand = "/Opera:next"

    /// Gap between typing a slash command and pressing Return. Claude filters its command menu as you type,
    /// and a Return in the same breath can be taken by whatever the menu had highlighted a moment earlier.
    private static let returnDelay: TimeInterval = 0.4
    /// How long `/clear` is given to land before the follow-up command is typed.
    private static let followUpDelay: TimeInterval = 1.5
    /// Nothing may fire again this soon after a sequence, whatever is on screen. Public because the
    /// Settings blurb states it — a rule you are told about is a rule you can trust.
    static let cooldownSeconds: TimeInterval = 30

    /// False between firing and the next scan that finds the marker gone.
    private var armed = true
    private var lastFired = Date.distantPast
    /// True while a sequence is still typing itself out.
    private var sending = false
    /// Where the last line walk ended, so a scan does not re-walk the whole scrollback each time.
    private var lastEnd = 0

    /// Called from the status poll. `type` writes to the pty *without* a newline of its own — this decides
    /// when Return is pressed.
    func evaluate(terminal: Terminal, tokens: Int?, type: @escaping (String) -> Void) {
        guard !sending else { return }
        guard markerVisible(in: terminal) else {
            armed = true            // marker gone: the next one is a new one
            return
        }
        guard armed, Date().timeIntervalSince(lastFired) > Self.cooldownSeconds else { return }
        guard let tokens, tokens > Self.tokenThreshold else { return }
        armed = false
        lastFired = Date()
        send(type: type)
    }

    // MARK: - Detection

    private func markerVisible(in terminal: Terminal) -> Bool {
        // The alternate buffer (vim, less) has its own row numbering; walking it with scroll-invariant rows
        // would read nonsense. Claude Code does not use it.
        guard !terminal.isCurrentBufferAlternate else { return false }

        let floor = terminal.buffer.totalLinesTrimmed   // oldest row still in memory
        var end = max(lastEnd, floor)
        // A `clear` resets the buffer's numbering and can leave the cached anchor past the end.
        if terminal.getScrollInvariantLine(row: end) == nil { end = floor }
        while terminal.getScrollInvariantLine(row: end) != nil { end += 1 }
        lastEnd = max(floor, end - 1)

        // Only what is on screen (plus a little slack for a line that just scrolled off between polls):
        // claude repaints its message blocks, so the marker is on screen for as long as it is current, and
        // scanning further back would keep finding a marker from an earlier round.
        let start = max(floor, end - terminal.rows - 10)
        guard start < end else { return false }
        for row in start ..< end {
            guard let line = terminal.getScrollInvariantLine(row: row) else { continue }
            if Self.matches(line.translateToString(trimRight: true)) { return true }
        }
        return false
    }

    /// Whether one screen line carries the marker, in either its raw or its rendered spelling.
    static func matches(_ line: String) -> Bool {
        let normalized = line
            .replacingOccurrences(of: "*", with: " ")
            .lowercased()
            .split(whereSeparator: { $0.isWhitespace })
            .joined(separator: " ")
        return normalized.contains(marker)
    }

    // MARK: - Sending

    /// `/clear` ⏎ … `/Opera:next` ⏎, typed with the pauses a TUI needs rather than pasted in one burst.
    /// Return is `\r`, which is what a terminal sends for the Return key — claude reads keys itself and
    /// never sees the line discipline that would accept `\n`.
    private func send(type: @escaping (String) -> Void) {
        sending = true
        let steps: [(TimeInterval, String)] = [
            (0, Self.clearCommand),
            (Self.returnDelay, "\r"),
            (Self.returnDelay + Self.followUpDelay, Self.followUpCommand),
            (Self.returnDelay + Self.followUpDelay + Self.returnDelay, "\r"),
        ]
        for (delay, text) in steps {
            DispatchQueue.main.asyncAfter(deadline: .now() + delay) { type(text) }
        }
        let done = steps.last?.0 ?? 0
        DispatchQueue.main.asyncAfter(deadline: .now() + done + 0.2) { [weak self] in self?.sending = false }
        NotificationCenter.default.post(name: .claudeAutoClearFired, object: nil)
    }
}
