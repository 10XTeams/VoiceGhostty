import Foundation

/// Claude API 极简客户端(Swift 无官方 SDK,直连 POST /v1/messages)。
/// 只做一件事:自然语言 → {command, explanation} JSON。
/// 用 structured outputs(output_config.format json_schema)让 API 层保证输出结构,
/// 无需在客户端做"抠 JSON"容错。
enum ClaudeClient {
    static let defaultModel = "claude-opus-4-8"

    /// - Parameters:
    ///   - apiKey: Anthropic API Key(来自配置文件 llm-api-key 或 ANTHROPIC_API_KEY 环境变量)
    ///   - model: 模型 ID,配置文件 llm-model 可覆盖
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

        // 语音场景要快:effort=low 降低延迟;json_schema 保证输出就是目标 JSON
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
                                        "description": "一条可直接执行的 shell 命令,只有命令本身;无法安全完成时为空字符串"],
                            "explanation": ["type": "string",
                                            "description": "一句中文说明,危险操作要提示风险"],
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
            throw NL2Command.NL2CommandError.message("网络请求失败:\(error.localizedDescription)")
        }

        guard let http = response as? HTTPURLResponse else {
            throw NL2Command.NL2CommandError.message("Claude API 响应异常")
        }
        guard http.statusCode == 200 else {
            throw NL2Command.NL2CommandError.message(errorText(statusCode: http.statusCode, data: data))
        }

        let message = try JSONDecoder().decode(MessageResponse.self, from: data)

        // 安全分类器可能拒答(HTTP 200 + stop_reason=refusal),此时 content 可能为空
        if message.stopReason == "refusal" {
            throw NL2Command.NL2CommandError.message("该请求被 Claude 安全策略拒绝,换个说法试试")
        }
        guard let text = message.content.first(where: { $0.type == "text" })?.text,
              let jsonData = text.data(using: .utf8),
              let parsed = try? JSONDecoder().decode(RawSuggestion.self, from: jsonData) else {
            throw NL2Command.NL2CommandError.message("没能生成命令,换个说法试试")
        }
        return NL2Command.Translation(
            command: parsed.command.trimmingCharacters(in: .whitespacesAndNewlines),
            explanation: parsed.explanation)
    }

    /// 把 HTTP 错误翻译成用户能看懂的一句话
    private static func errorText(statusCode: Int, data: Data) -> String {
        // API 错误体:{"type":"error","error":{"type":"...","message":"..."}}
        let apiMessage = (try? JSONDecoder().decode(ErrorResponse.self, from: data))?.error.message
        switch statusCode {
        case 401: return "API Key 无效,请检查 llm-api-key 配置"
        case 403: return "API Key 无权限:\(apiMessage ?? "permission_error")"
        case 429: return "请求太频繁(限流),稍后再试"
        case 500...: return "Claude 服务暂时不可用,稍后再试"
        default:  return "Claude API 错误(\(statusCode)):\(apiMessage ?? "未知错误")"
        }
    }

    // MARK: - 响应模型(只解析用得到的字段)

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
