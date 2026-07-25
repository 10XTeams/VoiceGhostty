import SwiftUI

/// Lightweight reactive UI localization.
///
/// Every call site carries both languages inline — `loc("Done", "完成")` — so there is no central
/// string table to keep in sync. The current language follows `AppSettings.defaultLanguage`, so the
/// UI language and the speech-recognition language switch together.
///
/// Views observe the shared instance with `@ObservedObject private var loc = Loc.shared`; when the
/// language changes, `lang` publishes and every observing view re-renders in the new language.
final class Loc: ObservableObject {
    static let shared = Loc()

    /// Current UI language, mirroring `AppSettings.defaultLanguage` ("zh-CN" or "en-US").
    @Published var lang: String

    private init() { lang = AppSettings.defaultLanguage }

    /// Return the string for the current language.
    func callAsFunction(_ en: String, _ zh: String) -> String {
        lang == "zh-CN" ? zh : en
    }

    /// Switch the language (persisting it) so both the UI and the recognizer follow.
    func set(_ newLang: String) {
        AppSettings.defaultLanguage = newLang
        lang = newLang
    }
}
