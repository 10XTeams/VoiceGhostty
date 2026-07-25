import SwiftUI
import SwiftTerm

/// Wraps a single SwiftTerm terminal view into SwiftUI.
struct TerminalHostView: NSViewRepresentable {
    let controller: TerminalController

    func makeNSView(context: Context) -> FocusReportingTerminalView {
        let view = controller.terminalView
        DispatchQueue.main.async { view.window?.makeFirstResponder(view) }
        return view
    }

    func updateNSView(_ nsView: FocusReportingTerminalView, context: Context) {}
}

/// A single split pane. Focus is distinguished by dimming the inactive pane; no border is drawn.
private struct PaneView: View {
    @ObservedObject var session: TerminalSession
    @ObservedObject private var loc = Loc.shared
    let isActive: Bool
    let isSplit: Bool

    var body: some View {
        TerminalHostView(controller: session.controller)
            // Dim the inactive split pane (overlay a translucent black layer, so text darkens with it)
            .overlay {
                if isSplit && !isActive {
                    Color.black.opacity(0.35).allowsHitTesting(false)
                }
            }
            // When split, show a status dot in the top-right corner of every pane (yellow = running, green = done);
            // hidden in the normal state. Both panes show their own light so you can tell which is busy vs finished.
            .overlay(alignment: .topTrailing) {
                if isSplit && session.status != .normal {
                    Circle()
                        .fill(session.status.dotColor)
                        .frame(width: 8, height: 8)
                        .padding(6)
                }
            }
            .overlay(alignment: .topLeading) {
                if session.isTerminated {
                    Text(loc("Process exited", "进程已退出"))
                        .font(.caption2).padding(.horizontal, 6).padding(.vertical, 2)
                        .background(.thinMaterial, in: Capsule())
                        .padding(6)
                }
            }
    }
}

/// The content of a tab: 1 pane fills the space, 2 panes split along the axis.
struct TabContentView: View {
    @ObservedObject var tab: TerminalTab

    var body: some View {
        Group {
            if tab.panes.count == 1 {
                PaneView(session: tab.panes[0], isActive: true, isSplit: false)
            } else {
                let layout = tab.splitAxis == .horizontal
                    ? AnyLayout(HStackLayout(spacing: 1))
                    : AnyLayout(VStackLayout(spacing: 1))
                layout {
                    ForEach(tab.panes) { pane in
                        PaneView(session: pane,
                                 isActive: pane.id == tab.activePaneID,
                                 isSplit: true)
                    }
                }
                .background(Color.secondary.opacity(0.4))  // Separator line
            }
        }
    }
}

/// The top tab bar. Double-click a tab name to rename it; the tab-switching shortcuts (⌘1-9, etc.) are unchanged.
struct TabBarView: View {
    @ObservedObject var store: SessionStore
    @ObservedObject private var loc = Loc.shared
    /// The tab being renamed; nil means no editing
    @State private var editingTabID: UUID?
    @State private var editingText = ""
    @FocusState private var renameFocused: Bool

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 4) {
                ForEach(Array(store.tabs.enumerated()), id: \.element.id) { index, tab in
                    tabChip(tab, index: index)
                }
                Button {
                    store.addTab()
                } label: {
                    Image(systemName: "plus").frame(width: 28, height: 28)
                }
                .buttonStyle(.plain)
                .help(loc("New Tab (⌘T)", "新建标签页(⌘T)"))
            }
            .padding(.horizontal, 6)
            .padding(.vertical, 5)
        }
        .background(.bar)
    }

    private func tabChip(_ tab: TerminalTab, index: Int) -> some View {
        let isActive = tab.id == store.activeTabID
        let isEditing = editingTabID == tab.id
        return HStack(spacing: 6) {
            // Status dot: gray = normal / yellow = busy / green = done and awaiting input
            Circle()
                .fill(tab.aggregateStatus.dotColor)
                .frame(width: 9, height: 9)
            if tab.isSplit { Image(systemName: "rectangle.split.2x1").font(.caption) }
            Text("\(index + 1).").font(.system(size: 13)).foregroundStyle(.secondary)

            if isEditing {
                TextField(loc("Tab name", "标签页名称"), text: $editingText)
                    .textFieldStyle(.plain)
                    .font(.system(size: 13))
                    .frame(width: 140)
                    .focused($renameFocused)
                    .onSubmit { commitRename(tab) }
                    .onExitCommand { cancelRename() }        // Esc cancels
                    .onChange(of: renameFocused) { focused in  // Commit on losing focus
                        if !focused && editingTabID == tab.id { commitRename(tab) }
                    }
            } else {
                Text(tab.displayTitle)
                    .font(.system(size: 13))
                    .lineLimit(1)
                    .frame(maxWidth: 180)
            }

            Button {
                if store.activeTabID != tab.id { store.activeTabID = tab.id }
                store.closeActivePaneOrTab()
            } label: {
                Image(systemName: "xmark")
                    .font(.system(size: 10, weight: .bold))
                    .frame(width: 18, height: 18)   // Enlarge the close button's hit area
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .opacity(0.6)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(isActive ? Color.accentColor.opacity(0.25) : Color.secondary.opacity(0.12),
                    in: RoundedRectangle(cornerRadius: 7))
        .contentShape(Rectangle())
        // The gesture is attached to the whole chip: tapping the status dot/number/blank area also switches. Check double-tap (rename) first, then single-tap (switch)
        .onTapGesture(count: 2) { if !isEditing { startRename(tab) } }
        .onTapGesture { if !isEditing { store.selectTab(index: index) } }
        .contextMenu {
            Button(loc("Rename", "重命名")) { startRename(tab) }
            if tab.customTitle != nil {
                Button(loc("Restore process title", "恢复进程标题")) { tab.customTitle = nil }
            }
        }
    }

    // MARK: - Rename

    private func startRename(_ tab: TerminalTab) {
        if store.activeTabID != tab.id { store.activeTabID = tab.id }
        editingText = tab.customTitle ?? tab.displayTitle
        editingTabID = tab.id
        DispatchQueue.main.async { renameFocused = true }
    }

    private func commitRename(_ tab: TerminalTab) {
        let trimmed = editingText.trimmingCharacters(in: .whitespaces)
        tab.customTitle = trimmed.isEmpty ? nil : trimmed   // Empty = fall back to the process title
        editingTabID = nil
        store.focusActive()                                  // Return keyboard focus to the terminal
    }

    private func cancelRename() {
        editingTabID = nil
        store.focusActive()
    }
}
