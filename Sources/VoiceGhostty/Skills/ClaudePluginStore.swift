import Foundation

// MARK: - Reading and writing the project's plugin settings

/// Reads and writes exactly two keys of one project settings file: `enabledPlugins` and
/// `extraKnownMarketplaces`. It never touches `~/.claude`, and never rewrites a key it did not put there.
enum ClaudePluginStore {

    struct StoreError: LocalizedError {
        let message: String
        var errorDescription: String? { message }
    }

    /// Plugin ids switched on in a settings file. An absent file or key is an empty set, not an error.
    static func enabledPluginIDs(in path: String) throws -> Set<String> {
        let root = try readObject(at: path)
        guard let raw = root["enabledPlugins"] as? [String: Any] else { return [] }
        return Set(raw.compactMap { key, value in
            let flag = (value as? Bool) ?? ((value as? NSNumber)?.boolValue ?? false)
            return flag ? key : nil
        })
    }

    /// Rewrite `enabledPlugins` (to exactly `enabledIDs`) and the managed part of
    /// `extraKnownMarketplaces`, leaving every other setting in the file untouched.
    ///
    /// - Parameters:
    ///   - managedMarketplaces: (name, folder) pairs this panel is responsible for
    ///   - stillEnabled: whether a given marketplace still has at least one plugin switched on;
    ///     the ones that do not get their registration dropped again, so the file does not accumulate
    ///     paths for marketplaces nothing uses.
    static func write(enabledIDs: Set<String>,
                      managedMarketplaces: [(name: String, dir: String)],
                      stillEnabled: (String) -> Bool,
                      to path: String) throws {
        let fm = FileManager.default
        let existed = fm.fileExists(atPath: path)
        var root = try readObject(at: path)

        if enabledIDs.isEmpty {
            root.removeValue(forKey: "enabledPlugins")
        } else {
            root["enabledPlugins"] = enabledIDs.reduce(into: [String: Bool]()) { $0[$1] = true }
        }

        var markets = root["extraKnownMarketplaces"] as? [String: Any] ?? [:]
        for entry in managedMarketplaces {
            if stillEnabled(entry.name) {
                markets[entry.name] = ["source": ["source": "directory", "path": entry.dir]]
            } else if let existing = markets[entry.name] as? [String: Any],
                      let source = existing["source"] as? [String: Any],
                      source["path"] as? String == entry.dir {
                markets.removeValue(forKey: entry.name)   // Ours, and no longer needed
            }
        }
        if markets.isEmpty {
            root.removeValue(forKey: "extraKnownMarketplaces")
        } else {
            root["extraKnownMarketplaces"] = markets
        }

        // Browsing the panel must never litter a project with a stub settings file
        if root.isEmpty && !existed { return }

        let dir = (path as NSString).deletingLastPathComponent
        try fm.createDirectory(atPath: dir, withIntermediateDirectories: true)
        let data = try JSONSerialization.data(withJSONObject: root,
                                              options: [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes])
        try data.write(to: URL(fileURLWithPath: path), options: .atomic)
    }

    /// Parse a JSON object file. Missing or empty → `[:]`; malformed → throw, so a hand-edited settings
    /// file with a syntax error is reported instead of being silently flattened by a write.
    private static func readObject(at path: String) throws -> [String: Any] {
        guard FileManager.default.fileExists(atPath: path) else { return [:] }
        let data = try Data(contentsOf: URL(fileURLWithPath: path))
        guard !data.isEmpty else { return [:] }
        guard let object = try? JSONSerialization.jsonObject(with: data),
              let dict = object as? [String: Any] else {
            throw StoreError(message: (path as NSString).lastPathComponent
                             + Loc.shared(" is not valid JSON", " 不是合法的 JSON"))
        }
        return dict
    }
}
