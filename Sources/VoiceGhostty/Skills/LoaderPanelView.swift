import SwiftUI
import AppKit

/// The loader behind the toolbar's 🧩 button.
///
/// Three tabs — Skill / Agent / Plugin — filled by scanning the folders added in Settings. A folder is
/// never asked *what it is*, only *what it has*, and each kind it holds lands in the matching tab. Add as
/// many folders as you like; inside a tab they stay one group per folder.
///
/// One exception to "whatever it has": a folder declaring a plugin contributes only that plugin, because
/// the skills and agents inside it are the plugin's and load with it.
///
/// The tabs are a view of one list, not three separate panels: checkboxes ticked in one tab survive
/// switching to another, and Apply commits every tab at once.
///
/// Applying does two unrelated things, because the two kinds of thing live in different places: skills and
/// agents are copied into the project's `.claude/`, while plugins are switched on in the project's
/// `.claude/settings.local.json` (`settings.local.json` rather than `settings.json` because registering a
/// plugin folder names it by absolute path — a fact about this machine, not about the repo).
///
/// A name can only be loaded once: `.claude/skills/<name>` is a single folder, and `enabledPlugins` a
/// single key. So the first folder offering a name owns it, and a later folder offering the same name gets
/// a disabled row saying who already provides it, rather than a silent coin toss.
struct LoaderPanelView: View {
    let currentDirectory: String?
    var onDone: () -> Void = {}
    /// Run a command in the active terminal after a successful sync (e.g. /reload-skills); nil means do not run
    var runInTerminal: ((String) -> Void)? = nil

    @ObservedObject private var loc = Loc.shared
    @State private var scans: [LoaderLibrary.Scan] = []
    @State private var items: [LoaderItem] = []
    @State private var statusText = ""
    @State private var loadError: String?
    /// Which groups are folded shut, keyed by "<tab>/<folder>"; persisted, so a folder you never load
    /// from stays out of the way
    @State private var collapsed: Set<String> = AppSettings.collapsedLoaderGroups
    @State private var tab: LoaderItem.Kind = .skill

    // MARK: - Model

    struct LoaderItem: Identifiable {
        enum Kind: String { case plugin, skill, agent }

        /// Why a row cannot be checked
        enum Blocker {
            /// Another added folder already provides this name (that folder's title)
            case duplicate(String)
            /// A bare plugin folder declares no marketplace, so project settings have no way to name it
            case noMarketplace
        }

        let kind: Kind
        /// What the item is called where it lands: skill folder name, `<agent>.md`, or `name@marketplace`
        let key: String
        let name: String
        /// The added folder it comes from; nil = already in the project but offered by no folder
        let sourceDir: String?
        /// Absolute source on disk (skills and agents only — a plugin is a settings entry, not a copy)
        let sourcePath: String?
        let detail: String
        let blocker: Blocker?
        let loaded: Bool          // Already loaded in the project when the panel opened
        var checked: Bool

        /// The source folder is part of the identity: a duplicate row shares kind and key with its owner
        var id: String { "\(kind.rawValue)/\(key)/\(sourceDir ?? "")" }
        var isBlocked: Bool { blocker != nil }
    }

    private var settingsPath: String? {
        guard let cwd = currentDirectory else { return nil }
        return (cwd as NSString).appendingPathComponent(".claude/settings.local.json")
    }

    /// Where a kind syncs to inside the project (nil for plugins, which are not copied anywhere)
    private func targetDir(for kind: LoaderItem.Kind) -> String? {
        guard let cwd = currentDirectory else { return nil }
        switch kind {
        case .skill:  return (cwd as NSString).appendingPathComponent(".claude/skills")
        case .agent:  return (cwd as NSString).appendingPathComponent(".claude/agents")
        case .plugin: return nil
        }
    }

    private var hasChanges: Bool { items.contains { $0.checked != $0.loaded } }

    /// Duplicates in the tab on screen — the number the warning line reports
    private var blockedCount: Int {
        tabItems.filter { if case .duplicate = $0.blocker { return true } else { return false } }.count
    }

    /// Tabs in the order they are shown; a kind with nothing in it still gets its tab, so an empty one
    /// says why it is empty instead of silently vanishing and shifting the others sideways
    private static let tabOrder: [LoaderItem.Kind] = [.skill, .agent, .plugin]

    private var tabItems: [LoaderItem] { items.filter { $0.kind == tab } }

