import SwiftUI
import AppKit

/// The loader's "Plugin" page.
///
/// Its source of truth is the same folder list as the Skill page — whichever of those folders turn out to
/// carry a plugin manifest land here instead of there. Nothing global is read or written: what a plugin
/// folder needs to become loadable in a project is a marketplace registration plus a switch, and both go
/// into the project's own `.claude/settings.local.json`.
///
/// `settings.local.json` rather than `settings.json` on purpose: the registration has to name the plugin
/// folder by absolute path, which is a fact about this machine and has no business in a file shared
/// through the repo.
struct PluginSyncView: View {
    let currentDirectory: String?
    var onDone: () -> Void = {}

    @ObservedObject private var loc = Loc.shared
    @State private var groups: [PluginGroup] = []
    @State private var plugins: [PluginItem] = []
    @State private var statusText = ""
    @State private var loadError: String?
    @State private var collapsed: Set<String> = AppSettings.collapsedPluginGroups

    /// One group per added plugin folder, plus a trailing group for ids the project enables that no
    /// added folder accounts for
    struct PluginGroup: Identifiable {
        let id: String            // Marketplace name, or "" for the orphan group
        let title: String
        let tooltip: String
        /// The folder to register in extraKnownMarketplaces; nil for the orphan group
        let libraryDir: String?
    }

    struct PluginItem: Identifiable {
        let id: String            // "name@marketplace"
        let name: String
        let groupID: String
        let detail: String
        /// A bare plugin folder declares no marketplace, so project settings have no way to name it
        let loadable: Bool
        let loaded: Bool          // Enabled in the project file when the panel opened
        var checked: Bool
    }

    private var settingsPath: String? {
        guard let cwd = currentDirectory else { return nil }
        return (cwd as NSString).appendingPathComponent(".claude/settings.local.json")
    }

    private var hasChanges: Bool {
        plugins.contains { $0.checked != $0.loaded }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                if let path = settingsPath {
                    Text(loc("Writes to ", "写入 ") + (path as NSString).abbreviatingWithTildeInPath)
                        .font(.caption).foregroundStyle(.secondary)
                        .lineLimit(1).truncationMode(.middle)
                        .help(loc("Enabling a plugin also registers its folder as a marketplace in that file. It stays out of settings.json because the folder is an absolute path on this machine.",
                                  "启用插件时会同时把它所在的文件夹注册成 marketplace 写进这个文件。不写进 settings.json,是因为那是本机的绝对路径。"))
                }
                Spacer()
                if groups.count > 1 {
                    Button {
                        setAllCollapsed(false)
                    } label: {
                        Image(systemName: "chevron.down.square")
                    }
                    .buttonStyle(.plain)
                    .disabled(groups.allSatisfy { !collapsed.contains($0.id) })
                    .help(loc("Expand all", "全部展开"))

                    Button {
                        setAllCollapsed(true)
                    } label: {
                        Image(systemName: "chevron.right.square")
                    }
                    .buttonStyle(.plain)
                    .disabled(groups.allSatisfy { collapsed.contains($0.id) })
                    .help(loc("Collapse all", "全部收起"))
                }
                Button {
                    reload()
                } label: {
                    Image(systemName: "arrow.clockwise")
                }
                .buttonStyle(.plain)
                .help(loc("Rescan", "重新扫描"))
            }

            if let err = loadError {
                Text(err).font(.caption).foregroundStyle(.orange)
            }

