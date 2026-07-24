import AppKit

/// 按住空格说话(push-to-talk):
/// - 快速点按空格(< 阈值)→ 正常输入空格(keyUp 时补发)
/// - 按住空格(≥ 阈值)→ 开始录音,松开即停止
final class PushToTalkMonitor {
    var onHoldStart: (() -> Void)?
    var onHoldEnd: (() -> Void)?

    /// 长按判定阈值:低于它算普通空格输入
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
        // 只拦截无修饰键的空格;补发时(passthrough)直接放行
        guard event.keyCode == spaceKeyCode,
              event.modifierFlags.intersection([.command, .option, .control]).isEmpty,
              !passthrough else { return event }

        switch event.type {
        case .keyDown:
            if isHolding || event.isARepeat { return nil } // 按住期间吞掉系统按键重复
            pendingKeyDown = event
            let work = DispatchWorkItem { [weak self] in
                guard let self else { return }
                self.isHolding = true
                self.pendingKeyDown = nil
                self.onHoldStart?()
            }
            holdWork = work
            DispatchQueue.main.asyncAfter(deadline: .now() + holdThreshold, execute: work)
            return nil // 先吞掉;快速点按会在 keyUp 时补发

        case .keyUp:
            holdWork?.cancel()
            holdWork = nil
            if isHolding {
                isHolding = false
                onHoldEnd?()
                return nil
            }
            // 快速点按:补发原始空格 keyDown + keyUp,行为与正常打字一致
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
