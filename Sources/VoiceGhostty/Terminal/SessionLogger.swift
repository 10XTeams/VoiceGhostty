import Foundation
import SwiftTerm

/// Appends a pane's transcript to a plain-text file under `~/.config/voiceghostty/history/`.
///
/// The source of truth is the terminal buffer, not the raw pty stream: by the time a line reaches the
/// buffer, SwiftTerm has already applied every escape sequence, so a redrawing TUI (claude's spinner,
/// its re-rendered message blocks) lands as the one final line you actually saw rather than dozens of
/// half-painted ones. Only lines that have scrolled off the top of the screen are written, because a
/// line still on screen can be repainted; `finish()` then flushes what remains at shutdown.
///
/// Writing incrementally — rather than dumping the buffer — means the file keeps the whole session even
/// after `scrollback` trims the oldest lines out of memory.
final class SessionLogger {
    /// Absolute (scroll-invariant) index of the next line to write.
    private var nextRow = 0
    private var handle: FileHandle?
    private var openFailed = false
    private let startedAt: Date

    init(startedAt: Date = Date()) {
        self.startedAt = startedAt
    }

    static var directory: String {
        (NSHomeDirectory() as NSString).appendingPathComponent(".config/voiceghostty/history")
    }

    /// Path of this session's file, once one has been created (nil until the first line is written).
    private(set) var path: String?

    /// The folder the pane is in, kept current by the session's cwd poll; used to name the file.
    var currentDirectory: String?

    // MARK: - Appending

    /// Write every line that has scrolled off screen since the last call.
    func flush(_ terminal: Terminal) { append(from: terminal, includingScreen: false) }

    /// Final write: also takes the lines still on screen, then closes the file.
    func finish(_ terminal: Terminal) {
        append(from: terminal, includingScreen: true)
        try? handle?.close()
        handle = nil
    }

    private func append(from terminal: Terminal, includingScreen: Bool) {
        // The alternate buffer (vim, less) has no scrollback and its own row numbering — logging it would
        // interleave garbage into the transcript and corrupt our absolute-row anchor.
        guard !terminal.isCurrentBufferAlternate else { return }

        let floor = terminal.buffer.totalLinesTrimmed   // oldest line still in memory
        if nextRow < floor {
            write("\n[… \(floor - nextRow) line(s) dropped: scrollback limit reached …]\n")
            nextRow = floor
        }
        // `clear` resets the buffer's row numbering, which leaves our anchor pointing past the end.
        // Detect that (a live buffer always has a line at `floor`) and re-anchor instead of stalling.
        if terminal.getScrollInvariantLine(row: nextRow) == nil, nextRow > floor {
            nextRow = floor
        }

        var end = nextRow
        while terminal.getScrollInvariantLine(row: end) != nil { end += 1 }
        // Lines from `end - rows` on are the current screen: still repaintable, so not final yet.
        let limit = includingScreen ? end : max(nextRow, end - terminal.rows)
        guard limit > nextRow else { return }

        var text = ""
        while nextRow < limit {
            if let line = terminal.getScrollInvariantLine(row: nextRow) {
                text += line.translateToString(trimRight: true) + "\n"
            }
            nextRow += 1
        }
        write(text)
    }

    // MARK: - File

    /// Append to the file, creating it on first use. Named after the folder the session was working in
    /// *at that moment* plus its start time — panes all start in the home directory, so waiting for real
    /// output means the name is usually the project you cd'd into.
    private func write(_ text: String) {
        guard !text.isEmpty, !openFailed else { return }
        guard let data = text.data(using: .utf8) else { return }
        if handle == nil { open() }
        guard let handle else { return }
        handle.seekToEndOfFile()
        handle.write(data)
    }

    private func open() {
        let dir = Self.directory
        let file = (dir as NSString).appendingPathComponent(fileName())
        let fm = FileManager.default
        do {
            try fm.createDirectory(atPath: dir, withIntermediateDirectories: true)
            if !fm.fileExists(atPath: file) {
                fm.createFile(atPath: file, contents: nil)
            }
            handle = try FileHandle(forWritingTo: URL(fileURLWithPath: file))
            path = file
        } catch {
            openFailed = true   // read-only home, no permission… log silently rather than nagging per flush
        }
    }

    /// `<folder>-<yyyyMMdd-HHmmss>.log`, with anything that would break a filename replaced.
    private func fileName() -> String {
        let stamp = DateFormatter.historyStamp.string(from: startedAt)
        return "\(folderName())-\(stamp).log"
    }

    private func folderName() -> String {
        var name = ((currentDirectory ?? NSHomeDirectory()) as NSString).lastPathComponent
        if name.isEmpty || name == "/" { name = "root" }
        return name.replacingOccurrences(of: "/", with: "-")
                   .replacingOccurrences(of: ":", with: "-")
    }
}

private extension DateFormatter {
    static let historyStamp: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "yyyyMMdd-HHmmss"
        return f
    }()
}
