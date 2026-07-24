import Foundation
#if canImport(FoundationModels)
import FoundationModels
#endif

/// 自然语言 → shell 命令。两条路径:
///   - Claude API(云端 LLM,ClaudeClient.swift):配置了 API Key 时优先,能力最强
///   - Apple 端侧模型(FoundationModels, macOS 26+):无 Key 时的本地回落
/// 本地 Ollama 小模型只做听写矫正(OllamaClient.swift),不参与命令生成。
/// 结果直接送入终端光标处(剥离换行),绝不自动回车执行。
///
/// 由 `~/.config/voiceghostty/config` 的 llm-provider 键控制:
///   auto(默认,有 Key 走 Claude)| claude | apple
///
/// 注:CLT-only 构建环境没有 @Generable/@Guide 的 macro 插件(随 Xcode 走),
/// 故不用引导生成,而是让模型只输出 JSON,再本地解析。
enum NL2Command {
    struct Translation {
        /// 生成的 shell 命令
        let command: String
        /// 给用户看的一句话中文解释,显示在状态栏降低误执行风险
        let explanation: String
    }

    /// - Parameter currentDirectory: 当前工作目录(来自 Shell 集成 OSC 7),喂给模型提升命令准确度
    static func translate(_ naturalLanguage: String,
                          currentDirectory: String? = nil) async throws -> Translation {
        let config = Config.load()
        let apiKey = resolveAPIKey(config)

        switch config.llmProvider {
        case "claude":
            guard let apiKey else {
                throw NL2CommandError.message(
                    "llm-provider = claude 但没有 API Key,请在配置文件加 llm-api-key")
            }
            return try await translateViaClaude(naturalLanguage,
                                                currentDirectory: currentDirectory,
                                                apiKey: apiKey, config: config)
        case "apple":
            break  // 落到下方端侧路径
        default:   // auto:有 Key 走 Claude,否则端侧
            if let apiKey {
                return try await translateViaClaude(naturalLanguage,
                                                    currentDirectory: currentDirectory,
                                                    apiKey: apiKey, config: config)
            }
        }

        #if canImport(FoundationModels)
        if #available(macOS 26, *) {
            return try await translateOnDevice(naturalLanguage, currentDirectory: currentDirectory)
        }
        #endif
        throw NL2CommandError.message("自然语言模式需要配置 llm-api-key(Claude)或 macOS 26+ 的端侧模型")
    }

    /// API Key 来源:配置文件 llm-api-key 优先,其次 ANTHROPIC_API_KEY 环境变量
    /// (Finder 启动的 app 拿不到 shell 环境变量,所以配置文件是主路径)
    private static func resolveAPIKey(_ config: Config) -> String? {
        if !config.llmAPIKey.isEmpty { return config.llmAPIKey }
        if let env = ProcessInfo.processInfo.environment["ANTHROPIC_API_KEY"], !env.isEmpty {
            return env
        }
        return nil
    }

    private static func translateViaClaude(_ text: String,
                                           currentDirectory: String?,
                                           apiKey: String,
                                           config: Config) async throws -> Translation {
        try await ClaudeClient.translate(text,
                                         instructions: instructions(currentDirectory: currentDirectory),
                                         apiKey: apiKey,
                                         model: config.llmModel)
    }


    #if canImport(FoundationModels)
    @available(macOS 26, *)
    private static func translateOnDevice(_ text: String,
                                          currentDirectory: String?) async throws -> Translation {
        // 检查 Apple 智能端侧模型是否就绪
        if case .unavailable(let reason) = SystemLanguageModel.default.availability {
            throw NL2CommandError.message(unavailableText(reason))
        }

        let session = LanguageModelSession(instructions: instructions(currentDirectory: currentDirectory))
        let reply = try await session.respond(to: text)
        let parsed = parse(reply.content)
        guard !parsed.command.isEmpty else {
            throw NL2CommandError.message(parsed.explanation.isEmpty ? "没能生成命令,换个说法试试" : parsed.explanation)
        }
        return parsed
    }

    @available(macOS 26, *)
    private static func unavailableText(_ reason: SystemLanguageModel.Availability.UnavailableReason) -> String {
        switch reason {
        case .deviceNotEligible:
            return "此设备不支持 Apple 智能端侧模型"
        case .appleIntelligenceNotEnabled:
            return "请先在 系统设置 → Apple 智能与 Siri 里开启 Apple Intelligence"
        case .modelNotReady:
            return "端侧模型正在下载/准备中,请稍后再试"
        @unknown default:
            return "端侧模型暂不可用"
        }
    }
    #endif

    /// 两条路径共用的 system prompt(Claude 走 structured outputs,格式由 API 层强制;
    /// 端侧模型靠这里的文字约束 + parse() 容错)
    static func instructions(currentDirectory: String?) -> String {
        var s = """
        你是 macOS 终端(zsh)的命令助手。把用户的自然语言意图转成【一条】可直接执行的 shell 命令。
        只输出一个 JSON 对象,不要 markdown、不要代码块、不要多余文字,格式:
        {"command": "<一条 shell 命令,只有命令本身>", "explanation": "<一句中文说明,危险操作要提示风险>"}
        无法安全完成时 command 留空字符串,并在 explanation 说明原因。
        """
        if let cwd = currentDirectory, !cwd.isEmpty {
            s += "\n当前工作目录: \(cwd)"
        }
        return s
    }

    /// 从模型输出里抠出 JSON 并解析;抠不到就把整段当命令(容错)。
    private static func parse(_ raw: String) -> Translation {
        if let start = raw.firstIndex(of: "{"),
           let end = raw.lastIndex(of: "}"), start <= end,
           let data = String(raw[start...end]).data(using: .utf8),
           let obj = try? JSONDecoder().decode(RawSuggestion.self, from: data) {
            return Translation(command: obj.command.trimmingCharacters(in: .whitespacesAndNewlines),
                               explanation: obj.explanation)
        }
        // 兜底:模型没按格式来,整段当命令
        return Translation(command: raw.trimmingCharacters(in: .whitespacesAndNewlines), explanation: "")
    }

    private struct RawSuggestion: Decodable {
        let command: String
        let explanation: String
    }

    enum NL2CommandError: LocalizedError {
        case message(String)
        var errorDescription: String? {
            switch self { case .message(let m): return m }
        }
    }

    /// 诊断:端侧模型当前可用状态
    static func availabilityStatus() -> String {
        #if canImport(FoundationModels)
        if #available(macOS 26, *) {
            switch SystemLanguageModel.default.availability {
            case .available: return "available(模型就绪)"
            case .unavailable(.deviceNotEligible): return "deviceNotEligible(地区/设备不支持)"
            case .unavailable(.appleIntelligenceNotEnabled): return "appleIntelligenceNotEnabled(未开启 Apple 智能)"
            case .unavailable(.modelNotReady): return "modelNotReady(模型下载/准备中)"
            case .unavailable: return "unavailable(其它原因)"
            }
        }
        #endif
        return "macOS < 26,无 FoundationModels"
    }
}
