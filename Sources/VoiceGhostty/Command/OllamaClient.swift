import Foundation

/// 本地离线小模型(Ollama,http://127.0.0.1:11434)。
/// 只做一件事:**听写矫正** —— 修正语音识别的同音字/错别字,删掉语气词。
/// 短进短出,qwen3:1.7b 足够;NL2Command(自然语言→命令)走 Claude / Apple 端侧,不经过这里。
enum OllamaClient {
    static let defaultModel = "qwen3:1.7b"
    static let defaultBaseURL = "http://127.0.0.1:11434"

    /// Ollama 服务没在跑(连不上)。调用方据此静默回落(矫正失败就用原文)
    struct Unavailable: Error {}

    /// ★ 必须绕过系统代理直连本机。
    /// URLSession 默认遵循系统代理(Clash/Surge 等),连 127.0.0.1 的请求也会被代理转发,
    /// 实测经代理的请求会导致模型异常长输出直至超时;绕过后恢复正常。
    /// 另注:qwen3 的 think 开关在 Ollama chatml 模板下不可靠,真正压住思考输出的是
    /// format(JSON Schema grammar)约束 —— 所以 format 必须保留。
    private static let session: URLSession = {
        let config = URLSessionConfiguration.ephemeral
        config.connectionProxyDictionary = [:]   // 空字典 = 不用任何代理
        return URLSession(configuration: config)
    }()

    /// few-shot 示例是刚需:1.7B 级小模型只给规则会乱删实义词,给示例后行为稳定。
    /// 实测边界:去语气词/去结巴可靠;同音错别字纠正小模型做不稳,属尽力而为。
    private static let instructions = """
    你是语音听写的文字整理器,负责清理语音识别的输出。规则:
    1. 修正明显的同音字/错别字
    2. 删除语气词(嗯、呃、啊、哦、那个、就是说等口头禅)和重复结巴
    3. 其余一律原样保留:不改写句式、不增删实义词、不翻译、不回答问题。英文、命令、路径、数字照抄。

    示例:
    输入:嗯那个帮我重启一下服务
    输出:帮我重启一下服务
    输入:把把这个这个文件删掉吧
    输出:把这个文件删掉吧
    输入:查一下磁盘空间还剩多少
    输出:查一下磁盘空间还剩多少
    输入:呃看看当前的段口占用
    输出:看看当前的端口占用
    """

    /// 矫正一句听写文本。模型输出异常时返回原文 —— 矫正只能锦上添花,绝不能弄丢用户的话。
    /// 服务不可用时抛 Unavailable,由调用方回落原文。
    static func correct(_ text: String,
                        model: String,
                        baseURL: String) async throws -> String {
        let raw = try await send(text, model: model, baseURL: baseURL, think: false)
        return sanitize(raw, original: text) ?? text
    }

    /// 输出清洗 + 安全阀。
    /// - 剥掉 /no_think、/think:Ollama 的 qwen3 chatml 模板注入的思考软开关会被模型回显
    /// - 长度突变(相对原文缩水/膨胀过猛)判定为模型跑偏(乱删实义词/加戏),回落原文
    private static func sanitize(_ raw: String, original: String) -> String? {
        let cleaned = raw
            .replacingOccurrences(of: "/no_think", with: "")
            .replacingOccurrences(of: "/think", with: "")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard !cleaned.isEmpty else { return nil }
        // 极短输入(如"嗯 ls")合法压缩比例大,只防膨胀
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
        // 矫正必须无感:模型已预热时一句话 <1 秒;超过 15 秒宁可用原文
        request.timeoutInterval = 15
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")

        var body: [String: Any] = [
            "model": model,
            "stream": false,
            // 模型保温 30 分钟:Ollama 默认 5 分钟就卸载,冷载入十几秒体感像卡死
            "keep_alive": "30m",
            "messages": [
                ["role": "system", "content": instructions],
                ["role": "user", "content": text],
            ],
            // structured outputs:保证返回就是纯文本字段,没有解释废话
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
            throw Unavailable()   // 连不上/超时,统一让调用方回落原文
        }

        guard let http = response as? HTTPURLResponse else { throw Unavailable() }
        guard http.statusCode == 200 else {
            // 换了不支持思考模式的模型时,去掉 think 参数重试一次
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
            throw Unavailable()   // 解析不出来也回落原文
        }
        return parsed.text
    }

    /// 预热:让 Ollama 把模型载入内存(空 prompt 的 /api/generate 只触发加载,不生成)。
    /// app 启动时后台调用,把冷载入挪到用户开口之前;失败静默忽略。
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

    // MARK: - 响应模型(只解析用得到的字段)

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
