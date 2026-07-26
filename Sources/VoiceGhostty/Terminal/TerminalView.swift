import SwiftUI
import SwiftTerm

/// Wraps a single SwiftTerm terminal view into SwiftUI.
///
/// `makeNSView` hands back the controller's permanent `hostContainer` instead of a fresh view: the
/// terminal is parented to that container once, at controller init, and never moves again. Nothing here
/// mutates the AppKit view tree, so SwiftUI is the only thing that ever restructures a pane — at the
/// times it expects to.
///
/// That is only sound because a pane's representable is never duplicated, which the two views below
/// guarantee: `TabContentView` renders a flat, session-keyed list (so splitting *moves* panes rather
/// than rebuilding them) and `ContentView` keeps every tab mounted (so switching tabs never tears one
/// pane subtree down while another is coming up).
struct TerminalHostView: NSViewRepresentable {
    let controller: TerminalController
    /// False for panes of a tab you are not looking at. They stay mounted — the shell runs, the status
    /// light updates, the transcript is written — but AppKit skips drawing them.
    let isVisible: Bool
    /// True for the one pane that should own the keyboard: the active pane of the visible tab.
    let isFocused: Bool

    func makeNSView(context: Context) -> NSView {
        let container = controller.hostContainer
        container.isHidden = !isVisible
        // First insertion into the window: hand the terminal keyboard focus (SessionStore drives it afterwards)
        let view = controller.terminalView
        DispatchQueue.main.async { view.window?.makeFirstResponder(view) }
        return container
    }

    /// Visibility is the only thing left to push into AppKit, and it touches no view relationships.
    func updateNSView(_ container: NSView, context: Context) {
        let wasHidden = container.isHidden
        if wasHidden == isVisible { container.isHidden = !isVisible }
        // Re-claim the keyboard for a pane that just came on screen. `SessionStore.focusActive()` runs at
        // the moment the tab changes — before SwiftUI has applied this update — so at that point the
        // incoming terminal is still hidden and cannot meaningfully take first responder.
        if wasHidden && isVisible && isFocused {
            let view = controller.terminalView
            DispatchQueue.main.async { view.window?.makeFirstResponder(view) }
        }
    }
}

/// A single split pane. Focus is distinguished by dimming the inactive pane; no border is drawn.
private struct PaneView: View {
    @ObservedObject var session: TerminalSession
    @ObservedObject private var loc = Loc.shared
    let isActive: Bool
    let isSplit: Bool
    let isVisible: Bool

    var body: some View {
        TerminalHostView(controller: session.controller,
                         isVisible: isVisible,
                         isFocused: isVisible && isActive)
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

/// Places a tab's panes according to its split tree.
///
/// Deliberately a `Layout` with an empty cache: the tree is plain data, the pane list is flat, and every
/// pass re-reads the tree from scratch. Nothing is carried between passes, so nothing can go stale.
///
/// The nested `HStack`/`VStack` version this replaces mirrored the tree in the *view* hierarchy, so every
/// split, close or collapse restructured the stacks — and SwiftUI's `StackLayout` cache, which holds
/// references to the children it laid out, could be left pointing at views that had already gone away.
/// That is the EXC_BAD_ACCESS in `StackLayout.makeChildren` this app kept dying from.
private struct SplitLayout: Layout {
    let root: SplitNode
    let spacing: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        proposal.replacingUnspecifiedDimensions(by: CGSize(width: 800, height: 400))
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let frames = root.paneFrames(in: CGRect(origin: .zero, size: bounds.size), spacing: spacing)
        for (index, subview) in subviews.enumerated() {
            // Subview i is leaf i — see the ordering contract on paneFrames. Counts disagreeing can only
            // mean the tree and the ForEach are one update apart; park the extra rather than read past the end.
            guard index < frames.count else {
                subview.place(at: bounds.origin, anchor: .topLeading,
                              proposal: ProposedViewSize(width: 0, height: 0))
                continue
            }
            let frame = frames[index]
            subview.place(at: CGPoint(x: bounds.minX + frame.minX, y: bounds.minY + frame.minY),
                          anchor: .topLeading,
                          proposal: ProposedViewSize(width: frame.width, height: frame.height))
        }
    }
}

/// The content of a tab: all of its panes in one flat list keyed by session id, positioned by the split tree.
///
/// Because the key is the session — not the shape of the tree — a pane keeps its view identity (and with
/// it its terminal view, scrollback and first-responder status) through every split, close and collapse.
struct TabContentView: View {
    @ObservedObject var tab: TerminalTab
    let isVisible: Bool