    // MARK: - Body

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Picker("", selection: $tab) {
                ForEach(Self.tabOrder, id: \.self) { kind in
                    Text(tabLabel(kind)).tag(kind)
                }
            }
            .pickerStyle(.segmented)
            .labelsHidden()

            header

            if let err = loadError {
                Text(err).font(.caption).foregroundStyle(.orange)
                    .fixedSize(horizontal: false, vertical: true)
            }

            if blockedCount > 0 {
                Label(loc("\(blockedCount) duplicate name(s) skipped — a name can only be loaded from one folder",
                          "\(blockedCount) 项重名已跳过 —— 同一个名字只能由一个文件夹提供"),
                      systemImage: "exclamationmark.triangle.fill")
                    .font(.caption).foregroundStyle(.orange)
                    .fixedSize(horizontal: false, vertical: true)
            }

            if tabItems.isEmpty {
                Text(emptyMessage)
                    .font(.caption).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .leading)
            } else {
                Divider()
                ScrollView {
                    VStack(alignment: .leading, spacing: 6) {
                        ForEach(groups) { group in
                            groupHeader(group)
                                .padding(.top, group.id == groups.first?.id ? 0 : 4)
                            if !collapsed.contains(group.id) {
                                rows(in: group)
                            }
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.vertical, 2)
                }
                // Inside a floating panel a ScrollView with no intrinsic height collapses to 0, so the
                // height is set explicitly by row count — a collapsed group contributes its header only
                .frame(height: min(CGFloat(visibleRowCount) * 24 + CGFloat(groups.count) * 24 + 20, 320))
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
                    .disabled(!hasChanges || currentDirectory == nil)
                    .help(loc("Applies every tab at once, not just the one on screen",
                              "三个页签的改动一起应用,不只是当前这一页"))
            }
        }
        .padding(12)
        .frame(width: 380)
        .onAppear { reload() }
    }

    /// Tab title with what it holds — the count is what makes an unvisited tab worth visiting
    private func tabLabel(_ kind: LoaderItem.Kind) -> String {
        let count = items.filter { $0.kind == kind }.count
        let name: String
        switch kind {
        case .skill:  name = loc("Skill", "技能")
        case .agent:  name = loc("Agent", "Agent")
        case .plugin: name = loc("Plugin", "插件")
        }
        return count > 0 ? "\(name) \(count)" : name
    }

    /// Why the tab on screen has nothing in it — each kind is found by a different marker, so a blanket
    /// "nothing here" would leave you guessing which one your folder is missing
    private var emptyMessage: String {
        guard !scans.isEmpty else {
            return loc("No folder added — add one in Settings and its skills, agents and plugins are detected automatically.",
                       "还没有添加文件夹 —— 去设置里添加,里面的技能、Agent、插件会被自动识别。")
        }
        switch tab {
        case .skill:
            return loc("No skill found in the added folders. A skill is a folder holding SKILL.md, directly in the folder you added or under its skills/ — and a plugin folder is not searched at all, since its skills load with the plugin.",
                       "已添加的文件夹里没有识别到技能。技能是带 SKILL.md 的文件夹,可以在添加的文件夹本层,也可以在它的 skills/ 下 —— 插件文件夹不在这里扫描,它带的技能随插件一起加载。")
        case .agent:
            return loc("No agent found in the added folders. An agent is a .md file opening with YAML front matter, directly in the folder you added or under its agents/ — and a plugin folder is not searched at all, since its agents load with the plugin.",
                       "已添加的文件夹里没有识别到 Agent。Agent 是以 YAML front matter 开头的 .md 文件,可以在添加的文件夹本层,也可以在它的 agents/ 下 —— 插件文件夹不在这里扫描,它带的 Agent 随插件一起加载。")
        case .plugin:
            return loc("No plugin found in the added folders. A plugin folder carries .claude-plugin/marketplace.json (or plugin.json).",
                       "已添加的文件夹里没有识别到插件。插件文件夹带 .claude-plugin/marketplace.json(或 plugin.json)。")
        }
    }

    private var header: some View {
        HStack {
            if let cwd = currentDirectory {
                Text(loc("Current directory: ", "当前目录:") + (cwd as NSString).abbreviatingWithTildeInPath)
                    .font(.caption).foregroundStyle(.secondary)
                    .lineLimit(1).truncationMode(.middle)
                    .help(targetsTooltip(cwd))
            }
            Spacer()
            if groups.count > 1 {
                Button { setAllCollapsed(false) } label: { Image(systemName: "chevron.down.square") }
                    .buttonStyle(.plain)
                    // Judged against the groups on screen, not the stored set (which may hold stale ids)
                    .disabled(groups.allSatisfy { !collapsed.contains($0.id) })
                    .help(loc("Expand all", "全部展开"))

                Button { setAllCollapsed(true) } label: { Image(systemName: "chevron.right.square") }
                    .buttonStyle(.plain)
                    .disabled(groups.allSatisfy { collapsed.contains($0.id) })
                    .help(loc("Collapse all", "全部收起"))
            }
            Button { reload() } label: { Image(systemName: "arrow.clockwise") }
                .buttonStyle(.plain)
                .help(loc("Rescan", "重新扫描"))
        }
    }

    /// Where each of the three kinds ends up in the project
    private func targetsTooltip(_ cwd: String) -> String {
        let skills: String = loc("Skills → ", "技能 → ") + cwd + "/.claude/skills/"
        let agents: String = loc("Agents → ", "Agent → ") + cwd + "/.claude/agents/"
        let plugins: String = loc("Plugins → ", "插件 → ") + cwd + "/.claude/settings.local.json"
        return [skills, agents, plugins].joined(separator: "\n")
    }

    // MARK: - Groups inside a tab (one per folder, in the configured order; project-only items go last)

    private struct Group: Identifiable {
        /// "<tab>/<folder>" — the tab is part of it so folding a folder shut in one tab leaves it open
        /// in the others, where it holds something else entirely
        let id: String
        /// nil for the group of items the project has but no added folder offers
        let dir: String?
        let title: String
        let tooltip: String
    }

    private var groups: [Group] {
        let mine = tabItems
        var result = scans.compactMap { scan -> Group? in
            guard mine.contains(where: { $0.sourceDir == scan.dir }) else { return nil }
            return Group(id: "\(tab.rawValue)/\(scan.dir)",
                         dir: scan.dir,
                         title: folderTitle(for: scan.dir),
                         tooltip: scan.dir)
        }
        if mine.contains(where: { $0.sourceDir == nil }) {
            result.append(Group(id: "\(tab.rawValue)/",
                                dir: nil,
                                title: loc("In the project only", "仅项目内"),
                                tooltip: orphanNote))
        }
        return result
    }

    /// Rows actually laid out in this tab: a collapsed group shows its header only
    private var visibleRowCount: Int {
        tabItems.filter { !collapsed.contains("\(tab.rawValue)/\($0.sourceDir ?? "")") }.count
    }

    private func groupHeader(_ group: Group) -> some View {
        let members = tabItems.filter { $0.sourceDir == group.dir }
        return LoaderGroupHeader(title: group.title,
                                 tooltip: group.tooltip,
                                 checkedCount: members.filter(\.checked).count,
                                 totalCount: members.count,
                                 isCollapsed: collapsed.contains(group.id)) {
            if collapsed.contains(group.id) { collapsed.remove(group.id) } else { collapsed.insert(group.id) }
            AppSettings.collapsedLoaderGroups = collapsed
        }
    }

    /// Expand/collapse all applies to the tab on screen; the other tabs keep their own folding
    private func setAllCollapsed(_ shouldCollapse: Bool) {
        let ids = Set(groups.map(\.id))
        collapsed = shouldCollapse ? collapsed.union(ids) : collapsed.subtracting(ids)
        AppSettings.collapsedLoaderGroups = collapsed
    }

    private func rows(in group: Group) -> some View {
        ForEach($items) { $item in
            if item.kind == tab && item.sourceDir == group.dir {
                Toggle(isOn: $item.checked) {
                    HStack(spacing: 4) {
                        Image(systemName: icon(for: item))
                            .font(.caption2)
                            .foregroundStyle(item.isBlocked ? Color.orange : Color.secondary)
                            .frame(width: 12)
                        Text(item.name)
                            .lineLimit(1).truncationMode(.middle)
                            .foregroundStyle(item.isBlocked ? Color.secondary : Color.primary)
                        if let badge = badge(for: item) {
                            Spacer(minLength: 6)
                            Text(badge)
                                .font(.caption2).foregroundStyle(.orange)
                        }
                    }
                }
                .toggleStyle(LoaderCheckboxStyle())
                .disabled(item.isBlocked)
                .padding(.leading, 8)
                .padding(.trailing, 4)
                .help(item.detail)
            }
        }
    }

    private func icon(for item: LoaderItem) -> String {
        if item.isBlocked { return "exclamationmark.triangle.fill" }
        switch item.kind {
        case .plugin: return "shippingbox.fill"
        case .skill:  return "puzzlepiece.extension.fill"
        case .agent:  return "person.fill"
        }
    }

    /// Trailing note on a row: only why it cannot be checked. Which folder it came from is the group
    /// header's job, so repeating it on every row would be noise.
    private func badge(for item: LoaderItem) -> String? {
        switch item.blocker {
        case .duplicate:      return loc("duplicate", "重名")
        case .noMarketplace:  return loc("no marketplace", "无 marketplace")
        case nil:             return nil
        }
    }

    /// The folder's own name, unless another added folder ends in the same name — then the full path
    private func folderTitle(for dir: String) -> String {
        let name = (dir as NSString).lastPathComponent
        let collides = scans.contains { $0.dir != dir && ($0.dir as NSString).lastPathComponent == name }
        return collides ? (dir as NSString).abbreviatingWithTildeInPath : name
    }

    /// Said by both the group header and every row in it
    private var orphanNote: String {
        loc("In the project, but no added folder offers it — unchecking removes it for good",
            "项目里有,但没有任何已添加的文件夹提供它 —— 取消勾选即永久移除")
    }

    // MARK: - Scanning

    private func reload() {
        loadError = nil
        statusText = ""
        LoaderLibrary.invalidateCache()

        let configured = AppSettings.skillLibraryDirs
        let missing = configured.filter { !FileManager.default.fileExists(atPath: $0) }
        scans = LoaderLibrary.scanAll(configured)

        guard currentDirectory != nil, let settingsPath else {
            loadError = loc("Unable to determine the current terminal's working directory",
                            "无法确定当前终端的工作目录")
            items = []
            return
        }
        if !missing.isEmpty {
            // A missing folder is only a warning; the other folders and the loaded items still list
            loadError = loc("Folder does not exist: ", "文件夹不存在:")
                + missing.map { ($0 as NSString).abbreviatingWithTildeInPath }.joined(separator: ", ")
        }

        var enabledPlugins: Set<String> = []
        do {
            enabledPlugins = try ClaudePluginStore.enabledPluginIDs(in: settingsPath)
        } catch {
            // A hand-edited settings file with a syntax error: say so, and leave the plugin rows off
            // rather than offering switches that Apply would refuse to write
            loadError = loc("Cannot read settings: ", "无法读取设置:") + error.localizedDescription
        }

        let loadedSkills = Set(targetDir(for: .skill).map(LoaderLibrary.subfolders(of:)) ?? [])
        let loadedAgents = Set(targetDir(for: .agent).map(LoaderLibrary.markdownFiles(of:)) ?? [])

        var result: [LoaderItem] = []
        var ownership = LoaderOwnership()

        for scan in scans {
            for plugin in scan.plugins {
                let blocker: LoaderItem.Blocker?
                if let owner = ownership.claim(kind: "plugin", key: plugin.name, by: scan.dir) {
                    blocker = .duplicate(folderTitle(for: owner))
                } else if plugin.marketplace.isEmpty {
                    blocker = .noMarketplace
                } else {
                    blocker = nil
                }
                let isLoaded = blocker == nil && enabledPlugins.contains(plugin.id)
                result.append(LoaderItem(kind: .plugin,
                                         key: plugin.id,
                                         name: plugin.name,
                                         sourceDir: scan.dir,
                                         sourcePath: nil,
                                         detail: detail(for: blocker) ?? (plugin.description.isEmpty ? plugin.id : plugin.description),
                                         blocker: blocker,
                                         loaded: isLoaded,
                                         checked: isLoaded))
            }

            for skill in scan.skills {
                let blocker = ownership.claim(kind: "skill", key: skill.name, by: scan.dir)
                    .map { LoaderItem.Blocker.duplicate(folderTitle(for: $0)) }
                let isLoaded = blocker == nil && loadedSkills.contains(skill.name)
                result.append(LoaderItem(kind: .skill,
                                         key: skill.name,
                                         name: skill.name,
                                         sourceDir: scan.dir,
                                         sourcePath: skill.path,
                                         detail: detail(for: blocker) ?? skill.path,
                                         blocker: blocker,
                                         loaded: isLoaded,
                                         checked: isLoaded))
            }

            for agent in scan.agents {
                let blocker = ownership.claim(kind: "agent", key: agent.fileName, by: scan.dir)
                    .map { LoaderItem.Blocker.duplicate(folderTitle(for: $0)) }
                let isLoaded = blocker == nil && loadedAgents.contains(agent.fileName)
                result.append(LoaderItem(kind: .agent,
                                         key: agent.fileName,
                                         name: agent.name,
                                         sourceDir: scan.dir,
                                         sourcePath: agent.path,
                                         detail: detail(for: blocker) ?? agent.path,
                                         blocker: blocker,
                                         loaded: isLoaded,
                                         checked: isLoaded))
            }
        }

        // Already in the project but offered by no added folder. Listed so they stay switchable —
        // something left behind by a folder since removed would otherwise be invisible.
        for name in loadedSkills.filter({ !ownership.isClaimed(kind: "skill", key: $0) }).sorted() {
            result.append(LoaderItem(kind: .skill, key: name, name: name, sourceDir: nil, sourcePath: nil,
                                     detail: orphanNote, blocker: nil, loaded: true, checked: true))
        }
        for name in loadedAgents.filter({ !ownership.isClaimed(kind: "agent", key: $0) }).sorted() {
            result.append(LoaderItem(kind: .agent, key: name,
                                     name: (name as NSString).deletingPathExtension,
                                     sourceDir: nil, sourcePath: nil,
                                     detail: orphanNote, blocker: nil, loaded: true, checked: true))
        }
        // Plugin ids are claimed by name, but enabled by id — match on the id the settings file holds
        let offeredIDs = Set(result.filter { $0.kind == .plugin }.map(\.key))
        for id in enabledPlugins.subtracting(offeredIDs).sorted() {
            result.append(LoaderItem(kind: .plugin, key: id, name: id, sourceDir: nil, sourcePath: nil,
                                     detail: orphanNote, blocker: nil, loaded: true, checked: true))
        }

        items = result
    }

    private func detail(for blocker: LoaderItem.Blocker?) -> String? {
        switch blocker {
        case .duplicate(let owner):
            return loc("Already provided by \"\(owner)\" — a name can only be loaded from one folder. Remove that folder, or reorder the folders in Settings, to load this one instead.",
                       "「\(owner)」已经提供了同名项 —— 一个名字只能由一个文件夹加载。想改用这个,请在设置里移除那个文件夹或调整顺序。")
        case .noMarketplace:
            return loc("No .claude-plugin/marketplace.json in this folder, so project settings have no way to name it — they reference a plugin as name@marketplace. Add the marketplace folder that lists this plugin instead.",
                       "这个文件夹里没有 .claude-plugin/marketplace.json,项目设置无法引用它 —— 引用插件靠的是「插件名@市场名」。请改为添加收录了这个插件的 marketplace 文件夹。")
        case nil:
            return nil
        }
    }

    // MARK: - Apply

    private func apply() {
        guard currentDirectory != nil else { return }
        let fm = FileManager.default
        var copied = 0, removed = 0, switchedOn = 0, switchedOff = 0
        var failures: [String] = []

        // Skills and agents: copy in, or delete the one item under its target folder, touching nothing else
        for item in items where item.kind != .plugin && item.checked != item.loaded {
            guard let dir = targetDir(for: item.kind) else { continue }
            let target = (dir as NSString).appendingPathComponent(item.key)
            do {
                if item.checked {
                    guard let source = item.sourcePath else { continue }   // Orphan: nothing to copy back from
                    guard !fm.fileExists(atPath: target) else { continue } // Never overwrite what is there
                    try fm.createDirectory(atPath: dir, withIntermediateDirectories: true)
                    try fm.copyItem(atPath: source, toPath: target)
                    copied += 1
                } else if fm.fileExists(atPath: target) {
                    try fm.removeItem(atPath: target)
                    removed += 1
                }
            } catch {
                failures.append("\(item.name): \(error.localizedDescription)")
            }
        }

        // Plugins: one rewrite of the project settings file, only when a switch actually moved
        let pluginItems = items.filter { $0.kind == .plugin }
        if pluginItems.contains(where: { $0.checked != $0.loaded }), let settingsPath {
            for item in pluginItems where item.checked != item.loaded {
                if item.checked { switchedOn += 1 } else { switchedOff += 1 }
            }
            let desired = Set(pluginItems.filter(\.checked).map(\.key))
            // Only marketplaces this panel knows about may be registered or dropped; anything else in
            // the file was put there by someone else and is left alone.
            let managed = scans.compactMap { scan -> (name: String, dir: String)? in
                guard let name = scan.marketplace else { return nil }
                return (name, scan.dir)
            }
            do {
                try ClaudePluginStore.write(enabledIDs: desired,
                                            managedMarketplaces: managed,
                                            stillEnabled: { marketplace in
                                                desired.contains { $0.hasSuffix("@" + marketplace) }
                                            },
                                            to: settingsPath)
            } catch {
                failures.append(loc("plugin settings", "插件设置") + ": \(error.localizedDescription)")
            }
        }

        // What the running session has to be told, and the two commands do not overlap: skills and
        // agents are files under .claude/, which /reload-skills rescans; a plugin switch is a line in
        // settings.local.json, which only /reload-plugins applies.
        var reloads: [String] = []
        if copied + removed > 0 { reloads.append("/reload-skills") }
        if switchedOn + switchedOff > 0 { reloads.append("/reload-plugins") }

        var parts: [String] = []
        if copied + removed > 0 {
            parts.append(loc("copied ", "复制 ") + "\(copied)" + loc(", deleted ", ",删除 ") + "\(removed)")
        }
        if switchedOn + switchedOff > 0 {
            parts.append(loc("plugins on ", "插件启用 ") + "\(switchedOn)"
                         + loc(", off ", ",停用 ") + "\(switchedOff)")
        }
        statusText = parts.isEmpty ? loc("Nothing changed", "没有变化")
                                   : loc("Synced: ", "已同步:") + parts.joined(separator: " · ")
        if !failures.isEmpty {
            statusText = loc("Partial failure — ", "部分失败 —— ") + failures.joined(separator: "; ")
        } else if !reloads.isEmpty {
            if let run = runInTerminal {
                send(reloads, using: run)
                statusText += loc(", sent ", ",已发送 ") + reloads.joined(separator: " + ")
            } else {
                // No terminal to type into — the change is on disk, so a fresh session picks it up
                statusText += loc(" (restart the claude session)", "(需重启 claude 会话生效)")
            }
        }

        reload()   // Refresh the checkboxes from the actual on-disk state
    }

    /// Type the reload commands into the terminal, one at a time.
    ///
    /// Claude is busy running the first slash command while the second is being typed, so they are
    /// spaced out instead of sent back to back: what arrives mid-run lands in the prompt box, and the
    /// second command would sit there unsubmitted.
    private func send(_ commands: [String], using run: @escaping (String) -> Void) {
        for (index, command) in commands.enumerated() {
            guard index > 0 else { run(command); continue }
            DispatchQueue.main.asyncAfter(deadline: .now() + Double(index) * 2.0) { run(command) }
        }
    }
}

