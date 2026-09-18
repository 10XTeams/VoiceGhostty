import Foundation

/// Context-window usage of the Claude Code session running in a pane, as of its last API request.
struct ClaudeContextUsage: Equatable {
    /// Prompt + output tokens of the last request — near enough to what the *next* request will carry,
    /// which is the number worth watching. `cache_read` dominates it and is still context.
    let tokens: Int
    /// The model's context window: 200K, or 1M when the transcript names a `[1m]` model.
    let limit: Int
    let model: String?
    let sessionID: String?
    /// Transcript file the numbers came from (shown in the tooltip, so a two-pane project is explainable).
    let transcript: String

    var fraction: Double { limit > 0 ? min(1, Double(tokens) / Double(limit)) : 0 }
    var percent: Int { Int((fraction * 100).rounded()) }

    /// "65K / 1M" — short enough for a status strip that must stay one line.
    var short: String { "\(Self.compact(tokens)) / \(Self.compact(limit))" }

    static func compact(_ n: Int) -> String {
        if n >= 1_000_000 {
            let m = Double(n) / 1_000_000
            return m >= 10 || m == m.rounded() ? "\(Int(m.rounded()))M" : String(format: "%.1fM", m)
        }
        if n >= 1_000 { return "\(Int((Double(n) / 1_000).rounded()))K" }
        return "\(n)"
    }
}

/// Reads the context size of the Claude Code session running in one pane, out of Claude Code's own
/// transcript.
///
/// Claude Code appends one JSON object per record to `~/.claude/projects/<slugged cwd>/<session>.jsonl`,
/// and every assistant record carries the request's `usage`. Reading that is strictly better than scraping
/// the TUI: it costs no API call, needs no cooperation from the pane, and does not break when claude
/// changes what its screen looks like. What it cannot do is report the request that is *in flight* — the
/// record is written once the response completes, so the figure trails the live session by one turn.
///
/// The file is consumed incrementally (`offset`, `carry`), the same way `SessionLogger` writes one: the
/// first poll of a transcript parses all of it — that is where the model identity lives, near the top —
/// and every poll after it reads only the bytes that were appended since.
///
/// Its own `ObservableObject`, not a `@Published` on `TerminalSession`: the session is observed by
/// `PaneView`, which hosts the terminal, so publishing from there would re-run a terminal host every time
/// a token count moved. Only the status-bar meter observes this object.
final class ClaudeUsageWatcher: ObservableObject {
    @Published private(set) var usage: ClaudeContextUsage?

    /// Standard context window, and the long one a `[1m]` model id selects.
    static let standardLimit = 200_000
    static let longLimit = 1_000_000

    /// One shared serial queue for all panes: the cursor state below is only ever touched on it, and
    /// scans are short enough that serializing a handful of panes costs nothing.
    private static let queue = DispatchQueue(label: "voiceghostty.claude-usage")

    // Cursor + last-known values. Queue-confined.
    private var path: String?
    private var offset: UInt64 = 0
    private var carry = Data()
    private var tokens: Int?
    private var model: String?
    /// From the transcript's model attachment, which — unlike `message.model` — keeps the `[1m]` suffix.
    private var modelID: String?
    private var sessionID: String?

    /// True while a scan is queued, so a slow poll cannot pile up behind itself. Main-thread only.
    private var scanning = false
    /// Whether the cursor is on a transcript, so the twice-a-minute idle poll of a pane with no claude in
    /// it does not keep enqueuing a reset that has nothing left to reset. Main-thread only.
    private var attached = false

    /// Poll the transcript for the Claude Code session in `directory`, or clear the reading when nil
    /// (nothing claude-shaped in the foreground). Call from the main thread.
    func refresh(directory: String?) {
        guard let directory else {
            if usage != nil { usage = nil }
            if attached {
                attached = false
                Self.queue.async { [weak self] in self?.resetCursor(keepingPath: false) }
            }
            return
        }
        guard !scanning else { return }
        scanning = true
        attached = true
        Self.queue.async { [weak self] in
            guard let self else { return }
            let next = self.scan(directory: directory)
            DispatchQueue.main.async {
                self.scanning = false
                if self.usage != next { self.usage = next }
            }
        }
    }

    // MARK: - Scanning (queue)

