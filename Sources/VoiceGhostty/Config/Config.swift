import AppKit

/// User configuration, loaded from `~/.config/voiceghostty/config`.
/// Minimal `key = value` format, `#` starts a comment. Supports 12 keys:
///   font-family, font-size, theme, foreground, background,
///   llm-provider, llm-model, llm-api-key, llm-local-model, llm-local-url, correction,
///   skill-library-dir
struct Config {
    /// Empty = system monospaced SF Mono; a named font is used only when font-family is set in the config file
    var fontFamily: String = ""
    var fontSize: CGFloat = 12
    var themeName: String = "dark"
    /// Explicit override from the config file (takes precedence over the theme's built-in colors); nil means no override
    var foregroundOverride: NSColor?
    var backgroundOverride: NSColor?

    // MARK: LLM selection for natural-language mode (consumed by NL2Command.swift)
    /// auto (default: use Claude if an API key is present, otherwise Apple on-device) | claude | apple
    var llmProvider: String = "auto"
    /// Claude model ID
    var llmModel: String = ClaudeClient.defaultModel
    /// Anthropic API key; if empty, falls back to the ANTHROPIC_API_KEY environment variable
    var llmAPIKey: String = ""

    // MARK: Dictation correction (consumed by OllamaClient.swift): remove filler words, fix typos
    /// Whether dictation correction is enabled (local Ollama small model; if the service is down, silently uses the raw text)
    var correctionEnabled: Bool = true
    /// Local model used for correction (Ollama model name)
    var llmLocalModel: String = OllamaClient.defaultModel
    /// Local Ollama service address
    var llmLocalURL: String = OllamaClient.defaultBaseURL

    // MARK: Skill dynamic loading (consumed by SkillPanelView.swift)
    /// Skill library (source) directory: the panel lists its subfolders as selectable skills
    var skillLibraryDir: String =
        (NSHomeDirectory() as NSString).appendingPathComponent(".claude/skills")

    /// Resolved final theme (with fg/bg overrides applied)
    var theme: Theme {
        Theme.named(themeName).overriding(foreground: foregroundOverride, background: backgroundOverride)
    }

    var font: NSFont {
        let systemMono = NSFont.monospacedSystemFont(ofSize: fontSize, weight: .regular)
        guard !fontFamily.isEmpty else { return systemMono }   // Default: SF Mono
        return NSFont(name: fontFamily, size: fontSize) ?? systemMono
    }

    static var configPath: String {
        (NSHomeDirectory() as NSString).appendingPathComponent(".config/voiceghostty/config")
    }

    /// Read and parse the config file; if the file is missing or has bad lines, fall back to defaults (bad lines are ignored, no error).
    static func load() -> Config {
        var config = Config()
        guard let text = try? String(contentsOfFile: configPath, encoding: .utf8) else {
            return config
        }
        for rawLine in text.split(whereSeparator: \.isNewline) {
            let line = rawLine.trimmingCharacters(in: .whitespaces)
            if line.isEmpty || line.hasPrefix("#") { continue }
            guard let eq = line.firstIndex(of: "=") else { continue }
            let key = line[..<eq].trimmingCharacters(in: .whitespaces)
            let value = line[line.index(after: eq)...].trimmingCharacters(in: .whitespaces)
            guard !value.isEmpty else { continue }

            switch key {
            case "font-family": config.fontFamily = value
            case "font-size":   if let n = Double(value) { config.fontSize = CGFloat(n) }
            case "theme":       config.themeName = value
            case "foreground":  config.foregroundOverride = hexNS(value)
            case "background":  config.backgroundOverride = hexNS(value)
            case "llm-provider": config.llmProvider = value.lowercased()
            case "llm-model":    config.llmModel = value
            case "llm-api-key":  config.llmAPIKey = value
            case "llm-local-model": config.llmLocalModel = value
            case "llm-local-url":   config.llmLocalURL = value
            case "correction":      config.correctionEnabled = (value.lowercased() != "off")
            case "skill-library-dir": config.skillLibraryDir = (value as NSString).expandingTildeInPath
            default:            break  // Unknown keys are ignored
            }
        }
        return config
    }
}