// MARK: - Checkbox

/// The stock `.checkbox` draws its off state as a hairline outline that all but disappears against the
/// panel's dark background, so an unticked row reads as empty space rather than as a box you can tick.
/// This redraws both states with a border that holds its own in either appearance.
private struct LoaderCheckboxStyle: ToggleStyle {
    /// A blocked row is `.disabled`, which dims a stock control for us but not a hand-drawn one
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 6) {
            ZStack {
                RoundedRectangle(cornerRadius: 3)
                    .fill(configuration.isOn ? Color.accentColor : Color.primary.opacity(0.10))
                RoundedRectangle(cornerRadius: 3)
                    .strokeBorder(configuration.isOn ? Color.accentColor : Color.secondary.opacity(0.8),
                                  lineWidth: 1)
                if configuration.isOn {
                    Image(systemName: "checkmark")
                        .font(.system(size: 9, weight: .bold))
                        .foregroundStyle(Color.white)
                }
            }
            .frame(width: 14, height: 14)
            .opacity(isEnabled ? 1 : 0.45)
            configuration.label
        }
        .contentShape(Rectangle())   // The label is part of the hit target, as it is for a stock checkbox
        .onTapGesture { configuration.isOn.toggle() }
        .accessibilityRepresentation {
            Toggle(isOn: configuration.$isOn) { configuration.label }
        }
    }
}

// MARK: - Group header

/// Clickable group header: chevron + title + "checked/total", the counts being the reason a collapsed
/// group is still readable at a glance.
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
