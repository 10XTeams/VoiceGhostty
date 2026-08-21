import SwiftUI
import AppKit

/// Loader panel behind the toolbar's 🧩 button. Two pages under one segmented control, because the two
/// things it loads are stored in completely different places: "Skill / Agent" copies folders and files
/// into the project's `.claude/`, while "Plugin" only flips switches in the project's settings.json.
struct SkillPanelView: View {
    let currentDirectory: String?
    var onDone: () -> Void = {}
    /// Run a command in the active terminal after a successful sync (e.g. /reload-skills); nil means do not run
    var runInTerminal: ((String) -> Void)? = nil

    @ObservedObject private var loc = Loc.shared
    @State private var tab: Tab = .skills

    private enum Tab: Hashable { case skills, plugins }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Picker("", selection: $tab) {
                Text(loc("Skill / Agent", "技能 / Agent")).tag(Tab.skills)
                Text(loc("Plugin", "插件")).tag(Tab.plugins)
            }
            .pickerStyle(.segmented)
            .labelsHidden()

            switch tab {
            case .skills:
                SkillSyncView(currentDirectory: currentDirectory,
                              onDone: onDone,
                              runInTerminal: runInTerminal)
            case .plugins:
                PluginSyncView(currentDirectory: currentDirectory, onDone: onDone)
            }
        }
        .padding(12)
        .frame(width: 360)
    }
}

// MARK: - Shared group header

/// Clickable group header shared by both pages: chevron + title + "checked/total", the counts being the
/// reason a collapsed group is still readable at a glance.
struct LoaderGroupHeader: View {
    let title: String
    let tooltip: String
    let checkedCount: Int
    let totalCount: Int
    let isCollapsed: Bool
    let toggle: () -> Void

    var body: some View {
        Button(action: toggle) {
            HStack(spacing: 4) {
                Image(systemName: isCollapsed ? "chevron.right" : "chevron.down")
                    .font(.caption2)
                    .frame(width: 10)
                Text(title)
                    .font(.caption).bold()
                    .lineLimit(1).truncationMode(.middle)
                Text("\(checkedCount)/\(totalCount)")
                    .font(.caption2)
                Spacer()
            }
            .foregroundStyle(.secondary)
            .contentShape(Rectangle())   // The whole header row is the hit target, not just the text
        }
        .buttonStyle(.plain)
        .help(tooltip)
    }
}

// MARK: - Skill / Agent page

/// A checkbox synchronizer between the skill libraries (source) ↔ the current project's `.claude/`.
/// - Each library folder contributes its subfolders as **skills** and its top-level `.md` files as **agents**
/// - Items already present in the current terminal's cwd are checked by default
/// - Click "Apply": newly checked → copy from the library into `.claude/skills/` or `.claude/agents/`;
///   unchecked → delete the corresponding item under cwd
/// The library paths are chosen in the Settings panel (stored in UserDefaults); if never chosen,
/// they fall back to the config's skill-library-dir (default ~/.claude/skills).
struct SkillSyncView: View {
    let currentDirectory: String?
    var onDone: () -> Void = {}
    var runInTerminal: ((String) -> Void)? = nil

    @ObservedObject private var loc = Loc.shared
    @State private var libraryDirs: [String] = AppSettings.skillLibraryDirs
    @State private var skills: [SkillItem] = []
    @State private var statusText = ""
    @State private var loadError: String?
    /// Ids of the collapsed groups (default: everything expanded); persisted so a folded-away library stays folded
    @State private var collapsed: Set<String> = AppSettings.collapsedSkillGroups

    /// What an item is decides where it lands in the project — the two kinds never share a target folder
    enum ItemKind: String { case skill, agent }

    struct SkillItem: Identifiable {
        /// Name on disk: the folder name for a skill, `<name>.md` for an agent
        let fileName: String
        let kind: ItemKind
        /// The library folder this item is copied from; nil = local-only (present in the project but in no
        /// library — once unchecked and deleted, it cannot be re-checked)
        let source: String?
        let loaded: Bool         // Whether it was already loaded in cwd when the panel opened
        var checked: Bool
        /// Displayed name — an agent's `.md` suffix is noise once the row carries an icon
        var name: String {
            kind == .agent ? (fileName as NSString).deletingPathExtension : fileName
        }
        // Kind is part of the identity: a skill folder and an agent file may well share a name
        var id: String { "\(kind.rawValue)/\(fileName)" }
    }

    /// Where a kind syncs to inside the project
    private func targetDir(for kind: ItemKind) -> String? {
        guard let cwd = currentDirectory else { return nil }
        let sub = kind == .skill ? ".claude/skills" : ".claude/agents"
        return (cwd as NSString).appendingPathComponent(sub)
    }

