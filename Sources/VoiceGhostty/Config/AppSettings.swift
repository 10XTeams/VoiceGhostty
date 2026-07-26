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

    // MARK: - Automatic punctuation

    static let addsPunctuationKey = "adds-punctuation"

    /// Whether recognition inserts punctuation (`SFSpeechRecognitionRequest.addsPunctuation`).
    /// On by default: dictating a sentence to Claude reads far better with punctuation. Turn it off if you
    /// mostly dictate shell commands, where a trailing period is noise you have to delete.
    /// Read when a recognition task starts, so a change applies to the next thing you say.
    static var addsPunctuation: Bool {
        get { defaults.object(forKey: addsPunctuationKey) as? Bool ?? true }
        set { defaults.set(newValue, forKey: addsPunctuationKey) }
    }

    // MARK: - Scrollback

    static let scrollbackKey = "scrollback"

    /// Bounds offered in the Settings panel. The ceiling is a memory guard: a stored line costs roughly a
    /// couple of KB, and every pane keeps its own history.
    static let scrollbackRange = 500...200_000

    /// Lines of scrolled-off output kept per pane. A GUI value wins, then the config file's `scrollback`.
    static var scrollback: Int {
        get { (defaults.object(forKey: scrollbackKey) as? Int).map(clampScrollback) ?? Config.load().scrollback }
        set { defaults.set(clampScrollback(newValue), forKey: scrollbackKey) }
    }

    static func clampScrollback(_ n: Int) -> Int {
        min(max(n, scrollbackRange.lowerBound), scrollbackRange.upperBound)
    }

    // MARK: - Session history file

    static let historyEnabledKey = "save-history"

    /// Whether each pane's transcript is appended to a file in `SessionLogger.directory`.
    static var historyEnabled: Bool {
        get { defaults.object(forKey: historyEnabledKey) as? Bool ?? true }
        set { defaults.set(newValue, forKey: historyEnabledKey) }
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

    // MARK: - Done chime

    static let doneSoundEnabledKey = "done-sound"

    /// Whether a chime plays when a pane's status light turns green (command finished — your turn).
    /// On by default: the light is only useful if you happen to be looking at it, and the whole point of
    /// the green state is that you have gone off to do something else.
    static var doneSoundEnabled: Bool {
        get { defaults.object(forKey: doneSoundEnabledKey) as? Bool ?? true }
        set { defaults.set(newValue, forKey: doneSoundEnabledKey) }
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
