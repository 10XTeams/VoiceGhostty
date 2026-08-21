import Foundation

/// What a folder added in Settings actually is.
///
/// The loader asks this once per folder and then keeps the two kinds strictly apart: a plugin folder
/// never contributes skills or agents, and a skill library never contributes plugins. Claude Code marks
/// both kinds itself — a marketplace carries `.claude-plugin/marketplace.json`, a single plugin carries
/// `.claude-plugin/plugin.json` — so the classification reads the same manifests Claude Code reads,
/// rather than guessing from folder names.
enum LoaderLibrary {

    /// One plugin offered by an added folder
    struct PluginEntry: Hashable {
        let name: String
        /// Marketplace it belongs to; empty when the folder is a bare plugin with no marketplace.json
        let marketplace: String
        let description: String
        /// The folder the user added — this is what `extraKnownMarketplaces` has to point at
        let libraryDir: String

        /// The id Claude Code uses in `enabledPlugins`
        var id: String { marketplace.isEmpty ? name : "\(name)@\(marketplace)" }
    }

    enum Kind {
        /// No plugin manifest: subfolders are skills, top-level `.md` files are agents
        case skills
        /// `.claude-plugin/marketplace.json` — one or more plugins, loadable through project settings
        case marketplace(name: String, plugins: [PluginEntry])
        /// `.claude-plugin/plugin.json` but no marketplace.json. Listed so it is visible, but it cannot
        /// be switched on from project settings: those reference plugins by marketplace, and this folder
        /// does not declare one.
        case barePlugin(PluginEntry)

        var isSkillLibrary: Bool {
            if case .skills = self { return true }
            return false
        }

        /// Plugins this folder offers (empty for a skill library)
        var plugins: [PluginEntry] {
            switch self {
            case .skills:                    return []
            case .marketplace(_, let list):  return list
            case .barePlugin(let entry):     return [entry]
            }
        }
    }

    /// Classification is read off disk, but SwiftUI re-evaluates a row's body far more often than the
    /// folder changes, so the answer is memoized. Every rescan (and every add/remove) clears it.
    private static var cache: [String: Kind] = [:]

    static func invalidateCache() { cache.removeAll() }

    static func classify(_ dir: String) -> Kind {
        if let hit = cache[dir] { return hit }
        let kind = read(dir)
        cache[dir] = kind
        return kind
    }

    private static func read(_ dir: String) -> Kind {
        let manifestDir = (dir as NSString).appendingPathComponent(".claude-plugin")
        let marketplacePath = (manifestDir as NSString).appendingPathComponent("marketplace.json")
        let pluginPath = (manifestDir as NSString).appendingPathComponent("plugin.json")

        if let json = readObject(at: marketplacePath) {
            // A marketplace with an unreadable name is still a marketplace — fall back to the folder name
            let name = (json["name"] as? String) ?? (dir as NSString).lastPathComponent
            let raw = json["plugins"] as? [[String: Any]] ?? []
            let plugins = raw.compactMap { entry -> PluginEntry? in
                guard let pluginName = entry["name"] as? String, !pluginName.isEmpty else { return nil }
                return PluginEntry(name: pluginName,
                                   marketplace: name,
                                   description: entry["description"] as? String ?? "",
                                   libraryDir: dir)
            }
            return .marketplace(name: name, plugins: plugins)
        }

        if let json = readObject(at: pluginPath) {
            let name = (json["name"] as? String) ?? (dir as NSString).lastPathComponent
            return .barePlugin(PluginEntry(name: name,
                                           marketplace: "",
                                           description: json["description"] as? String ?? "",
                                           libraryDir: dir))
        }

        return .skills
    }

    /// nil for a missing or malformed file — a broken manifest just means "not that kind of folder",
    /// which degrades to treating it as a skill library rather than failing the whole panel.
    private static func readObject(at path: String) -> [String: Any]? {
        guard FileManager.default.fileExists(atPath: path),
              let data = try? Data(contentsOf: URL(fileURLWithPath: path)),
              let object = try? JSONSerialization.jsonObject(with: data),
              let dict = object as? [String: Any] else { return nil }
        return dict
    }
}
