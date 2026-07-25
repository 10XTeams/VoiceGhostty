import AppKit

/// GUI-editable settings backed by `UserDefaults` (set from the Settings panel).
///
/// These override the read-only `~/.config/voiceghostty/config` file for the keys they share
/// (currently `skill-library-dir`). The config file remains the fallback / power-user path.
enum AppSettings {
    private static let defaults = UserDefaults.standard

    // MARK: - Default speech-recognition language

    static let languageKey = "default-language"

    /// Selectable primary recognition locales shown in the Settings picker.
    /// Labels are endonyms so they read the same regardless of the current UI language.
    static let languageOptions: [(id: String, label: String)] = [
        ("zh-CN", "中文"),
        ("en-US", "English"),
    ]

    /// Primary recognition locale: "zh-CN" (default) or "en-US".
    /// The other locale is still tried as a fallback after recording stops.
    static var defaultLanguage: String {
        get { defaults.string(forKey: languageKey) ?? "zh-CN" }
        set { defaults.set(newValue, forKey: languageKey) }
    }

    // MARK: - Command input color

    static let inputColorKey = "input-color"

    /// Hex color (e.g. "#5CC8FF") applied to the command line you type at the zsh prompt.
    /// Writing also mirrors the value to `inputColorFilePath`, which the zsh highlight hook reads
    /// live on each redraw — so a color change takes effect in every open tab, not just new ones.
    static var inputColor: String {
        get { defaults.string(forKey: inputColorKey) ?? "#5CC8FF" }
        set {
            defaults.set(newValue, forKey: inputColorKey)
            writeInputColorFile()
        }
    }

    /// The file the zsh `line-pre-redraw` hook reads to color the current input line.
    static var inputColorFilePath: String {
        (NSHomeDirectory() as NSString).appendingPathComponent(".config/voiceghostty/input-color")
    }

    /// Mirror the current input color to `inputColorFilePath` (creating the directory if needed).
    static func writeInputColorFile() {
        let path = inputColorFilePath
        let dir = (path as NSString).deletingLastPathComponent
        try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        try? (inputColor + "\n").write(toFile: path, atomically: true, encoding: .utf8)
    }

    // MARK: - Skill library (source) directory

    static let skillLibraryDirKey = "skill-library-dir"

    /// Skill library folder whose subfolders the puzzle panel lists as loadable skills.
    /// A GUI selection (UserDefaults) wins, then the config file's `skill-library-dir`,
    /// then the default `~/.claude/skills`.
    static var skillLibraryDir: String {
        get { defaults.string(forKey: skillLibraryDirKey) ?? Config.load().skillLibraryDir }
        set { defaults.set(newValue, forKey: skillLibraryDirKey) }
    }
}
