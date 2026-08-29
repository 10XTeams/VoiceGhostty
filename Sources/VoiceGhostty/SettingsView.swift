import SwiftUI
import AppKit

/// Combined Settings + Shortcuts panel (popped up by the toolbar's gear button).
/// A segmented control switches between the editable settings and the read-only shortcut reference.
struct SettingsView: View {
    var onDone: () -> Void = {}

    @ObservedObject private var loc = Loc.shared
    @EnvironmentObject private var store: SessionStore

    private enum Tab: Hashable { case settings, shortcuts }

    @State private var tab: Tab = .settings
    @State private var language: String = AppSettings.defaultLanguage
    @State private var skillDirs: [String] = AppSettings.skillLibraryDirs
    @State private var inputColor: Color = Color(hex: AppSettings.inputColor)
    @State private var addsPunctuation: Bool = AppSettings.addsPunctuation
    @State private var scrollback: String = String(AppSettings.scrollback)
    @State private var saveHistory: Bool = AppSettings.historyEnabled
    @State private var doneSound: Bool = AppSettings.doneSoundEnabled
    /// Warning shown right after adding folders whose contents a folder already in the list provides
    @State private var addNotice: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Picker("", selection: $tab) {
                Text(loc("Settings", "设置")).tag(Tab.settings)
                Text(loc("Shortcuts", "快捷键")).tag(Tab.shortcuts)
            }
            .pickerStyle(.segmented)
            .labelsHidden()

