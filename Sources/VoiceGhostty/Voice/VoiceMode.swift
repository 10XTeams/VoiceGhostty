/// The two voice interaction modes (unified pipeline, forking only at the intermediate processing stage)
enum VoiceMode: String, CaseIterable, Identifiable {
    /// Speech-to-text is filled verbatim into the confirmation area
    case dictation = "Dictation"
    /// Speak naturally; an LLM generates the shell command (Claude API preferred, falling back to Apple's on-device model when no key is set)
    case naturalLanguage = "Natural Language"

    var id: String { rawValue }

    /// Icon on the segmented picker (dictation = text cursor, natural language = AI)
    var icon: String {
        switch self {
        case .dictation:       return "text.cursor"
        case .naturalLanguage: return "sparkles"
        }
    }
}
