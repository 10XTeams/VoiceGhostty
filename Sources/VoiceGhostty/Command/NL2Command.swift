import Foundation
#if canImport(FoundationModels)
import FoundationModels
#endif

/// Natural language → shell command. Two paths:
///   - Claude API (cloud LLM, ClaudeClient.swift): preferred when an API Key is configured; most capable
///   - Apple on-device model (FoundationModels, macOS 26+): the local fallback when there is no Key
/// The local Ollama small model only tidies the transcript before it gets here (OllamaClient.swift); it does not take part in command generation.
/// The result is sent straight to the terminal cursor (with newlines stripped) and is never executed automatically.
///
/// Controlled by the llm-provider key in `~/.config/voiceghostty/config`:
///   auto (default, uses Claude when a Key is present) | claude | apple
///
/// Note: the CLT-only build environment has no @Generable/@Guide macro plugin (that ships with Xcode),
/// so instead of guided generation we make the model output plain JSON and parse it locally.
enum NL2Command {
    struct Translation {
        /// The generated shell command
        let command: String
        /// A one-sentence English explanation for the user, shown in the status bar to reduce the risk of mistaken execution
        let explanation: String
    }

    /// - Parameter currentDirectory: the current working directory (from Shell integration OSC 7), fed to the model to improve command accuracy
    static func translate(_ naturalLanguage: String,
                          currentDirectory: String? = nil) async throws -> Translation {
        let config = Config.load()
        let apiKey = resolveAPIKey(config)

        switch config.llmProvider {
        case "claude":
            guard let apiKey else {
                throw NL2CommandError.message(
                    "llm-provider = claude but there is no API Key; please add llm-api-key to the config file")
            }
            return try await translateViaClaude(naturalLanguage,
                                                currentDirectory: currentDirectory,
                                                apiKey: apiKey, config: config)
        case "apple":
            break  // fall through to the on-device path below
        default:   // auto: use Claude when a Key is present, otherwise on-device
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
        throw NL2CommandError.message("Natural language mode requires configuring llm-api-key (Claude) or the on-device model on macOS 26+")
    }

    /// API Key sources: the llm-api-key config file entry takes priority, then the ANTHROPIC_API_KEY environment variable
    /// (an app launched from Finder can't see shell environment variables, so the config file is the primary path)
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
        // Check whether the Apple Intelligence on-device model is ready
        if case .unavailable(let reason) = SystemLanguageModel.default.availability {
            throw NL2CommandError.message(unavailableText(reason))
        }

        let session = LanguageModelSession(instructions: instructions(currentDirectory: currentDirectory))
        let reply = try await session.respond(to: text)
        let parsed = parse(reply.content)
        guard !parsed.command.isEmpty else {
            throw NL2CommandError.message(parsed.explanation.isEmpty ? "Couldn't generate a command; try rephrasing it" : parsed.explanation)
        }
        return parsed
    }

    @available(macOS 26, *)
    private static func unavailableText(_ reason: SystemLanguageModel.Availability.UnavailableReason) -> String {
        switch reason {
        case .deviceNotEligible:
            return "This device does not support the Apple Intelligence on-device model"
        case .appleIntelligenceNotEnabled:
            return "Please first enable Apple Intelligence in System Settings → Apple Intelligence & Siri"
        case .modelNotReady:
            return "The on-device model is downloading/preparing; please try again later"
        @unknown default:
            return "The on-device model is currently unavailable"
        }
    }
    #endif

    /// The system prompt shared by both paths (Claude uses structured outputs, with the format enforced by the API layer;
    /// the on-device model relies on the textual constraints here plus parse() for fault tolerance)
    static func instructions(currentDirectory: String?) -> String {
        var s = """
        You are a command assistant for the macOS terminal (zsh). Turn the user's natural-language intent into [one] directly executable shell command.
        Output only a single JSON object, no markdown, no code block, no extra text, in this format:
        {"command": "<a single shell command, the command only>", "explanation": "<a one-sentence explanation in English, flagging the risk for dangerous operations>"}
        When it cannot be done safely, leave command as an empty string and explain why in explanation.
        """
        if let cwd = currentDirectory, !cwd.isEmpty {
            s += "\nCurrent working directory: \(cwd)"
        }
        return s
    }

    /// Extract the JSON from the model output and parse it; if none can be found, treat the whole thing as the command (fault tolerance).
    private static func parse(_ raw: String) -> Translation {
        if let start = raw.firstIndex(of: "{"),
           let end = raw.lastIndex(of: "}"), start <= end,
           let data = String(raw[start...end]).data(using: .utf8),
           let obj = try? JSONDecoder().decode(RawSuggestion.self, from: data) {
            return Translation(command: obj.command.trimmingCharacters(in: .whitespacesAndNewlines),
                               explanation: obj.explanation)
        }
        // Fallback: the model didn't follow the format, so treat the whole thing as the command
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

    /// Diagnostics: the current availability status of the on-device model
    static func availabilityStatus() -> String {
        #if canImport(FoundationModels)
        if #available(macOS 26, *) {
            switch SystemLanguageModel.default.availability {
            case .available: return "available (model ready)"
            case .unavailable(.deviceNotEligible): return "deviceNotEligible (region/device not supported)"
            case .unavailable(.appleIntelligenceNotEnabled): return "appleIntelligenceNotEnabled (Apple Intelligence not enabled)"
            case .unavailable(.modelNotReady): return "modelNotReady (model downloading/preparing)"
            case .unavailable: return "unavailable (other reason)"
            }
        }
        #endif
        return "macOS < 26, no FoundationModels"
    }
}
