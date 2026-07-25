import SwiftUI

/// Shortcuts reference list, embedded in the Settings panel's "Shortcuts" tab.
struct ShortcutsHelpView: View {
    /// When embedded in the Settings panel the tab already labels this section, so the title is hidden.
    var showTitle: Bool = true

    @ObservedObject private var loc = Loc.shared

    private struct Item: Identifiable {
        let id = UUID()
        let keys: String
        let desc: String
    }
    private struct Section: Identifiable {
        let id = UUID()
        let title: String
        let items: [Item]
    }

    private var sections: [Section] {
        [
            Section(title: loc("Voice", "语音"), items: [
                Item(keys: "Hold Space", desc: loc("Talk (release to stop; a quick tap still types a space)",
                                                   "按住说话(松开停止;轻按一下仍输入空格)")),
                Item(keys: "⌘⇧M", desc: loc("Start / stop recording", "开始 / 停止录音")),
                Item(keys: "Return", desc: loc("Run the pending command", "执行待确认的命令")),
            ]),
            Section(title: loc("Tabs", "标签页"), items: [
                Item(keys: "⌘T", desc: loc("New tab", "新建标签页")),
                Item(keys: "⌘W", desc: loc("Close tab / split", "关闭标签页 / 分屏")),
                Item(keys: "⌘⇧] / ⌘⇧[", desc: loc("Next / previous tab", "下一个 / 上一个标签页")),
                Item(keys: "⌘1…⌘8", desc: loc("Jump to the Nth tab", "跳到第 N 个标签页")),
                Item(keys: "⌘9", desc: loc("Last tab", "最后一个标签页")),
            ]),
            Section(title: loc("Splits", "分屏"), items: [
                Item(keys: "⌘D", desc: loc("Split right", "向右分屏")),
                Item(keys: "⌘⇧D", desc: loc("Split down", "向下分屏")),
                Item(keys: "⌘⌥←↑↓→", desc: loc("Move split focus by direction", "按方向切换分屏焦点")),
                Item(keys: loc("Click a pane", "点击某个分屏"), desc: loc("Route voice / execution to that pane",
                                                                        "把语音 / 执行定向到该分屏")),
            ]),
            Section(title: loc("View", "视图"), items: [
                Item(keys: "⌘+ / ⌘=", desc: loc("Increase font size", "增大字号")),
                Item(keys: "⌘-", desc: loc("Decrease font size", "减小字号")),
                Item(keys: "⌘0", desc: loc("Reset font size", "重置字号")),
                Item(keys: "⌘K", desc: loc("Clear screen", "清屏")),
                Item(keys: "⌘F", desc: loc("Search scrollback (Return next, ⇧Return previous)",
                                           "搜索回滚内容(Return 下一个,⇧Return 上一个)")),
            ]),
            Section(title: loc("Edit", "编辑"), items: [
                Item(keys: "⌘C / ⌘V", desc: loc("Copy / paste", "复制 / 粘贴")),
                Item(keys: "⌘A", desc: loc("Select all", "全选")),
            ]),
        ]
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            if showTitle {
                Text(loc("Shortcuts", "快捷键"))
                    .font(.headline)
            }

            ForEach(sections) { section in
                VStack(alignment: .leading, spacing: 6) {
                    Text(section.title)
                        .font(.caption).bold()
                        .foregroundStyle(.secondary)
                    ForEach(section.items) { item in
                        HStack(alignment: .firstTextBaseline, spacing: 10) {
                            Text(item.keys)
                                .font(.system(.caption, design: .monospaced))
                                .frame(width: 96, alignment: .leading)
                                .foregroundStyle(.primary)
                            Text(item.desc)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}
