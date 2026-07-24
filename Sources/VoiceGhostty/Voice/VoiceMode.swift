/// 语音交互的两种模式(统一管线,只在中间处理阶段分叉)
enum VoiceMode: String, CaseIterable, Identifiable {
    /// 语音转文字后原样填入待确认区
    case dictation = "听写"
    /// 说人话,由 LLM 生成 shell 命令(Claude API 优先,无 Key 回落 Apple 端侧模型)
    case naturalLanguage = "自然语言"

    var id: String { rawValue }

    /// segmented 选择器上的图标(听写=转文字光标,自然语言=AI)
    var icon: String {
        switch self {
        case .dictation:       return "text.cursor"
        case .naturalLanguage: return "sparkles"
        }
    }
}
