import AppKit

/// 用户配置,来自 `~/.config/voiceghostty/config`。
/// 极简 `key = value` 格式,`#` 起注释。支持 12 个键:
///   font-family, font-size, theme, foreground, background,
///   llm-provider, llm-model, llm-api-key, llm-local-model, llm-local-url, correction,
///   skill-library-dir
struct Config {
    /// 空 = 系统等宽 SF Mono;配置文件里写了 font-family 才用具名字体
    var fontFamily: String = ""
    var fontSize: CGFloat = 12
    var themeName: String = "dark"
    /// 配置文件显式覆盖(优先于主题内置色);nil 表示不覆盖
    var foregroundOverride: NSColor?
    var backgroundOverride: NSColor?

    // MARK: 自然语言模式的 LLM 选择(NL2Command.swift 消费)
    /// auto(默认:有 API Key 走 Claude,否则 Apple 端侧)| claude | apple
    var llmProvider: String = "auto"
    /// Claude 模型 ID
    var llmModel: String = ClaudeClient.defaultModel
    /// Anthropic API Key;空则回落 ANTHROPIC_API_KEY 环境变量
    var llmAPIKey: String = ""

    // MARK: 听写矫正(OllamaClient.swift 消费):去语气词、纠错别字
    /// 是否开启听写矫正(本地 Ollama 小模型;服务没起会静默用原文)
    var correctionEnabled: Bool = true
    /// 矫正用的本地模型(Ollama 模型名)
    var llmLocalModel: String = OllamaClient.defaultModel
    /// 本地 Ollama 服务地址
    var llmLocalURL: String = OllamaClient.defaultBaseURL

    // MARK: Skill 动态加载(SkillPanelView.swift 消费)
    /// 技能库(源)目录:面板列出它下面的子文件夹作为可选技能
    var skillLibraryDir: String =
        (NSHomeDirectory() as NSString).appendingPathComponent(".claude/skills")

    /// 解析后的最终主题(已叠加 fg/bg 覆盖)
    var theme: Theme {
        Theme.named(themeName).overriding(foreground: foregroundOverride, background: backgroundOverride)
    }

    var font: NSFont {
        let systemMono = NSFont.monospacedSystemFont(ofSize: fontSize, weight: .regular)
        guard !fontFamily.isEmpty else { return systemMono }   // 默认:SF Mono
        return NSFont(name: fontFamily, size: fontSize) ?? systemMono
    }

    static var configPath: String {
        (NSHomeDirectory() as NSString).appendingPathComponent(".config/voiceghostty/config")
    }

    /// 读取并解析配置文件;文件不存在或有坏行则回落默认值(坏行忽略,不报错)。
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
            default:            break  // 未知键忽略
            }
        }
        return config
    }
}
