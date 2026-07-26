import Foundation

/// Local offline small model (Ollama, http://127.0.0.1:11434).
/// It does exactly one thing: **transcript correction** — fixing speech-recognition homophones/typos and dropping filler words.
/// Short in, short out; qwen3:1.7b is plenty.
/// It runs only on the natural-language path, as a pre-pass that hands NL2Command a clean intent; dictation mode goes to the
/// cursor verbatim and never comes through here. Command generation itself is Claude / Apple on-device, not this model.
enum OllamaClient {
    static let defaultModel = "qwen3:1.7b"
    static let defaultBaseURL = "http://127.0.0.1:11434"

    /// The Ollama service isn't running (can't connect). The caller uses this to fall back silently (on correction failure it uses the original text)
    struct Unavailable: Error {}

    /// ★ Must bypass the system proxy and connect to the local machine directly.
    /// URLSession follows the system proxy (Clash/Surge, etc.) by default, so even requests to 127.0.0.1 get forwarded through the proxy;
    /// in practice, requests routed through the proxy cause the model to emit abnormally long output until timeout; bypassing it restores normal behavior.
    /// Also note: qwen3's think toggle is unreliable under Ollama's chatml template; what actually suppresses the thinking output is the
    /// format (JSON Schema grammar) constraint — so format must be kept.
    private static let session: URLSession = {
        let config = URLSessionConfiguration.ephemeral
        config.connectionProxyDictionary = [:]   // empty dictionary = use no proxy
        return URLSession(configuration: config)
    }()

    /// Few-shot examples are essential: a 1.7B-class small model, given only rules, will delete content words at random; with examples its behavior is stable.
    /// Observed limits: removing filler words / de-stuttering is reliable; correcting homophone typos is not reliable for a small model, so it's best-effort.
    private static let instructions = """
    You are a text tidier for voice dictation, responsible for cleaning up speech-recognition output. Rules:
    1. Fix obvious homophones/typos
    2. Remove filler words (um, uh, ah, oh, like, you know, and similar verbal tics) and repeated stutters
    3. Keep everything else exactly as is: don't rewrite the sentence structure, don't add or remove content words, don't translate, don't answer questions. Copy English, commands, paths, and numbers verbatim.

    Examples:
    Input: um so restart the service for me
    Output: restart the service for me
    Input: delete delete this this file
    Output: delete this file
    Input: check how much disk space is left
    Output: check how much disk space is left
    Input: uh show the currant port usage
    Output: show the current port usage
    """

    /// Correct one dictated sentence. When the model output is abnormal it returns the original text — correction can only be a nice-to-have and must never lose the user's words.
    /// When the service is unavailable it throws Unavailable, and the caller falls back to the original text.
    static func correct(_ text: String,
                        model: String,
                        baseURL: String) async throws -> String {
        let raw = try await send(text, model: model, baseURL: baseURL, think: false)
        return sanitize(raw, original: text) ?? text
    }

    /// Output cleanup + safety valve.
    /// - Strip /no_think and /think: the soft thinking toggles injected by Ollama's qwen3 chatml template get echoed back by the model
    /// - A sudden length change (shrinking/ballooning too much relative to the original) is treated as the model going off the rails (deleting content words at random / adding fluff), so fall back to the original text
    private static func sanitize(_ raw: String, original: String) -> String? {
        let cleaned = raw
            .replacingOccurrences(of: "/no_think", with: "")
            .replacingOccurrences(of: "/think", with: "")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard !cleaned.isEmpty else { return nil }
        // Very short input (like "um ls") legitimately compresses a lot, so only guard against ballooning
        if original.count > 6 {
            let ratio = Double(cleaned.count) / Double(original.count)
            guard ratio >= 0.5 else { return nil }
        }
        guard Double(cleaned.count) <= Double(original.count) * 1.5 else { return nil }
        return cleaned
    }

    private static func send(_ text: String,
                             model: String,
                             baseURL: String,
                             think: Bool?) async throws -> String {
        guard let url = URL(string: "\(baseURL)/api/chat") else { throw Unavailable() }
        var request = URLRequest(url: url)
        // Correction must be imperceptible: with the model warmed up a single sentence takes <1 second; beyond 15 seconds it's better to use the original text
        request.timeoutInterval = 15
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")

        var body: [String: Any] = [
            "model": model,
            "stream": false,
            // Keep the model warm for 30 minutes: Ollama unloads after 5 minutes by default, and a cold load of a dozen-plus seconds feels like a freeze
            "keep_alive": "30m",
            "messages": [
                ["role": "system", "content": instructions],
                ["role": "user", "content": text],
            ],
            // structured outputs: guarantees the response is exactly the plain-text field, with no explanatory fluff
            "format": [
                "type": "object",
                "properties": ["text": ["type": "string"]],
                "required": ["text"],
            ],
            "options": ["temperature": 0],
        ]
        if let think { body["think"] = think }
        request.httpBody = try JSONSerialization.data(withJSONObject: body)

        let (data, response): (Data, URLResponse)
        do {
            (data, response) = try await session.data(for: request)
        } catch {
            throw Unavailable()   // can't connect / timed out; uniformly let the caller fall back to the original text
        }

        guard let http = response as? HTTPURLResponse else { throw Unavailable() }
        guard http.statusCode == 200 else {
            // If switched to a model that doesn't support thinking mode, drop the think parameter and retry once
            let detail = (try? JSONDecoder().decode(ErrorResponse.self, from: data))?.error
            if think != nil, let detail, detail.contains("does not support thinking") {
                return try await send(text, model: model, baseURL: baseURL, think: nil)
            }
            throw Unavailable()
        }

        guard let reply = try? JSONDecoder().decode(ChatResponse.self, from: data),
              let jsonData = reply.message.content.data(using: .utf8),
              let parsed = try? JSONDecoder().decode(Corrected.self, from: jsonData),
              !parsed.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            throw Unavailable()   // if it can't be parsed, also fall back to the original text
        }
        return parsed.text
    }

    /// Warm-up: get Ollama to load the model into memory (an /api/generate with an empty prompt only triggers loading, no generation).
    /// Called in the background at app startup to move the cold load to before the user starts speaking; failures are silently ignored.
    static func warmUp(model: String, baseURL: String) async {
        guard let url = URL(string: "\(baseURL)/api/generate") else { return }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.timeoutInterval = 120
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try? JSONSerialization.data(withJSONObject: [
            "model": model, "keep_alive": "30m",
        ])
        _ = try? await session.data(for: request)
    }

    // MARK: - Response models (only decode the fields we use)

    private struct ChatResponse: Decodable {
        struct Message: Decodable { let content: String }
        let message: Message
    }

    private struct Corrected: Decodable {
        let text: String
    }

    private struct ErrorResponse: Decodable {
        let error: String
    }
}
