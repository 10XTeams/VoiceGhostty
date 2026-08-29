import Foundation

/// What an added folder offers the loader.
///
/// The scan asks a folder what it holds, not what it is called, and reports each kind separately so the
/// panel can put it in its own tab: plugins from `.claude-plugin/`, skills from folders holding `SKILL.md`,
/// agents from `.md` files with front matter — under the folder itself or under its `skills/` / `agents/`.
///
/// The one either/or left is the plugin: **a folder that declares a plugin contributes only that plugin.**
/// Its skills and agents belong to the plugin and load with it, so offering them separately would only
/// invite loading the same thing twice.
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

    /// One skill: a folder that gets copied to `.claude/skills/<name>`
    struct SkillEntry: Hashable {
        let name: String
        /// Absolute source folder — not necessarily `<library>/<name>`, since skills may sit under `skills/`
        let path: String
        let libraryDir: String
    }

    /// One agent: a markdown file that gets copied to `.claude/agents/<fileName>`
    struct AgentEntry: Hashable {
        let fileName: String
        let path: String
        let libraryDir: String

        /// Displayed name — the `.md` suffix is noise once the row carries an icon
        var name: String { (fileName as NSString).deletingPathExtension }
    }

    /// Everything one added folder contributes
    struct Scan {
        let dir: String
        /// Name declared by `.claude-plugin/marketplace.json`; nil when the folder declares no marketplace.
        /// Only a named marketplace can be registered in project settings.
        let marketplace: String?
        let plugins: [PluginEntry]
        let skills: [SkillEntry]
        let agents: [AgentEntry]

        var isEmpty: Bool { plugins.isEmpty && skills.isEmpty && agents.isEmpty }
    }

    /// Scanning hits the disk, but SwiftUI re-evaluates a row's body far more often than a folder changes,
    /// so the answer is memoized. Every rescan (and every add/remove) clears it.
    private static var cache: [String: Scan] = [:]

    static func invalidateCache() { cache.removeAll() }

    static func scan(_ dir: String) -> Scan {
        if let hit = cache[dir] { return hit }
        let result = read(dir)
        cache[dir] = result
        return result
    }

    /// Scan every folder that still exists, in the order they were added
    static func scanAll(_ dirs: [String]) -> [Scan] {
        dirs.filter { FileManager.default.fileExists(atPath: $0) }.map(scan)
    }

    // MARK: - Reading one folder

    private static func read(_ dir: String) -> Scan {
        let (marketplace, plugins) = readPlugins(dir)
        // A folder that declares a plugin contributes **only** that plugin. The skills and agents inside
        // it are the plugin's own and arrive when the plugin loads; listing them here as well would offer
        // to copy a second, detached duplicate into the project's .claude/.
        // Judged on the manifest, not on the plugin count: a marketplace listing nothing loadable is
        // still a plugin folder, and scanning its subfolders as skills would list its plugin sources.
        guard marketplace == nil, plugins.isEmpty else {
            return Scan(dir: dir, marketplace: marketplace, plugins: plugins, skills: [], agents: [])
        }
        return Scan(dir: dir,
                    marketplace: nil,
                    plugins: [],
                    skills: readSkills(dir),
                    agents: readAgents(dir))
    }

    /// Claude Code marks a plugin folder itself: a marketplace carries `.claude-plugin/marketplace.json`,
    /// a single plugin carries `.claude-plugin/plugin.json`. The scan reads the same manifests rather than
    /// guessing from folder names.
    private static func readPlugins(_ dir: String) -> (marketplace: String?, plugins: [PluginEntry]) {
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
            return (name, plugins)
        }

        if let json = readObject(at: pluginPath) {
            let name = (json["name"] as? String) ?? (dir as NSString).lastPathComponent
            return (nil, [PluginEntry(name: name,
                                      marketplace: "",
                                      description: json["description"] as? String ?? "",
                                      libraryDir: dir)])
        }

        return (nil, [])
    }

    /// A skill is a folder holding `SKILL.md`, looked for under the folder's `skills/` subfolder and
    /// directly under the folder itself — the two layouts in the wild.
    ///
    /// Legacy fallback: a folder with no `SKILL.md` anywhere is an old-style library, where every
    /// subfolder was taken to be a skill. Without it, a library assembled before `SKILL.md` was the
    /// convention would suddenly list nothing.
    private static func readSkills(_ dir: String) -> [SkillEntry] {
        var found: [SkillEntry] = []
        var seen = Set<String>()
        for root in [(dir as NSString).appendingPathComponent("skills"), dir] {
            for name in subfolders(of: root).sorted() {
                let path = (root as NSString).appendingPathComponent(name)
                guard exists((path as NSString).appendingPathComponent("SKILL.md")),
                      seen.insert(name).inserted else { continue }
                found.append(SkillEntry(name: name, path: path, libraryDir: dir))
            }
        }
        guard found.isEmpty else { return found }
        return subfolders(of: dir).sorted().map {
            SkillEntry(name: $0, path: (dir as NSString).appendingPathComponent($0), libraryDir: dir)
        }
    }

    /// Files that are documentation for the folder, not agents — only relevant to the legacy fallback,
    /// which has no front matter to go by
    private static let docFileNames: Set<String> = ["readme.md", "license.md", "changelog.md",
                                                   "contributing.md", "claude.md", "agents.md"]

    /// An agent is a markdown file opening with YAML front matter (Claude Code requires `name` and
    /// `description` there), looked for under `agents/` and directly under the folder. Front matter is
    /// what separates an agent from the folder's own README.
    ///
    /// Legacy fallback, as for skills: no front-matter file anywhere → every top-level `.md` except the
    /// obvious documentation files.
    private static func readAgents(_ dir: String) -> [AgentEntry] {
        var found: [AgentEntry] = []
        var seen = Set<String>()
        for root in [(dir as NSString).appendingPathComponent("agents"), dir] {
            for name in markdownFiles(of: root).sorted() {
                let path = (root as NSString).appendingPathComponent(name)
                guard hasFrontMatter(at: path), seen.insert(name).inserted else { continue }
                found.append(AgentEntry(fileName: name, path: path, libraryDir: dir))
            }
        }
        guard found.isEmpty else { return found }
        return markdownFiles(of: dir)
            .filter { !docFileNames.contains($0.lowercased()) }
            .sorted()
            .map { AgentEntry(fileName: $0, path: (dir as NSString).appendingPathComponent($0), libraryDir: dir) }
    }

    // MARK: - Disk helpers

    /// Non-hidden subfolder names directly under dir
    static func subfolders(of dir: String) -> [String] {
        let fm = FileManager.default
        guard let names = try? fm.contentsOfDirectory(atPath: dir) else { return [] }
        return names.filter { name in
            guard !name.hasPrefix(".") else { return false }
            var isDir: ObjCBool = false
            let full = (dir as NSString).appendingPathComponent(name)
            return fm.fileExists(atPath: full, isDirectory: &isDir) && isDir.boolValue
        }
    }

    /// Non-hidden top-level `.md` file names directly under dir
    static func markdownFiles(of dir: String) -> [String] {
        let fm = FileManager.default
        guard let names = try? fm.contentsOfDirectory(atPath: dir) else { return [] }
        return names.filter { name in
            guard !name.hasPrefix("."), name.lowercased().hasSuffix(".md") else { return false }
            var isDir: ObjCBool = false
            let full = (dir as NSString).appendingPathComponent(name)
            return fm.fileExists(atPath: full, isDirectory: &isDir) && !isDir.boolValue
        }
    }

    private static func exists(_ path: String) -> Bool {
        FileManager.default.fileExists(atPath: path)
    }

    /// Whether the file opens with a `---` front-matter fence. Only the head of the file is read: an
    /// agent that does not declare front matter in its first few lines does not have any.
    private static func hasFrontMatter(at path: String) -> Bool {
        guard let handle = FileHandle(forReadingAtPath: path) else { return false }
        defer { try? handle.close() }
        guard let data = try? handle.read(upToCount: 512), let head = String(data: data, encoding: .utf8) else {
            return false
        }
        for line in head.components(separatedBy: .newlines) {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.isEmpty { continue }
            return trimmed == "---"
        }
        return false
    }

    /// nil for a missing or malformed file — a broken manifest just means "no plugin here", which leaves
    /// the folder's skills and agents loadable instead of failing the whole panel.
    private static func readObject(at path: String) -> [String: Any]? {
        guard FileManager.default.fileExists(atPath: path),
              let data = try? Data(contentsOf: URL(fileURLWithPath: path)),
              let object = try? JSONSerialization.jsonObject(with: data),
              let dict = object as? [String: Any] else { return nil }
        return dict
    }
}

/// Who owns a given name, walking the added folders in order.
///
/// A skill, an agent and a plugin all end up under a single project-wide name (`.claude/skills/<name>`,
/// `.claude/agents/<name>.md`, one `enabledPlugins` key), so two folders offering the same name cannot
/// both be loaded. The first folder to offer it owns it; every later one is a duplicate, and the panel
/// says so instead of silently picking a winner.
struct LoaderOwnership {
    private var owners: [String: String] = [:]

    /// Returns nil when `dir` now owns the name, or the folder that already owned it
    mutating func claim(kind: String, key: String, by dir: String) -> String? {
        let id = "\(kind)/\(key)"
        if let existing = owners[id] { return existing }
        owners[id] = dir
        return nil
    }

    func isClaimed(kind: String, key: String) -> Bool { owners["\(kind)/\(key)"] != nil }
}
