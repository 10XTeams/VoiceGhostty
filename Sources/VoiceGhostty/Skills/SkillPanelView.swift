import SwiftUI
import AppKit

/// Skill dynamic-loading panel: a checkbox synchronizer between the skill libraries (source) ↔ cwd/.claude/skills (target).
/// - Lists all subfolders under each skill library directory, one checkbox per item, one group per library
/// - Skills already present in the current terminal's cwd .claude/skills/ are checked by default
/// - Click "Apply": newly checked → copy from the library to cwd; unchecked → delete the corresponding skill folder under cwd
/// The skill library paths are chosen in the Settings panel (stored in UserDefaults); if never chosen,
/// they fall back to the config's skill-library-dir (default ~/.claude/skills).
struct SkillPanelView: View {
    let currentDirectory: String?
    var onDone: () -> Void = {}
    /// Run a command in the active terminal after a successful sync (e.g. /reload-skills); nil means do not run
    var runInTerminal: ((String) -> Void)? = nil

    @ObservedObject private var loc = Loc.shared
    @State private var libraryDirs: [String] = AppSettings.skillLibraryDirs
    @State private var skills: [SkillItem] = []
    @State private var statusText = ""
    @State private var loadError: String?
    /// Ids of the collapsed groups (default: everything expanded); persisted so a folded-away library stays folded
    @State private var collapsed: Set<String> = AppSettings.collapsedSkillGroups

    struct SkillItem: Identifiable {
        let name: String
        /// The library folder this skill is copied from; nil = local-only (present in the project but in no
        /// library — once unchecked and deleted, it cannot be re-checked)
        let source: String?
        let loaded: Bool         // Whether it was already loaded in cwd when the panel opened
        var checked: Bool
        var id: String { name }
    }

    private var targetDir: String? {
        guard let cwd = currentDirectory else { return nil }
        return (cwd as NSString).appendingPathComponent(".claude/skills")
    }

    /// Whether the checked state has changed relative to the initial state (decides whether "Apply" is clickable)
    private var hasChanges: Bool {
        skills.contains { $0.checked != $0.loaded }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(loc("Skill Loader", "技能加载器")).font(.headline)
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

            if let cwd = currentDirectory {
                Text(loc("Current directory: ", "当前目录:") + (cwd as NSString).abbreviatingWithTildeInPath)
                    .font(.caption).foregroundStyle(.secondary)
                    .lineLimit(1).truncationMode(.middle)
                    .help(loc("Skills sync to ", "技能同步到 ") + "\(cwd)/.claude/skills/")
            }

            if let err = loadError {
                Text(err).font(.caption).foregroundStyle(.orange)
            }

            if skills.isEmpty {
                Text(loc("No skills to show (every skill library is empty and the current directory has no loaded skills)",
                         "没有可显示的技能(所选技能库都是空的,且当前目录未加载任何技能)"))
                    .font(.caption).foregroundStyle(.secondary)
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
                    .disabled(!hasChanges || targetDir == nil)
            }
        }
        .padding(12)
        .frame(width: 320)
        .onAppear { reload() }
    }

    // MARK: - Grouped rows (one group per library folder, in the configured order; local-only skills go last)

    private struct SkillGroup: Identifiable {
        let id: String          // The library path, or "" for the local-only group
        let title: String
        let tooltip: String
    }

    /// Only groups that actually have skills to show; a library folder contributing nothing is skipped
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

    /// Clickable group header: chevron + folder name + "checked/total", the counts being the reason
    /// a collapsed group is still readable at a glance.
    private func groupHeader(_ group: SkillGroup) -> some View {
        let items = skills.filter { ($0.source ?? "") == group.id }
        let isCollapsed = collapsed.contains(group.id)
        return Button {
            if isCollapsed { collapsed.remove(group.id) } else { collapsed.insert(group.id) }
            AppSettings.collapsedSkillGroups = collapsed
        } label: {
            HStack(spacing: 4) {
                Image(systemName: isCollapsed ? "chevron.right" : "chevron.down")
                    .font(.caption2)
                    .frame(width: 10)
                Text(group.title)
                    .font(.caption).bold()
                    .lineLimit(1).truncationMode(.middle)
                Text("\(items.filter(\.checked).count)/\(items.count)")
                    .font(.caption2)
                Spacer()
            }
            .foregroundStyle(.secondary)
            .contentShape(Rectangle())   // The whole header row is the hit target, not just the text
        }
        .buttonStyle(.plain)
        .help(group.tooltip)
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
                    Text(item.name).lineLimit(1)
                }
                .toggleStyle(.checkbox)
                .padding(.leading, 8)
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

    private func reload() {
        loadError = nil
        libraryDirs = AppSettings.skillLibraryDirs
        let missing = libraryDirs.filter { !FileManager.default.fileExists(atPath: $0) }
        if currentDirectory == nil {
            loadError = loc("Unable to determine the current terminal's working directory",
                            "无法确定当前终端的工作目录")
        } else if !missing.isEmpty {
            // A missing library is only a warning; still list the other libraries and the loaded skills
            loadError = loc("Skill library does not exist: ", "技能库不存在:")
                + missing.map { ($0 as NSString).abbreviatingWithTildeInPath }.joined(separator: ", ")
        }

        let loaded = Set(targetDir.map(Self.subfolders(of:)) ?? [])
        var items: [SkillItem] = []
        var claimed = Set<String>()
        // One group per library, in configured order; a name in two libraries belongs to the first one,
        // since the target is a single .claude/skills/<name>
        for dir in libraryDirs {
            for name in Self.subfolders(of: dir).sorted() where claimed.insert(name).inserted {
                items.append(SkillItem(name: name, source: dir,
                                       loaded: loaded.contains(name),
                                       checked: loaded.contains(name)))
            }
        }
        // Local-only: loaded in the project but in no library (deleted once unchecked)
        for name in loaded.subtracting(claimed).sorted() {
            items.append(SkillItem(name: name, source: nil, loaded: true, checked: true))
        }
        skills = items
    }

    // MARK: - Apply (copy / delete)

    private func apply() {
        guard let targetDir else { return }
        let fm = FileManager.default
        var copied = 0, removed = 0, failures: [String] = []

        for item in skills where item.checked != item.loaded {
            let target = (targetDir as NSString).appendingPathComponent(item.name)
            do {
                if item.checked {
                    // Newly checked → copy from its library (skip if the target already exists, do not overwrite)
                    guard let library = item.source else { continue }   // Local-only: nothing to copy back from
                    guard !fm.fileExists(atPath: target) else { continue }
                    try fm.createDirectory(atPath: targetDir, withIntermediateDirectories: true)
                    let source = (library as NSString).appendingPathComponent(item.name)
                    try fm.copyItem(atPath: source, toPath: target)
                    copied += 1
                } else if fm.fileExists(atPath: target) {
                    // Unchecked → delete only this skill folder under targetDir, touching nothing else
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
