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

/// Menu bar commands — the keyboard shortcuts for tabs / splits / font size / theme / search all live here.
struct AppCommands: Commands {
    @ObservedObject var store: SessionStore

    var body: some Commands {
        // Copy/paste/select-all go through the system default "Edit" menu (SwiftTerm already implements the responder-chain methods)

        // ★ ⌘W must replace the system "File > Close": when two menu items collide on a shortcut, AppKit picks the
        // one earlier in menu order; "File" comes before "Terminal", so the system Close would steal ⌘W and close the window, quitting the app.
        CommandGroup(replacing: .saveItem) {
            Button("Close Tab / Split") { store.closeActivePaneOrTab() }
                .keyboardShortcut("w", modifiers: [.command])
        }

        CommandMenu("Terminal") {
            Button("New Tab") { store.addTab() }
                .keyboardShortcut("t", modifiers: [.command])
            Button("Split Right") { store.splitActiveTab(axis: .horizontal) }
                .keyboardShortcut("d", modifiers: [.command])
            Button("Split Down") { store.splitActiveTab(axis: .vertical) }
                .keyboardShortcut("d", modifiers: [.command, .shift])

            Divider()

            Button("Next Tab") { store.selectNextTab() }
                .keyboardShortcut("]", modifiers: [.command, .shift])
            Button("Previous Tab") { store.selectPreviousTab() }
                .keyboardShortcut("[", modifiers: [.command, .shift])

            Divider()

            // Split focus: ⌘⌥ arrow keys (matches Ghostty goto_split)
            Button("Focus Split Left") { store.focusSplit(.left) }
                .keyboardShortcut(.leftArrow, modifiers: [.command, .option])
            Button("Focus Split Right") { store.focusSplit(.right) }
                .keyboardShortcut(.rightArrow, modifiers: [.command, .option])
            Button("Focus Split Up") { store.focusSplit(.up) }
                .keyboardShortcut(.upArrow, modifiers: [.command, .option])
            Button("Focus Split Down") { store.focusSplit(.down) }
                .keyboardShortcut(.downArrow, modifiers: [.command, .option])

            Divider()

            // Move the active pane to another tab: ⌘⌃ + the tab's own ⌘N number, so "go to tab 2" and
            // "send this split to tab 2" differ by one modifier. The list is live, and names the tabs —
            // the shortcuts alone would never tell you which tab is which.
            // Eight fixed slots, enabled/disabled against the current tabs — never a list built from
            // `store.tabs`. AppKit picks up changed key equivalents only when the menu is next opened, so a
            // list that grows with the tabs leaves ⌘⌃N dead on a newly created tab until you pull the menu
            // down once. See `SessionStore.moveTargetLabel`.
            Menu("Move Split to Tab") {
                ForEach(1...8, id: \.self) { n in
                    Button(store.moveTargetLabel(n - 1)) { store.moveActivePane(toTabIndex: n - 1) }
                        .keyboardShortcut(KeyEquivalent(Character("\(n)")), modifiers: [.command, .control])
                        .disabled(!store.canMoveActivePane(toTabIndex: n - 1))
                }
            }
            Button("Move Split to New Tab") { store.moveActivePaneToNewTab() }
                .keyboardShortcut("t", modifiers: [.command, .control])
            Button("Move Split to Next Tab") { store.moveActivePane(toAdjacentTab: +1) }
                .keyboardShortcut(.rightArrow, modifiers: [.command, .control])
            Button("Move Split to Previous Tab") { store.moveActivePane(toAdjacentTab: -1) }
                .keyboardShortcut(.leftArrow, modifiers: [.command, .control])

            Divider()

            // ⌘1…⌘8 jump directly, ⌘9 = last tab (matches Ghostty)
            ForEach(1...8, id: \.self) { n in
                Button("Tab \(n)") { store.selectTab(index: n - 1) }
                    .keyboardShortcut(KeyEquivalent(Character("\(n)")), modifiers: [.command])
            }
            Button("Last Tab") { store.selectLastTab() }
                .keyboardShortcut("9", modifiers: [.command])
        }

        CommandMenu("View") {
            Button("Increase Font Size") { store.increaseFontSize() }
                .keyboardShortcut("+", modifiers: [.command])
            // ⌘= alias (zoom in without needing Shift, matches Ghostty)
            Button("Increase Font Size (=)") { store.increaseFontSize() }
                .keyboardShortcut("=", modifiers: [.command])
            Button("Decrease Font Size") { store.decreaseFontSize() }
                .keyboardShortcut("-", modifiers: [.command])
            Button("Reset Font Size") { store.resetFontSize() }
                .keyboardShortcut("0", modifiers: [.command])

            Divider()

            Button("Clear Screen") { store.clearActiveScreen() }
                .keyboardShortcut("k", modifiers: [.command])
            Button("Find…") { store.searchVisible = true }
                .keyboardShortcut("f", modifiers: [.command])

            Divider()

            Menu("Theme") {
                ForEach(Theme.all) { theme in
                    Button(theme.displayName) { store.setTheme(theme.id) }
                }
            }
        }
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        // An SPM executable is not a "foreground app" by default; force it to a regular app and activate the window
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)

        // Startup window: 80% of the screen's visible area (excluding menu bar/Dock), centered
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