            switch tab {
            case .settings:
                settingsTab
            case .shortcuts:
                ScrollView {
                    ShortcutsHelpView(showTitle: false)
                }
                .frame(height: 320)
            }
        }
        .padding(16)
        .frame(width: 640)
    }

    // MARK: - Settings tab

    private var settingsTab: some View {
        VStack(alignment: .leading, spacing: 12) {
            // No ScrollView here on purpose: inside the floating overlay it proposes a collapsed height
            // to its content and the panel shrinks to a sliver. The body is short enough to just fit.
            settingsBody

            HStack {
                Text(Self.versionLabel)
                    .font(.caption2).foregroundStyle(.tertiary)
                    .textSelection(.enabled)
                    .help(Self.versionDetail)
                Spacer()
                Button(loc("Done", "完成")) { onDone() }
                    .keyboardShortcut(.defaultAction)
            }
        }
    }

    /// The scrolling part of the settings tab. Rows pair up where the controls are narrow, and every
    /// long explanation lives behind the row's ⓘ instead of standing permanently between the rows.
    private var settingsBody: some View {
        VStack(alignment: .leading, spacing: 14) {
            // Voice
            VStack(alignment: .leading, spacing: 6) {
                sectionTitle(loc("VOICE", "语音"))
                HStack {
                    Text(loc("Default language", "默认语言"))
                    Picker("", selection: $language) {
                        ForEach(AppSettings.languageOptions, id: \.id) { opt in
                            Text(opt.label).tag(opt.id)
                        }
                    }
                    .labelsHidden()
                    .fixedSize()
                    .onChange(of: language) { loc.set($0) }

                    Spacer()

                    Toggle(loc("Add punctuation", "自动加标点"), isOn: $addsPunctuation)
                        .onChange(of: addsPunctuation) { AppSettings.addsPunctuation = $0 }
                    InfoTip(text: loc("Punctuates what you dictate — better for talking to Claude. Turn it off when dictating shell commands, where a trailing period is just something to delete. Applies to the next thing you say.",
                                      "给听写结果加上标点,对 Claude 说话时更好读。若主要用来听写 shell 命令,可关掉——句尾多个句号还得删。下一次说话即生效。"))
                }
                Text(loc("Primary speech-recognition locale, and the UI language. The other language is still tried as a fallback after you stop talking.",
                         "语音识别的主语言,同时也是界面语言。停止说话后仍会用另一种语言作后备识别。"))
                    .font(.caption2).foregroundStyle(.tertiary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            // Terminal
            VStack(alignment: .leading, spacing: 6) {
                sectionTitle(loc("TERMINAL", "终端"))
                HStack {
                    Text(loc("Command input color", "命令输入颜色"))
                    ColorPicker("", selection: $inputColor, supportsOpacity: false)
                        .labelsHidden()
                        .onChange(of: inputColor) { AppSettings.inputColor = $0.toHex() }
                    InfoTip(text: loc("Colors the command line you type at the zsh prompt. Changes apply to every open tab the moment you next type.",
                                      "给你在 zsh 提示符下输入的命令行整行上色。改颜色后,所有已打开标签页在你下次敲键时即刻生效。"))

                    Spacer()

                    Text(loc("Scrollback lines", "回滚行数"))
                    TextField("", text: $scrollback)
                        .labelsHidden()
                        .frame(width: 80)
                        .multilineTextAlignment(.trailing)
                        .onSubmit { commitScrollback() }
                    InfoTip(text: loc("Lines of scrolled-off output each pane keeps (\(AppSettings.scrollbackRange.lowerBound)–\(AppSettings.scrollbackRange.upperBound)). Press Return to apply — it takes effect in panes that are already open. Higher costs memory: roughly a couple of KB per line, per pane.",
                                      "每个分屏保留多少行滚出屏幕的输出(\(AppSettings.scrollbackRange.lowerBound)–\(AppSettings.scrollbackRange.upperBound))。按回车生效,已打开的分屏也会立即应用。调大更耗内存:每行约几 KB,且每个分屏各算一份。"))
                }

                HStack {
                    Toggle(loc("Chime on the green light", "转绿灯时提示音"), isOn: $doneSound)
                        .onChange(of: doneSound) {
                            AppSettings.doneSoundEnabled = $0
                            if $0 { DoneChime.preview() }
                        }
                    InfoTip(text: loc("Plays a sound when a pane finishes (command finished — your turn). With shell integration active it rings only for a finish the program announces — a bell, or the shell reporting the command ended — so an interactive app that just goes quiet for a moment turns its light green without making a sound, and a long task can't ring every few seconds while it works. Without shell integration a pause in the output is the only signal there is, so it rings on that. The pane you are typing in stays silent while VoiceGhostty is in front; send the app to the background and it reports its finish like any other pane. At most one chime every few seconds.",
                                      "分屏跑完(该你了)时响一声。启用了 shell 集成时,只认程序明确宣告的完成 —— 终端响铃,或 shell 报告命令结束 —— 所以交互式程序只是安静了一会儿的话,灯会转绿但不出声,免得一个长任务在干活期间每隔几秒响一次。没有 shell 集成时,输出停顿是唯一的信号,那就按停顿来响。VoiceGhostty 在前台时,你正在输入的那个分屏不会响;切到别的应用后,它和其他分屏一样会报告完成。每几秒最多响一次。"))
                    Spacer()
                }
            }

            // History
            VStack(alignment: .leading, spacing: 6) {
                sectionTitle(loc("HISTORY", "历史记录"))
                HStack(spacing: 6) {
                    Toggle(loc("Save every session to a file", "把每个会话保存成文件"), isOn: $saveHistory)
                        .onChange(of: saveHistory) { AppSettings.historyEnabled = $0 }
                    InfoTip(text: loc("One file per pane, named <folder>-<start time>.log. It holds the whole session — the scrollback limit above does not truncate it. Applies to panes opened from now on.",
                                      "每个分屏一个文件,命名为 <文件夹名>-<开始时间>.log。文件保存完整会话,不受上面回滚行数限制。改动对此后新开的分屏生效。"))

                    Spacer()

                    Image(systemName: "folder").foregroundStyle(.secondary)
                    Text((SessionLogger.directory as NSString).abbreviatingWithTildeInPath)
                        .font(.caption).foregroundStyle(.secondary)
                        .lineLimit(1).truncationMode(.middle)
                        .help(SessionLogger.directory)
                    Button(loc("Open", "打开")) { openHistoryFolder() }
                }
            }

            // Skills
            VStack(alignment: .leading, spacing: 6) {
                sectionTitle(loc("SKILLS & PLUGINS", "技能与插件"))
                HStack {
                    Text(loc("Loader folders", "加载器文件夹")).font(.callout)
                    InfoTip(text: loc("Folders the puzzle panel loads from. A folder carrying .claude-plugin/ (marketplace.json or plugin.json) is a plugin folder and contributes only its plugins — the skills and agents inside it load with the plugin, so they are not offered separately. Any other folder contributes skills (a folder with SKILL.md, directly here or under skills/) and agents (a .md file with YAML front matter, directly here or under agents/). Add as many folders as you like; the panel puts each kind in its own tab, one group per folder. A name offered by two folders is loaded from the first one in this list — the panel marks the other as a duplicate.",
                                      "拼图面板从这些文件夹加载。带 .claude-plugin/(marketplace.json 或 plugin.json)的是插件文件夹,只贡献插件 —— 它里面的技能和 Agent 随插件一起加载,不再单独列出。其余文件夹贡献技能(带 SKILL.md 的文件夹,可在本层或 skills/ 下)和 Agent(带 YAML front matter 的 .md 文件,可在本层或 agents/ 下)。可添加任意多个文件夹,面板按类型分成三个页签,每个页签里按文件夹分组。同名项以本列表中靠前的文件夹为准,其余会被标为重名。"))
                    Spacer()
                    Button(loc("Add…", "添加…")) { addSkillDirs() }
                }
                let summaries = Self.folderSummaries(skillDirs)
                ForEach(Array(skillDirs.enumerated()), id: \.element) { index, dir in
                    let summary = summaries[dir]
                    HStack(spacing: 6) {
                        Image(systemName: summary?.icon ?? "questionmark.folder")
                            .foregroundStyle(.secondary)
                        Text((dir as NSString).abbreviatingWithTildeInPath)
                            .font(.caption).foregroundStyle(.secondary)
                            .lineLimit(1).truncationMode(.middle)
                            .help(dir)
                        // What was found in it — right here, so what the 🧩 panel will show is known
                        // before you go looking for it there
                        Text(summary?.label ?? loc("does not exist", "不存在"))
                            .font(.caption2).foregroundStyle(.tertiary)
                        if let duplicates = summary?.duplicates, duplicates > 0 {
                            Text(loc("· \(duplicates) duplicate", "· \(duplicates) 项重名"))
                                .font(.caption2).foregroundStyle(.orange)
                                .help(loc("These names are already provided by a folder listed above, so they cannot be loaded from here. Remove the other folder to load them from this one.",
                                          "这些名字上面的文件夹已经提供了,无法再从这里加载。想改用这个文件夹,请移除上面那个。"))
                        }
                        Spacer()
                        Button {
                            removeSkillDir(at: index)
                        } label: {
                            Image(systemName: "minus.circle")
                        }
                        .buttonStyle(.plain)
                        .help(loc("Remove this folder", "移除这个文件夹"))
                    }
                }
                if skillDirs.isEmpty {
                    Text(loc("No folder added — the 🧩 panel will only list what the project already has.",
                             "没有添加文件夹 —— 🧩 面板只会列出项目里已加载的内容。"))
                        .font(.caption).foregroundStyle(.secondary)
                }
                if let notice = addNotice {
                    Text(notice)
                        .font(.caption).foregroundStyle(.orange)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
    }

    /// What one added folder contributes, as the row shows it
    private struct FolderSummary {
        let icon: String
        let label: String
        /// Items a folder listed *above* this one already provides — those cannot be loaded from here
        let duplicates: Int
    }

    /// Scan every added folder and count what it offers. Done for the whole list at once because
    /// "duplicate" is a fact about a folder's position in it: the first folder to offer a name owns it.
    private static func folderSummaries(_ dirs: [String]) -> [String: FolderSummary] {
        let loc = Loc.shared
        var ownership = LoaderOwnership()
        var result: [String: FolderSummary] = [:]
        for scan in LoaderLibrary.scanAll(dirs) {
            var duplicates = 0
            for plugin in scan.plugins where ownership.claim(kind: "plugin", key: plugin.name, by: scan.dir) != nil {
                duplicates += 1
            }
            for skill in scan.skills where ownership.claim(kind: "skill", key: skill.name, by: scan.dir) != nil {
                duplicates += 1
            }
            for agent in scan.agents where ownership.claim(kind: "agent", key: agent.fileName, by: scan.dir) != nil {
                duplicates += 1
            }
            var parts: [String] = []
            if !scan.plugins.isEmpty { parts.append("\(scan.plugins.count) " + loc("plugins", "插件")) }
            if !scan.skills.isEmpty  { parts.append("\(scan.skills.count) "  + loc("skills", "技能")) }
            if !scan.agents.isEmpty  { parts.append("\(scan.agents.count) "  + loc("agents", "Agent")) }
            result[scan.dir] = FolderSummary(icon: scan.plugins.isEmpty ? "folder" : "shippingbox",
                                             label: parts.isEmpty ? loc("nothing found", "未识别到内容")
                                                                  : parts.joined(separator: " · "),
                                             duplicates: duplicates)
        }
        return result
    }

    private func sectionTitle(_ text: String) -> some View {
        Text(text)
            .font(.caption).bold().foregroundStyle(.secondary)
    }

    // MARK: - Version

    /// Marketing version plus the commit it was built from — the commit is what actually tells you whether
    /// the running .app has a given fix in it, since the marketing version stays put across rebuilds.
    /// Empty when running outside an .app bundle (plain `swift run`), where there is no Info.plist to read.
    private static var versionLabel: String {
        guard let short = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String else { return "" }
        guard let commit = Bundle.main.infoDictionary?["VGGitCommit"] as? String, !commit.isEmpty else {
            return "v\(short)"
        }
        return "v\(short) (\(commit))"
    }

    /// Tooltip: adds the build timestamp, for telling two builds of the same commit apart.
    private static var versionDetail: String {
        let build = Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "-"
        return "\(versionLabel)  ·  build \(build)"
    }

    /// Parse + clamp the typed line count, apply it to every open pane, and echo the accepted value back
    /// into the field (so nonsense or an out-of-range number visibly snaps to what was actually stored).
    private func commitScrollback() {
        let value = AppSettings.clampScrollback(Int(scrollback.trimmingCharacters(in: .whitespaces)) ?? AppSettings.scrollback)
        AppSettings.scrollback = value
        store.setScrollback(value)
        scrollback = String(value)
    }

    private func openHistoryFolder() {
        let dir = SessionLogger.directory
        try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        NSWorkspace.shared.open(URL(fileURLWithPath: dir))
    }

    private func addSkillDirs() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = true
        panel.prompt = loc("Choose", "选择")
        panel.message = loc("Choose folders to load from — plugins, skills and agents inside are detected automatically",
                            "选择要加载的文件夹 —— 其中的插件、技能、Agent 会自动识别(可多选)")
        panel.directoryURL = skillDirs.last.map { URL(fileURLWithPath: $0) }
        guard panel.runModal() == .OK else { return }
        let picked = panel.urls.map(\.path).filter { !skillDirs.contains($0) }
        guard !picked.isEmpty else { return }
        skillDirs += picked
        AppSettings.skillLibraryDirs = skillDirs
        LoaderLibrary.invalidateCache()
        addNotice = Self.duplicateNotice(for: picked, in: skillDirs)
    }

    private func removeSkillDir(at index: Int) {
        guard skillDirs.indices.contains(index) else { return }
        skillDirs.remove(at: index)
        AppSettings.skillLibraryDirs = skillDirs
        LoaderLibrary.invalidateCache()
        addNotice = nil   // The list changed under it, so whatever it said may no longer be true
    }

    /// Said out loud the moment folders are added, rather than left for the user to find in the 🧩 panel:
    /// which of the new folders' items are already provided by a folder further up the list, and so will
    /// not be loadable from the new one.
    private static func duplicateNotice(for added: [String], in dirs: [String]) -> String? {
        let loc = Loc.shared
        let summaries = folderSummaries(dirs)
        let offenders = added.filter { (summaries[$0]?.duplicates ?? 0) > 0 }
        guard !offenders.isEmpty else { return nil }
        let total = offenders.reduce(0) { $0 + (summaries[$1]?.duplicates ?? 0) }
        let names = offenders.map { ($0 as NSString).lastPathComponent }.joined(separator: ", ")
        return loc("\(names): \(total) item(s) share a name with something a folder above already provides, and cannot be loaded from here.",
                   "\(names):有 \(total) 项与上面的文件夹重名,无法从这里加载。")
    }
}

/// The ⓘ that carries a setting's explanation: hover for the tooltip, click for a popover you can read
/// at your own pace. Keeping the prose in here is what lets the panel stay short.
private struct InfoTip: View {
    let text: String

    @State private var shown = false

    var body: some View {
        Button { shown.toggle() } label: {
            Image(systemName: "info.circle")
                .foregroundStyle(.tertiary)
        }
        .buttonStyle(.plain)
        .help(text)
        .popover(isPresented: $shown, arrowEdge: .bottom) {
            Text(text)
                .font(.callout)
                .fixedSize(horizontal: false, vertical: true)
                .frame(width: 300, alignment: .leading)
                .padding(12)
        }
    }
}
