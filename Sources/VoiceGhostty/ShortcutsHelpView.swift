import SwiftUI

/// 快捷键说明面板(工具栏「快捷键」按钮弹出)。
struct ShortcutsHelpView: View {
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

    private let sections: [Section] = [
        Section(title: "语音", items: [
            Item(keys: "按住空格", desc: "说话(松开即停;快速点按仍输入空格)"),
            Item(keys: "⌘⇧M", desc: "开始/停止录音"),
            Item(keys: "回车", desc: "执行待确认区命令"),
        ]),
        Section(title: "标签", items: [
            Item(keys: "⌘T", desc: "新建标签"),
            Item(keys: "⌘W", desc: "关闭标签/分屏"),
            Item(keys: "⌘⇧] / ⌘⇧[", desc: "下一个 / 上一个标签"),
            Item(keys: "⌘1…⌘8", desc: "直达第 N 个标签"),
            Item(keys: "⌘9", desc: "最后一个标签"),
        ]),
        Section(title: "分屏", items: [
            Item(keys: "⌘D", desc: "向右分屏"),
            Item(keys: "⌘⇧D", desc: "向下分屏"),
            Item(keys: "⌘⌥←↑↓→", desc: "按方向切换分屏焦点"),
            Item(keys: "点击某格", desc: "语音/执行路由到该格"),
        ]),
        Section(title: "显示", items: [
            Item(keys: "⌘+ / ⌘=", desc: "放大字号"),
            Item(keys: "⌘-", desc: "缩小字号"),
            Item(keys: "⌘0", desc: "恢复默认字号"),
            Item(keys: "⌘K", desc: "清屏"),
            Item(keys: "⌘F", desc: "搜索滚动区(回车下一个,⇧回车上一个)"),
        ]),
        Section(title: "编辑", items: [
            Item(keys: "⌘C / ⌘V", desc: "复制 / 粘贴"),
            Item(keys: "⌘A", desc: "全选"),
        ]),
    ]

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("快捷键")
                .font(.headline)

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
        .padding(16)
        .frame(width: 340, alignment: .leading)
    }
}