    /// Whether the checked state has changed relative to the initial state (decides whether "Apply" is clickable)
    private var hasChanges: Bool {
        skills.contains { $0.checked != $0.loaded }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                if let cwd = currentDirectory {
                    Text(loc("Current directory: ", "当前目录:") + (cwd as NSString).abbreviatingWithTildeInPath)
                        .font(.caption).foregroundStyle(.secondary)
                        .lineLimit(1).truncationMode(.middle)
                        .help(loc("Skills sync to ", "技能同步到 ") + "\(cwd)/.claude/skills/, "
                              + loc("agents to ", "Agent 同步到 ") + "\(cwd)/.claude/agents/")
                }
                Spacer()
                if groups.count > 1 {
                    Button {
                        setAllCollapsed(false)
                    } label: {
                        Image(systemName: "chevron.down.square")
                    }
                    .buttonStyle(.plain)
                    // Judged against the groups on screen, not the stored set (which may hold ids of libraries since removed)
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

            if skills.isEmpty {
                Text(loc("Nothing to show — the folders you added are either empty or plugin folders (those live on the Plugin page), and the current directory has nothing loaded.",
                         "没有可显示的内容 —— 已添加的文件夹要么是空的,要么是插件文件夹(那些在「插件」页),且当前目录也没加载任何东西。"))
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
                                skillRows(in: group)
                            }
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.vertical, 2)
                }
                // Inside a popover, a ScrollView with no intrinsic height collapses to 0, so height must be set
                // explicitly by row count — collapsed groups contribute their header only
                .frame(height: min(CGFloat(visibleRowCount) * 24 + CGFloat(groups.count) * 24 + 20, 300))
                Divider()
            }

            if !statusText.isEmpty {
                Text(statusText).font(.caption).foregroundStyle(.secondary)
            }

            HStack {
                Spacer()
                Button(loc("Cancel", "取消")) { onDone() }
                    .keyboardShortcut(.cancelAction)
                Button(loc("Apply", "应用")) { apply() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(!hasChanges || currentDirectory == nil)
            }
        }
        .onAppear { reload() }
    }

    // MARK: - Grouped rows (one group per library folder, in the configured order; local-only items go last)

    private struct SkillGroup: Identifiable {
        let id: String          // The library path, or "" for the local-only group
        let title: String
        let tooltip: String
    }

    /// Only groups that actually have items to show; a library folder contributing nothing is skipped
    private var groups: [SkillGroup] {
        var result = libraryDirs
            .filter { dir in skills.contains { $0.source == dir } }
            .map { SkillGroup(id: $0, title: groupTitle(for: $0), tooltip: $0) }
        if skills.contains(where: { $0.source == nil }) {
            result.append(SkillGroup(id: "",
                                     title: loc("Local only", "仅本地"),
                                     tooltip: loc("In the project but in no library — unchecking deletes it for good",
                                                  "项目里有但不属于任何技能库 —— 取消勾选即永久删除")))
        }
        return result
    }

    /// Rows actually laid out: a collapsed group shows its header only
    private var visibleRowCount: Int {
        skills.filter { !collapsed.contains($0.source ?? "") }.count
    }

    private func groupHeader(_ group: SkillGroup) -> some View {
        let items = skills.filter { ($0.source ?? "") == group.id }
        return LoaderGroupHeader(title: group.title,
                                 tooltip: group.tooltip,
                                 checkedCount: items.filter(\.checked).count,
                                 totalCount: items.count,
                                 isCollapsed: collapsed.contains(group.id)) {
            if collapsed.contains(group.id) { collapsed.remove(group.id) } else { collapsed.insert(group.id) }
            AppSettings.collapsedSkillGroups = collapsed
        }
    }

    private func setAllCollapsed(_ shouldCollapse: Bool) {
        collapsed = shouldCollapse ? Set(groups.map(\.id)) : []
        AppSettings.collapsedSkillGroups = collapsed
    }

    /// The folder's own name, unless another library ends in the same name — then the full (~-abbreviated) path
    private func groupTitle(for dir: String) -> String {
        let name = (dir as NSString).lastPathComponent
        let collides = libraryDirs.contains { $0 != dir && ($0 as NSString).lastPathComponent == name }
        return collides ? (dir as NSString).abbreviatingWithTildeInPath : name
    }

    private func skillRows(in group: SkillGroup) -> some View {
        ForEach($skills) { $item in
            if (item.source ?? "") == group.id {
                Toggle(isOn: $item.checked) {
                    HStack(spacing: 4) {
                        Image(systemName: item.kind == .agent ? "person.fill" : "puzzlepiece.extension.fill")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                            .frame(width: 12)
                        Text(item.name).lineLimit(1)
                    }
                }
                .toggleStyle(.checkbox)
                .padding(.leading, 8)
                .help(item.kind == .agent
                      ? loc("Agent → .claude/agents/", "Agent → .claude/agents/") + item.fileName
                      : loc("Skill → .claude/skills/", "技能 → .claude/skills/") + item.fileName)
            }
        }
    }

    // MARK: - Scanning

    /// List the non-hidden subfolder names under dir (each subfolder = one skill)
    private static func subfolders(of dir: String) -> [String] {
        let fm = FileManager.default
        guard let names = try? fm.contentsOfDirectory(atPath: dir) else { return [] }
        return names.filter { name in
            guard !name.hasPrefix(".") else { return false }
            var isDir: ObjCBool = false
            let full = (dir as NSString).appendingPathComponent(name)
            return fm.fileExists(atPath: full, isDirectory: &isDir) && isDir.boolValue
        }
    }

    /// List the non-hidden top-level `.md` file names under dir (each file = one agent)
    private static func markdownFiles(of dir: String) -> [String] {
        let fm = FileManager.default
        guard let names = try? fm.contentsOfDirectory(atPath: dir) else { return [] }
        return names.filter { name in
            guard !name.hasPrefix("."), name.lowercased().hasSuffix(".md") else { return false }
            var isDir: ObjCBool = false
            let full = (dir as NSString).appendingPathComponent(name)
            return fm.fileExists(atPath: full, isDirectory: &isDir) && !isDir.boolValue
        }
    }

    private func reload() {
        loadError = nil
        LoaderLibrary.invalidateCache()
        let configured = AppSettings.skillLibraryDirs
        let missing = configured.filter { !FileManager.default.fileExists(atPath: $0) }
        // A folder carrying a plugin manifest belongs to the Plugin page. Its subfolders are plugin
        // sources, not skills — listing them here would fill the panel with things nothing can load.
        libraryDirs = configured.filter {
            FileManager.default.fileExists(atPath: $0) && LoaderLibrary.classify($0).isSkillLibrary
        }
        if currentDirectory == nil {
            loadError = loc("Unable to determine the current terminal's working directory",
                            "无法确定当前终端的工作目录")
        } else if !missing.isEmpty {
            // A missing library is only a warning; still list the other libraries and the loaded skills
            loadError = loc("Skill library does not exist: ", "技能库不存在:")
                + missing.map { ($0 as NSString).abbreviatingWithTildeInPath }.joined(separator: ", ")
        }

        let loadedSkills = Set(targetDir(for: .skill).map(Self.subfolders(of:)) ?? [])
        let loadedAgents = Set(targetDir(for: .agent).map(Self.markdownFiles(of:)) ?? [])
        var items: [SkillItem] = []
        // Claimed per kind: the two land in different folders, so a shared name is not a conflict
        var claimedSkills = Set<String>()
        var claimedAgents = Set<String>()
        // One group per library, in configured order; a name in two libraries belongs to the first one,
        // since the target is a single .claude/skills/<name> (resp. .claude/agents/<name>.md)
        for dir in libraryDirs {
            for name in Self.subfolders(of: dir).sorted() where claimedSkills.insert(name).inserted {
                items.append(SkillItem(fileName: name, kind: .skill, source: dir,
                                       loaded: loadedSkills.contains(name),
                                       checked: loadedSkills.contains(name)))
            }
            for name in Self.markdownFiles(of: dir).sorted() where claimedAgents.insert(name).inserted {
                items.append(SkillItem(fileName: name, kind: .agent, source: dir,
                                       loaded: loadedAgents.contains(name),
                                       checked: loadedAgents.contains(name)))
            }
        }
        // Local-only: loaded in the project but in no library (deleted once unchecked)
        for name in loadedSkills.subtracting(claimedSkills).sorted() {
            items.append(SkillItem(fileName: name, kind: .skill, source: nil, loaded: true, checked: true))
        }
        for name in loadedAgents.subtracting(claimedAgents).sorted() {
            items.append(SkillItem(fileName: name, kind: .agent, source: nil, loaded: true, checked: true))
        }
        skills = items
    }

    // MARK: - Apply (copy / delete)

    private func apply() {
        guard currentDirectory != nil else { return }
        let fm = FileManager.default
        var copied = 0, removed = 0, failures: [String] = []

        for item in skills where item.checked != item.loaded {
            guard let dir = targetDir(for: item.kind) else { continue }
            let target = (dir as NSString).appendingPathComponent(item.fileName)
            do {
                if item.checked {
                    // Newly checked → copy from its library (skip if the target already exists, do not overwrite)
                    guard let library = item.source else { continue }   // Local-only: nothing to copy back from
                    guard !fm.fileExists(atPath: target) else { continue }
                    try fm.createDirectory(atPath: dir, withIntermediateDirectories: true)
                    let source = (library as NSString).appendingPathComponent(item.fileName)
                    try fm.copyItem(atPath: source, toPath: target)
                    copied += 1
                } else if fm.fileExists(atPath: target) {
                    // Unchecked → delete only this item under its target dir, touching nothing else
                    try fm.removeItem(atPath: target)
                    removed += 1
                }
            } catch {
                failures.append("\(item.name): \(error.localizedDescription)")
            }
        }

        if failures.isEmpty {
            statusText = loc("Synced: copied ", "已同步:复制 ") + "\(copied)"
                + loc(", deleted ", ",删除 ") + "\(removed)"
            // Notify Claude in the terminal to reload skills when there were actual changes
            if copied + removed > 0, let run = runInTerminal {
                run("/reload-skills")
                statusText += loc(", sent /reload-skills", ",已发送 /reload-skills")
            }
        } else {
            statusText = loc("Partial failure — ", "部分失败 —— ") + failures.joined(separator: "; ")
        }
        reload()   // Refresh checkboxes from the actual on-disk state
    }
}
