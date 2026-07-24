import SwiftUI
import SwiftTerm

/// 把单个 SwiftTerm 终端视图包进 SwiftUI。
struct TerminalHostView: NSViewRepresentable {
    let controller: TerminalController

    func makeNSView(context: Context) -> FocusReportingTerminalView {
        let view = controller.terminalView
        DispatchQueue.main.async { view.window?.makeFirstResponder(view) }
        return view
    }

    func updateNSView(_ nsView: FocusReportingTerminalView, context: Context) {}
}

/// 一个分屏格。焦点靠「非活动格压暗」区分,不画任何边框。
private struct PaneView: View {
    @ObservedObject var session: TerminalSession
    let isActive: Bool
    let isSplit: Bool

    var body: some View {
        TerminalHostView(controller: session.controller)
            // 非活动分屏格压暗(叠一层半透明黑,文字随之变暗)
            .overlay {
                if isSplit && !isActive {
                    Color.black.opacity(0.35).allowsHitTesting(false)
                }
            }
            // 分屏时,非活动格右上角状态点(常态不显示)
            .overlay(alignment: .topTrailing) {
                if isSplit && !isActive && session.status != .normal {
                    Circle()
                        .fill(session.status.dotColor)
                        .frame(width: 8, height: 8)
                        .padding(6)
                }
            }
            .overlay(alignment: .topLeading) {
                if session.isTerminated {
                    Text("进程已退出")
                        .font(.caption2).padding(.horizontal, 6).padding(.vertical, 2)
                        .background(.thinMaterial, in: Capsule())
                        .padding(6)
                }
            }
    }
}

/// 一个标签的内容:1 格铺满,2 格按轴向分屏。
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
                .background(Color.secondary.opacity(0.4))  // 分隔线
            }
        }
    }
}

/// 顶部标签栏。双击标签名可重命名;切换标签的快捷键(⌘1-9 等)不变。
struct TabBarView: View {
    @ObservedObject var store: SessionStore
    /// 正在重命名的标签;nil 表示无编辑
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
                .help("新建标签 (⌘T)")
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
            // 状态点:灰=常态 / 黄=忙碌 / 绿=完成待输入
            Circle()
                .fill(tab.aggregateStatus.dotColor)
                .frame(width: 9, height: 9)
            if tab.isSplit { Image(systemName: "rectangle.split.2x1").font(.caption) }
            Text("\(index + 1).").font(.system(size: 13)).foregroundStyle(.secondary)

            if isEditing {
                TextField("标签名", text: $editingText)
                    .textFieldStyle(.plain)
                    .font(.system(size: 13))
                    .frame(width: 140)
                    .focused($renameFocused)
                    .onSubmit { commitRename(tab) }
                    .onExitCommand { cancelRename() }        // Esc 取消
                    .onChange(of: renameFocused) { focused in  // 失焦即提交
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
                    .frame(width: 18, height: 18)   // 扩大关闭按钮命中区
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
        // 手势挂在整块 chip 上:点状态点/编号/空白处也能切换。先判双击(重命名),再判单击(切换)
        .onTapGesture(count: 2) { if !isEditing { startRename(tab) } }
        .onTapGesture { if !isEditing { store.selectTab(index: index) } }
        .contextMenu {
            Button("重命名") { startRename(tab) }
            if tab.customTitle != nil {
                Button("恢复进程标题") { tab.customTitle = nil }
            }
        }
    }

    // MARK: - 重命名

    private func startRename(_ tab: TerminalTab) {
        if store.activeTabID != tab.id { store.activeTabID = tab.id }
        editingText = tab.customTitle ?? tab.displayTitle
        editingTabID = tab.id
        DispatchQueue.main.async { renameFocused = true }
    }

    private func commitRename(_ tab: TerminalTab) {
        let trimmed = editingText.trimmingCharacters(in: .whitespaces)
        tab.customTitle = trimmed.isEmpty ? nil : trimmed   // 清空 = 回退进程标题
        editingTabID = nil
        store.focusActive()                                  // 键盘焦点交回终端
    }

    private func cancelRename() {
        editingTabID = nil
        store.focusActive()
    }
}