    private func scan(directory: String) -> ClaudeContextUsage? {
        let candidates = Self.projectDirectories(forWorkingDirectory: directory)
        guard let file = candidates.compactMap({ Self.newestTranscript(in: $0) })
            .max(by: { (Self.modified($0) ?? .distantPast) < (Self.modified($1) ?? .distantPast) }) else {
            resetCursor(keepingPath: false)
            return nil
        }
        if file != path {
            resetCursor(keepingPath: false)
            path = file
        }
        guard let handle = FileHandle(forReadingAtPath: file) else { return nil }
        defer { try? handle.close() }

        // A shorter file than our cursor means this is not the file we were reading any more (rotated,
        // rewritten): start over rather than seek past the end.
        let size = (try? handle.seekToEnd()) ?? 0
        if size < offset { resetCursor(keepingPath: true) }
        guard size > offset else { return reading() }

        try? handle.seek(toOffset: offset)
        guard let fresh = try? handle.readToEnd(), !fresh.isEmpty else { return reading() }
        offset = size

        var data = carry
        data.append(fresh)
        carry = Data()
        var start = data.startIndex
        while let newline = data[start...].firstIndex(of: 0x0A) {
            consume(data[start ..< newline])
            start = data.index(after: newline)
        }
        // Whatever follows the last newline is a record claude is still writing; hold it for next time.
        if start < data.endIndex { carry = Data(data[start...]) }
        return reading()
    }

    /// Fold one transcript record into the last-known values.
    private func consume(_ line: Data) {
        guard line.count > 2 else { return }
        // Substring gate before parsing: JSON-decoding every record of a multi-megabyte transcript would
        // cost more than everything else in the poll put together. Gating on the two field *names* we
        // need — rather than on `"type":"assistant"` — keeps it independent of how the writer spaces its
        // JSON. A false positive (a tool result that happens to quote one of them) is harmless: the
        // record's structure is still checked below.
        guard let text = String(data: line, encoding: .utf8),
              text.contains("\"usage\"") || text.contains("\"modelId\"") else { return }
        guard let record = try? JSONSerialization.jsonObject(with: line) as? [String: Any] else { return }

        if record["type"] as? String == "assistant",
           // A subagent's turns are billed to their own context window, not this session's.
           record["isSidechain"] as? Bool != true,
           let message = record["message"] as? [String: Any],
           let usage = message["usage"] as? [String: Any] {
            let field = { (key: String) in usage[key] as? Int ?? 0 }
            tokens = field("input_tokens")
                + field("cache_creation_input_tokens")
                + field("cache_read_input_tokens")
                + field("output_tokens")
            model = message["model"] as? String ?? model
            sessionID = record["sessionId"] as? String ?? sessionID
        }

        if let attachment = record["attachment"] as? [String: Any],
           attachment["type"] as? String == "model",
           let identity = attachment["identity"] as? [String: Any],
           let id = identity["modelId"] as? String {
            modelID = id
        }
    }

    private func reading() -> ClaudeContextUsage? {
        guard let tokens, let path else { return nil }
        return ClaudeContextUsage(tokens: tokens,
                                  limit: limit(for: tokens),
                                  model: modelID ?? model,
                                  sessionID: sessionID,
                                  transcript: path)
    }

    /// `[1m]` in the model id means the long window. A reading that overflows the window we assumed is
    /// itself proof of a long-context model we do not recognise — believe the tokens, not the guess,
    /// rather than pinning the meter at 100%.
    private func limit(for tokens: Int) -> Int {
        let base = modelID?.contains("[1m]") == true ? Self.longLimit : Self.standardLimit
        return tokens > base ? Self.longLimit : base
    }

    private func resetCursor(keepingPath: Bool) {
        if !keepingPath { path = nil }
        offset = 0
        carry = Data()
        tokens = nil
        model = nil
        modelID = nil
        sessionID = nil
    }

    // MARK: - Locating the transcript

    static var projectsDirectory: String {
        (NSHomeDirectory() as NSString).appendingPathComponent(".claude/projects")
    }

    /// Claude Code's folder for a working directory: the path with its separators replaced by dashes
    /// (`/Users/me/app` → `-Users-me-app`). Dots are dashed too in some versions, so both spellings are
    /// offered and the caller takes whichever exists.
    static func projectDirectories(forWorkingDirectory cwd: String) -> [String] {
        let dashed = String(cwd.map { $0 == "/" ? "-" : $0 })
        let dotless = String(dashed.map { $0 == "." ? "-" : $0 })
        let root = projectsDirectory as NSString
        return (dashed == dotless ? [dashed] : [dashed, dotless])
            .map { root.appendingPathComponent($0) }
    }

    /// The most recently written `.jsonl` in a project folder.
    ///
    /// Two panes running claude in the same project therefore read the same file — the transcript names
    /// no pid, so there is nothing to tell their sessions apart by. The tooltip names the session id, so
    /// the reading is at least attributable.
    private static func newestTranscript(in directory: String) -> String? {
        let fm = FileManager.default
        guard let names = try? fm.contentsOfDirectory(atPath: directory) else { return nil }
        var newest: (path: String, date: Date)?
        for name in names where name.hasSuffix(".jsonl") {
            let file = (directory as NSString).appendingPathComponent(name)
            guard let date = modified(file) else { continue }
            if newest == nil || date > newest!.date { newest = (file, date) }
        }
        return newest?.path
    }

    private static func modified(_ file: String) -> Date? {
        (try? FileManager.default.attributesOfItem(atPath: file))?[.modificationDate] as? Date
    }
}
