import SwiftUI
import AppKit

/// Combined Settings + Shortcuts panel (popped up by the toolbar's gear button).
/// A segmented control switches between the editable settings and the read-only shortcut reference.
struct SettingsView: View {
    var onDone: () -> Void = {}

    @ObservedObject private var loc = Loc.shared

    private enum Tab: Hashable { case settings, shortcuts }

    @State private var tab: Tab = .settings
    @State private var language: String = AppSettings.defaultLanguage
    @State private var skillDir: String = AppSettings.skillLibraryDir
    @State private var inputColor: Color = Color(hex: AppSettings.inputColor)

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
                Spacer()
                Button(loc("Done", "完成")) { onDone() }
                    .keyboardShortcut(.defaultAction)
            }
        }
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
