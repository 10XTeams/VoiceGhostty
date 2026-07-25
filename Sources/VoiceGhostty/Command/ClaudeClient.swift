import Foundation

/// Minimal Claude API client (Swift has no official SDK, so we POST directly to /v1/messages).
/// It does exactly one thing: natural language → {command, explanation} JSON.
/// Structured outputs (output_config.format json_schema) let the API layer guarantee the output
/// structure, so the client doesn't need any "scrape the JSON out" fallback handling.
enum ClaudeClient {
    static let defaultModel = "claude-opus-4-8"

    /// - Parameters:
    ///   - apiKey: Anthropic API Key (from the llm-api-key config file entry or the ANTHROPIC_API_KEY environment variable)
    ///   - model: Model ID; can be overridden by the llm-model config file entry
    static func translate(_ naturalLanguage: String,
                          instructions: String,
                          apiKey: String,
                          model: String) async throws -> NL2Command.Translation {
        var request = URLRequest(url: URL(string: "https://api.anthropic.com/v1/messages")!)
        request.httpMethod = "POST"
        request.timeoutInterval = 30
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue(apiKey, forHTTPHeaderField: "x-api-key")
        request.setValue("2023-06-01", forHTTPHeaderField: "anthropic-version")

        // Voice use cases need speed: effort=low lowers latency; json_schema guarantees the output is exactly the target JSON
        let body: [String: Any] = [
            "model": model,
            "max_tokens": 1024,
            "system": instructions,
            "output_config": [
                "effort": "low",
                "format": [
                    "type": "json_schema",
                    "schema": [
                        "type": "object",
                        "properties": [
                            "command": ["type": "string",
                                        "description": "A single directly executable shell command, the command only; an empty string when it cannot be done safely"],
                            "explanation": ["type": "string",
                                            "description": "A one-sentence explanation in English, flagging the risk for dangerous operations"],
                        ],
                        "required": ["command", "explanation"],
                        "additionalProperties": false,
                    ],
                ],
            ],
            "messages": [["role": "user", "content": naturalLanguage]],
        ]
        request.httpBody = try JSONSerialization.data(withJSONObject: body)

        let (data, response): (Data, URLResponse)
        do {
            (data, response) = try await URLSession.shared.data(for: request)
        } catch {
            throw NL2Command.NL2CommandError.message("Network request failed: \(error.localizedDescription)")
        }

        guard let http = response as? HTTPURLResponse else {
            throw NL2Command.NL2CommandError.message("Unexpected Claude API response")
        }
        guard http.statusCode == 200 else {
            throw NL2Command.NL2CommandError.message(errorText(statusCode: http.statusCode, data: data))
        }

        let message = try JSONDecoder().decode(MessageResponse.self, from: data)

        // The safety classifier may refuse (HTTP 200 + stop_reason=refusal), in which case content may be empty
        if message.stopReason == "refusal" {
            throw NL2Command.NL2CommandError.message("This request was refused by Claude's safety policy; try rephrasing it")
        }
        guard let text = message.content.first(where: { $0.type == "text" })?.text,
              let jsonData = text.data(using: .utf8),
              let parsed = try? JSONDecoder().decode(RawSuggestion.self, from: jsonData) else {
            throw NL2Command.NL2CommandError.message("Couldn't generate a command; try rephrasing it")
        }
        return NL2Command.Translation(
            command: parsed.command.trimmingCharacters(in: .whitespacesAndNewlines),
            explanation: parsed.explanation)
    }

    /// Translate an HTTP error into a single sentence the user can understand
    private static func errorText(statusCode: Int, data: Data) -> String {
        // API error body: {"type":"error","error":{"type":"...","message":"..."}}
        let apiMessage = (try? JSONDecoder().decode(ErrorResponse.self, from: data))?.error.message
        switch statusCode {
        case 401: return "Invalid API Key; please check the llm-api-key config"
        case 403: return "API Key lacks permission: \(apiMessage ?? "permission_error")"
        case 429: return "Too many requests (rate limited); please try again later"
        case 500...: return "Claude service is temporarily unavailable; please try again later"
        default:  return "Claude API error (\(statusCode)): \(apiMessage ?? "unknown error")"
        }
    }

    // MARK: - Response models (only decode the fields we use)

    private struct MessageResponse: Decodable {
        let content: [ContentBlock]
        let stopReason: String?
        enum CodingKeys: String, CodingKey {
            case content
            case stopReason = "stop_reason"
        }
    }

    private struct ContentBlock: Decodable {
        let type: String
        let text: String?
    }

    private struct RawSuggestion: Decodable {
        let command: String
        let explanation: String
    }

    private struct ErrorResponse: Decodable {
        struct APIError: Decodable { let message: String }
        let error: APIError
    }
}
