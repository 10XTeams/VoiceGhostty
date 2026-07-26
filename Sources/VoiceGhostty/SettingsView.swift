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
    @State private var skillDir: String = AppSettings.skillLibraryDir
    @State private var inputColor: Color = Color(hex: AppSettings.inputColor)
    @State private var addsPunctuation: Bool = AppSettings.addsPunctuation
    @State private var scrollback: String = String(AppSettings.scrollback)
    @State private var saveHistory: Bool = AppSettings.historyEnabled
    @State private var doneSound: Bool = AppSettings.doneSoundEnabled

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
        .frame(width: 320)
    }

    // MARK: - Settings tab

    private var settingsTab: some View {
        VStack(alignment: .leading, spacing: 16) {
            // Voice
            VStack(alignment: .leading, spacing: 6) {
                Text(loc("VOICE", "语音"))
                    .font(.caption).bold().foregroundStyle(.secondary)
                HStack {
                    Text(loc("Default language", "默认语言"))
                    Spacer()
                    Picker("", selection: $language) {
                        ForEach(AppSettings.languageOptions, id: \.id) { opt in
                            Text(opt.label).tag(opt.id)
                        }
                    }
                    .labelsHidden()
                    .fixedSize()
                    .onChange(of: language) { loc.set($0) }
                }
                Text(loc("Primary speech-recognition locale, and the UI language. The other language is still tried as a fallback after you stop talking.",
                         "语音识别的主语言,同时也是界面语言。停止说话后仍会用另一种语言作后备识别。"))
                    .font(.caption2).foregroundStyle(.tertiary)
                    .fixedSize(horizontal: false, vertical: true)

                Toggle(loc("Add punctuation", "自动加标点"), isOn: $addsPunctuation)
                    .onChange(of: addsPunctuation) { AppSettings.addsPunctuation = $0 }
                Text(loc("Punctuates what you dictate — better for talking to Claude. Turn it off when dictating shell commands, where a trailing period is just something to delete. Applies to the next thing you say.",
                         "给听写结果加上标点,对 Claude 说话时更好读。若主要用来听写 shell 命令,可关掉——句尾多个句号还得删。下一次说话即生效。"))
                    .font(.caption2).foregroundStyle(.tertiary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            Divider()

            // Terminal
            VStack(alignment: .leading, spacing: 6) {
                Text(loc("TERMINAL", "终端"))
                    .font(.caption).bold().foregroundStyle(.secondary)
                HStack {
                    Text(loc("Command input color", "命令输入颜色"))
                    Spacer()
                    ColorPicker("", selection: $inputColor, supportsOpacity: false)
                        .labelsHidden()
                        .onChange(of: inputColor) { AppSettings.inputColor = $0.toHex() }
                }
                Text(loc("Colors the command line you type at the zsh prompt. Changes apply to every open tab the moment you next type.",
                         "给你在 zsh 提示符下输入的命令行整行上色。改颜色后,所有已打开标签页在你下次敲键时即刻生效。"))
                    .font(.caption2).foregroundStyle(.tertiary)
                    .fixedSize(horizontal: false, vertical: true)

                HStack {
                    Text(loc("Scrollback lines", "回滚行数"))
                    Spacer()
                    TextField("", text: $scrollback)
                        .labelsHidden()
                        .frame(width: 80)
                        .multilineTextAlignment(.trailing)
                        .onSubmit { commitScrollback() }
                }
                Text(loc("Lines of scrolled-off output each pane keeps (\(AppSettings.scrollbackRange.lowerBound)–\(AppSettings.scrollbackRange.upperBound)). Press Return to apply — it takes effect in panes that are already open. Higher costs memory: roughly a couple of KB per line, per pane.",
                         "每个分屏保留多少行滚出屏幕的输出(\(AppSettings.scrollbackRange.lowerBound)–\(AppSettings.scrollbackRange.upperBound))。按回车生效,已打开的分屏也会立即应用。调大更耗内存:每行约几 KB,且每个分屏各算一份。"))
                    .font(.caption2).foregroundStyle(.tertiary)
                    .fixedSize(horizontal: false, vertical: true)

                Toggle(loc("Chime on the green light", "转绿灯时提示音"), isOn: $doneSound)
                    .onChange(of: doneSound) {
                        AppSettings.doneSoundEnabled = $0
                        if $0 { DoneChime.preview() }
                    }
                Text(loc("Plays a sound the moment a pane's status dot turns green (command finished — your turn). The pane you are typing in stays silent while VoiceGhostty is in front; send the app to the background and it reports its finish like any other pane. At most one chime every few seconds.",
                         "分屏的状态灯转绿(命令跑完,该你了)时响一声。VoiceGhostty 在前台时,你正在输入的那个分屏不会响;切到别的应用后,它和其他分屏一样会报告完成。每几秒最多响一次。"))
                    .font(.caption2).foregroundStyle(.tertiary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            Divider()

            // History
            VStack(alignment: .leading, spacing: 6) {
                Text(loc("HISTORY", "历史记录"))
                    .font(.caption).bold().foregroundStyle(.secondary)
                Toggle(loc("Save every session to a file", "把每个会话保存成文件"), isOn: $saveHistory)
                    .onChange(of: saveHistory) { AppSettings.historyEnabled = $0 }
                HStack(spacing: 6) {
                    Image(systemName: "folder").foregroundStyle(.secondary)
                    Text((SessionLogger.directory as NSString).abbreviatingWithTildeInPath)
                        .font(.caption).foregroundStyle(.secondary)
                        .lineLimit(1).truncationMode(.middle)
                    Spacer()
                    Button(loc("Open", "打开")) { openHistoryFolder() }
                }
                Text(loc("One file per pane, named <folder>-<start time>.log. It holds the whole session — the scrollback limit above does not truncate it. Applies to panes opened from now on.",
                         "每个分屏一个文件,命名为 <文件夹名>-<开始时间>.log。文件保存完整会话,不受上面回滚行数限制。改动对此后新开的分屏生效。"))
                    .font(.caption2).foregroundStyle(.tertiary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            Divider()

            // Skills
            VStack(alignment: .leading, spacing: 6) {
                Text(loc("SKILLS", "技能"))
                    .font(.caption).bold().foregroundStyle(.secondary)
                Text(loc("Default skill library folder", "默认技能库文件夹")).font(.callout)
                HStack(spacing: 6) {
                    Image(systemName: "folder").foregroundStyle(.secondary)
                    Text((skillDir as NSString).abbreviatingWithTildeInPath)
                        .font(.caption).foregroundStyle(.secondary)
                        .lineLimit(1).truncationMode(.middle)
                        .help(skillDir)
                    Spacer()
                    Button(loc("Choose…", "选择…")) { chooseSkillDir() }
                }
                Text(loc("Subfolders here are the skills the puzzle panel lists for syncing into a project.",
                         "该文件夹下的每个子文件夹会作为技能,显示在拼图面板中,可同步到项目里。"))
                    .font(.caption2).foregroundStyle(.tertiary)
                    .fixedSize(horizontal: false, vertical: true)
            }

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

    private func chooseSkillDir() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.prompt = loc("Choose", "选择")
        panel.message = loc("Choose the skill library folder (each subfolder under it is a loadable skill)",
                            "选择技能库文件夹(其下每个子文件夹是一个可加载的技能)")
        panel.directoryURL = URL(fileURLWithPath: skillDir)
        guard panel.runModal() == .OK, let path = panel.url?.path else { return }
        skillDir = path
        AppSettings.skillLibraryDir = path
    }
}
