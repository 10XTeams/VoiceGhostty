import SwiftUI

@main
struct VoiceGhosttyApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var appDelegate
    @StateObject private var store = SessionStore()

    var body: some Scene {
        WindowGroup("VoiceGhostty") {
            ContentView()
                .environmentObject(store)
                .frame(minWidth: 800, minHeight: 500)
        }
        .commands {
            AppCommands(store: store)
        }
    }
}

/// 菜单栏命令 —— 标签 / 分屏 / 字号 / 主题 / 搜索的键盘快捷键都挂在这里。
struct AppCommands: Commands {
    @ObservedObject var store: SessionStore

    var body: some Commands {
        // 复制/粘贴/全选走系统默认「编辑」菜单(SwiftTerm 已实现响应链方法)

        // ★ ⌘W 必须替换掉系统「文件 > 关闭」:两个菜单项撞快捷键时 AppKit 取菜单顺序
        // 靠前的那个,「文件」在「终端」前面,系统 Close 会抢走 ⌘W 直接关窗退出 app。
        CommandGroup(replacing: .saveItem) {
            Button("关闭标签/分屏") { store.closeActivePaneOrTab() }
                .keyboardShortcut("w", modifiers: [.command])
        }

        CommandMenu("终端") {
            Button("新建标签") { store.addTab() }
                .keyboardShortcut("t", modifiers: [.command])
            Button("向右分屏") { store.splitActiveTab(axis: .horizontal) }
                .keyboardShortcut("d", modifiers: [.command])
            Button("向下分屏") { store.splitActiveTab(axis: .vertical) }
                .keyboardShortcut("d", modifiers: [.command, .shift])

            Divider()

            Button("下一个标签") { store.selectNextTab() }
                .keyboardShortcut("]", modifiers: [.command, .shift])
            Button("上一个标签") { store.selectPreviousTab() }
                .keyboardShortcut("[", modifiers: [.command, .shift])

            Divider()

            // 分屏焦点:⌘⌥ 方向键(对齐 Ghostty goto_split)
            Button("聚焦左侧分屏") { store.focusSplit(.left) }
                .keyboardShortcut(.leftArrow, modifiers: [.command, .option])
            Button("聚焦右侧分屏") { store.focusSplit(.right) }
                .keyboardShortcut(.rightArrow, modifiers: [.command, .option])
            Button("聚焦上方分屏") { store.focusSplit(.up) }
                .keyboardShortcut(.upArrow, modifiers: [.command, .option])
            Button("聚焦下方分屏") { store.focusSplit(.down) }
                .keyboardShortcut(.downArrow, modifiers: [.command, .option])

            Divider()

            // ⌘1…⌘8 直达,⌘9 = 最后一个标签(对齐 Ghostty)
            ForEach(1...8, id: \.self) { n in
                Button("第 \(n) 个标签") { store.selectTab(index: n - 1) }
                    .keyboardShortcut(KeyEquivalent(Character("\(n)")), modifiers: [.command])
            }
            Button("最后一个标签") { store.selectLastTab() }
                .keyboardShortcut("9", modifiers: [.command])
        }

        CommandMenu("显示") {
            Button("放大字号") { store.increaseFontSize() }
                .keyboardShortcut("+", modifiers: [.command])
            // ⌘= 别名(无需 Shift 也能放大,对齐 Ghostty)
            Button("放大字号(=)") { store.increaseFontSize() }
                .keyboardShortcut("=", modifiers: [.command])
            Button("缩小字号") { store.decreaseFontSize() }
                .keyboardShortcut("-", modifiers: [.command])
            Button("恢复默认字号") { store.resetFontSize() }
                .keyboardShortcut("0", modifiers: [.command])

            Divider()

            Button("清屏") { store.clearActiveScreen() }
                .keyboardShortcut("k", modifiers: [.command])
            Button("查找…") { store.searchVisible = true }
                .keyboardShortcut("f", modifiers: [.command])

            Divider()

            Menu("主题") {
                ForEach(Theme.all) { theme in
                    Button(theme.displayName) { store.setTheme(theme.id) }
                }
            }
        }
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        // SPM 可执行文件默认不是"前台 App",这里强制变成常规 App 并激活窗口
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)

        // 启动窗口:屏幕可见区域(排除菜单栏/程序坞)的 80%,居中
        DispatchQueue.main.async {
            guard let screen = NSScreen.main, let window = NSApp.windows.first else { return }
            let vf = screen.visibleFrame
            let w = vf.width * 0.8
            let h = vf.height * 0.8
            let x = vf.minX + (vf.width - w) / 2
            let y = vf.minY + (vf.height - h) / 2
            window.setFrame(NSRect(x: x, y: y, width: w, height: h), display: true)
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }
}