            if plugins.isEmpty {
                Text(loc("No plugin folder added yet — add one in Settings. A folder counts as a plugin when it has .claude-plugin/marketplace.json (or plugin.json).",
                         "还没有添加插件文件夹 —— 去设置里添加。文件夹带 .claude-plugin/marketplace.json(或 plugin.json)才算插件。"))
                    .font(.caption).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            } else {
                Divider()
                ScrollView {
                    VStack(alignment: .leading, spacing: 6) {
                        ForEach(groups) { group in
                            groupHeader(group)
                                .padding(.top, group.id == groups.first?.id ? 0 : 4)
                            if !collapsed.contains(group.id) {
                                pluginRows(in: group)
                            }
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.vertical, 2)
                }
                // Same reason as the skill page: inside the floating panel a ScrollView has no intrinsic height
                .frame(height: min(CGFloat(visibleRowCount) * 24 + CGFloat(groups.count) * 24 + 20, 300))
                Divider()
            }

            if !statusText.isEmpty {
                Text(statusText).font(.caption).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            HStack {
                Spacer()
                Button(loc("Cancel", "取消")) { onDone() }
                    .keyboardShortcut(.cancelAction)
                Button(loc("Apply", "应用")) { apply() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(!hasChanges || settingsPath == nil)
            }
        }
        .onAppear { reload() }
    }

    // MARK: - Grouped rows

    private var visibleRowCount: Int {
        plugins.filter { !collapsed.contains($0.groupID) }.count
    }

    private func groupHeader(_ group: PluginGroup) -> some View {
        let items = plugins.filter { $0.groupID == group.id }
        return LoaderGroupHeader(title: group.title,
                                 tooltip: group.tooltip,
                                 checkedCount: items.filter(\.checked).count,
                                 totalCount: items.count,
                                 isCollapsed: collapsed.contains(group.id)) {
            if collapsed.contains(group.id) { collapsed.remove(group.id) } else { collapsed.insert(group.id) }
            AppSettings.collapsedPluginGroups = collapsed
        }
    }

    private func setAllCollapsed(_ shouldCollapse: Bool) {
        collapsed = shouldCollapse ? Set(groups.map(\.id)) : []
        AppSettings.collapsedPluginGroups = collapsed
    }

    private func pluginRows(in group: PluginGroup) -> some View {
        ForEach($plugins) { $item in
            if item.groupID == group.id {
                Toggle(isOn: $item.checked) {
                    HStack(spacing: 4) {
                        Image(systemName: item.loadable ? "shippingbox.fill" : "exclamationmark.triangle.fill")
                            .font(.caption2)
                            .foregroundStyle(item.loadable ? Color.secondary : Color.orange)
                            .frame(width: 12)
                        Text(item.name).lineLimit(1).truncationMode(.middle)
                    }
                }
                .toggleStyle(.checkbox)
                .disabled(!item.loadable)
                .padding(.leading, 8)
                .help(item.detail)
            }
        }
    }

    // MARK: - Scanning

    private func reload() {
        loadError = nil
        statusText = ""
        LoaderLibrary.invalidateCache()
        guard let settingsPath else {
            loadError = loc("Unable to determine the current terminal's working directory",
                            "无法确定当前终端的工作目录")
            groups = []; plugins = []
            return
        }

        let enabled: Set<String>
        do {
            enabled = try ClaudePluginStore.enabledPluginIDs(in: settingsPath)
        } catch {
            loadError = loc("Cannot read settings: ", "无法读取设置:") + error.localizedDescription
            groups = []; plugins = []
            return
        }

        var newGroups: [PluginGroup] = []
        var newItems: [PluginItem] = []
        var seen = Set<String>()

        for dir in AppSettings.skillLibraryDirs {
            let kind = LoaderLibrary.classify(dir)
            guard !kind.isSkillLibrary else { continue }

            switch kind {
            case .skills:
                continue
            case .marketplace(let name, let entries):
                newGroups.append(PluginGroup(id: name,
                                             title: name,
                                             tooltip: dir,
                                             libraryDir: dir))
                for entry in entries where seen.insert(entry.id).inserted {
                    newItems.append(PluginItem(id: entry.id,
                                               name: entry.name,
                                               groupID: name,
                                               detail: entry.description.isEmpty ? entry.id : entry.description,
                                               loadable: true,
                                               loaded: enabled.contains(entry.id),
                                               checked: enabled.contains(entry.id)))
                }
            case .barePlugin(let entry):
                let groupID = "!bare/" + dir
                newGroups.append(PluginGroup(id: groupID,
                                             title: (dir as NSString).lastPathComponent,
                                             tooltip: dir,
                                             libraryDir: nil))
                newItems.append(PluginItem(id: entry.id,
                                           name: entry.name,
                                           groupID: groupID,
                                           detail: loc("No .claude-plugin/marketplace.json in this folder, so project settings have no way to name it. Point the loader at the marketplace folder that lists this plugin instead.",
                                                       "这个文件夹里没有 .claude-plugin/marketplace.json,项目设置无法引用它。请改为添加收录了这个插件的 marketplace 文件夹。"),
                                           loadable: false,
                                           loaded: false,
                                           checked: false))
            }
        }

        // Ids the project already enables that no added folder accounts for. Listed last so they stay
        // switchable — an entry left behind by a folder since removed would otherwise be invisible.
        let orphans = enabled.subtracting(seen).sorted()
        if !orphans.isEmpty {
            newGroups.append(PluginGroup(id: "",
                                         title: loc("Not from an added folder", "非来自已添加文件夹"),
                                         tooltip: loc("Enabled in this project, but no added folder offers it — unchecking removes the entry",
                                                      "项目里启用了,但没有任何已添加的文件夹提供它 —— 取消勾选即移除该条目"),
                                         libraryDir: nil))
            for id in orphans {
                newItems.append(PluginItem(id: id,
                                           name: id,
                                           groupID: "",
                                           detail: id,
                                           loadable: true,
                                           loaded: true,
                                           checked: true))
            }
        }

        groups = newGroups
        plugins = newItems
    }

    // MARK: - Apply

    private func apply() {
        guard let settingsPath else { return }
        var enabled = 0, disabled = 0
        // Only the marketplaces this panel knows about may be added or dropped; anything else in the
        // file was put there by someone else and is left alone.
        let managed = groups.compactMap { group -> (String, String)? in
            guard let dir = group.libraryDir else { return nil }
            return (group.id, dir)
        }
        let desired = Set(plugins.filter(\.checked).map(\.id))
        for item in plugins where item.checked != item.loaded {
            if item.checked { enabled += 1 } else { disabled += 1 }
        }

        do {
            try ClaudePluginStore.write(enabledIDs: desired,
                                        managedMarketplaces: managed,
                                        stillEnabled: { marketplace in
                                            desired.contains { $0.hasSuffix("@" + marketplace) }
                                        },
                                        to: settingsPath)
            statusText = loc("Written: enabled ", "已写入:启用 ") + "\(enabled)"
                + loc(", disabled ", ",停用 ") + "\(disabled)"
                + loc(" — restart the claude session to take effect", " —— 需重启 claude 会话生效")
        } catch {
            statusText = loc("Failed: ", "失败:") + error.localizedDescription
        }
        reload()
    }
}

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
