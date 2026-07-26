import AppKit

/// The "your turn" chime, played when a pane's status light turns green.
///
/// The audible counterpart of `SessionStatus.done` — driven from `SessionStore.tickStatus`, so it fires on
/// the same status transition the green dot renders, never independently of it.
enum DoneChime {
    /// One shared instance rather than a fresh `NSSound` per ring: decoding the file every time would put
    /// disk I/O on the status tick.
    private static let sound = NSSound(named: NSSound.Name("Glass"))

    /// A single long task can flicker done→busy→done (a tool call pausing between bursts of output), and a
    /// split can finish two panes on the same tick. Ring at most once per interval rather than once per
    /// transition, so "finished" sounds like one notification instead of a rattle.
    private static let minInterval: TimeInterval = 5
    private static var lastPlayedAt = Date.distantPast

    /// Ring, unless the user turned the chime off or one just played. Main thread only (called from the tick).
    static func play() {
        guard AppSettings.doneSoundEnabled else { return }
        let now = Date()
        guard now.timeIntervalSince(lastPlayedAt) >= minInterval else { return }
        lastPlayedAt = now
        // `play()` on a sound that is still sounding is a no-op, so rewind first.
        sound?.stop()
        sound?.play()
    }

    /// Play regardless of the throttle — for the Settings toggle, where the sound *is* the feedback.
    static func preview() {
        lastPlayedAt = Date()
        sound?.stop()
        sound?.play()
    }
}