    /// Gap between panes; the backdrop shows through it as the separator line.
    private let separator: CGFloat = 1

    var body: some View {
        SplitLayout(root: tab.root, spacing: separator) {
            ForEach(tab.panes) { pane in
                PaneView(session: pane,
                         isActive: pane.id == tab.activePaneID,
                         isSplit: tab.isSplit,
                         isVisible: isVisible)
            }
        }
        .background(Color.secondary.opacity(0.4))
        // A hidden tab's terminals are already hidden at the AppKit level; this covers the SwiftUI
        // overlays (status dots, "process exited") that would otherwise paint over the active tab.
        .opacity(isVisible ? 1 : 0)
        .allowsHitTesting(isVisible)
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
                    TabChipView(tab: tab,
                                store: store,
                                index: index,
                                editingTabID: $editingTabID,
                                editingText: $editingText,
                                renameFocused: $renameFocused)
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
}

/// One chip in the tab bar.
///
/// Its own view, observing its own tab, so the status poll can wake exactly this chip (see
/// `SessionStore.tickStatus`) instead of the store — waking the store would re-run the whole window body,
/// terminal hosts included, twice a second.
private struct TabChipView: View {
    @ObservedObject var tab: TerminalTab
    @ObservedObject var store: SessionStore
    @ObservedObject private var loc = Loc.shared
    let index: Int
    @Binding var editingTabID: UUID?
    @Binding var editingText: String
    var renameFocused: FocusState<Bool>.Binding

    private var isActive: Bool { tab.id == store.activeTabID }
    private var isEditing: Bool { editingTabID == tab.id }

    var body: some View {
        HStack(spacing: 6) {
            // Status dot: gray = normal / yellow = busy / green = done and awaiting input
            Circle()
                .fill(tab.aggregateStatus.dotColor)
                .frame(width: 9, height: 9)
            if tab.isSplit {
                // Pane count rather than a fixed 2-up glyph — a tab can hold any number of splits
                HStack(spacing: 2) {
                    Image(systemName: "rectangle.split.2x1")
                    Text("\(tab.panes.count)")
                }
                .font(.caption)
                .foregroundStyle(.secondary)
            }
            Text("\(index + 1).").font(.system(size: 13)).foregroundStyle(.secondary)

            if isEditing {
                TextField(loc("Tab name", "标签页名称"), text: $editingText)
                    .textFieldStyle(.plain)
                    .font(.system(size: 13))
                    .frame(width: 140)
                    .focused(renameFocused)
                    .onSubmit { commitRename() }
                    .onExitCommand { cancelRename() }              // Esc cancels
                    .onChange(of: renameFocused.wrappedValue) { focused in   // Commit on losing focus
                        if !focused && editingTabID == tab.id { commitRename() }
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
        .onTapGesture(count: 2) { if !isEditing { startRename() } }
        .onTapGesture { if !isEditing { store.selectTab(index: index) } }
        .contextMenu {
            Button(loc("Rename", "重命名")) { startRename() }
            if tab.customTitle != nil {
                Button(loc("Restore process title", "恢复进程标题")) { tab.customTitle = nil }
            }
        }
    }

    // MARK: - Rename

    private func startRename() {
        if store.activeTabID != tab.id { store.activeTabID = tab.id }
        editingText = tab.customTitle ?? tab.displayTitle
        editingTabID = tab.id
        DispatchQueue.main.async { renameFocused.wrappedValue = true }
    }

    private func commitRename() {
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
