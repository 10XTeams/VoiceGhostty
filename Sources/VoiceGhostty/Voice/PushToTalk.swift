import AppKit

/// Push-to-talk (hold space to speak):
/// - Quick tap of space (< threshold) → normal space input (re-sent on keyUp)
/// - Hold space (≥ threshold) → start recording, stop on release
final class PushToTalkMonitor {
    var onHoldStart: (() -> Void)?
    var onHoldEnd: (() -> Void)?

    /// Hold-detection threshold: below it counts as a normal space input
    private let holdThreshold: TimeInterval = 0.25
    private let spaceKeyCode: UInt16 = 49

    private var monitor: Any?
    private var pendingKeyDown: NSEvent?
    private var holdWork: DispatchWorkItem?
    private var isHolding = false
    private var passthrough = false

    func start() {
        guard monitor == nil else { return }
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .keyUp]) { [weak self] event in
            guard let self else { return event }
            return self.handle(event)
        }
    }

    func stop() {
        if let monitor { NSEvent.removeMonitor(monitor) }
        monitor = nil
        holdWork?.cancel()
        holdWork = nil
        pendingKeyDown = nil
        isHolding = false
    }

    private func handle(_ event: NSEvent) -> NSEvent? {
        // Only intercept unmodified space; pass through directly when re-sending (passthrough)
        guard event.keyCode == spaceKeyCode,
              event.modifierFlags.intersection([.command, .option, .control]).isEmpty,
              !passthrough else { return event }

        switch event.type {
        case .keyDown:
            if isHolding || event.isARepeat { return nil } // Swallow system key repeats while holding
            pendingKeyDown = event
            let work = DispatchWorkItem { [weak self] in
                guard let self else { return }
                self.isHolding = true
                self.pendingKeyDown = nil
                self.onHoldStart?()
            }
            holdWork = work
            DispatchQueue.main.asyncAfter(deadline: .now() + holdThreshold, execute: work)
            return nil // Swallow for now; a quick tap will be re-sent on keyUp

        case .keyUp:
            holdWork?.cancel()
            holdWork = nil
            if isHolding {
                isHolding = false
                onHoldEnd?()
                return nil
            }
            // Quick tap: re-send the original space keyDown + keyUp, behaving like normal typing
            if let down = pendingKeyDown {
                pendingKeyDown = nil
                passthrough = true
                NSApp.sendEvent(down)
                NSApp.sendEvent(event)
                passthrough = false
                return nil
            }
            return event

        default:
            return event
        }
    }
}
