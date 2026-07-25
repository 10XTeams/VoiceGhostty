import SwiftUI
import AppKit

/// Skill dynamic-loading panel: a checkbox synchronizer between the skill library (source) ↔ cwd/.claude/skills (target).
/// - Lists all subfolders under the skill library directory, one checkbox per item
/// - Skills already present in the current terminal's cwd .claude/skills/ are checked by default
/// - Click "Apply": newly checked → copy from the library to cwd; unchecked → delete the corresponding skill folder under cwd
/// The skill library path is chosen in the Settings panel (stored in UserDefaults); if never chosen,
/// it falls back to the config's skill-library-dir (default ~/.claude/skills).
struct SkillPanelView: View {
    let currentDirectory: String?
    var onDone: () -> Void = {}
    /// Run a command in the active terminal after a successful sync (e.g. /reload-skills); nil means do not run
    var runInTerminal: ((String) -> Void)? = nil

    @ObservedObject private var loc = Loc.shared
    @State private var libraryDir: String = Self.resolveLibraryDir()
    @State private var skills: [SkillItem] = []
    @State private var statusText = ""
    @State private var loadError: String?

    /// A path chosen in the UI (Settings / UserDefaults) takes priority, then config, then the default ~/.claude/skills
    private static func resolveLibraryDir() -> String {
        AppSettings.skillLibraryDir
    }

    struct SkillItem: Identifiable {
        let name: String
        let inLibrary: Bool      // Exists in the skill library (absent = local-only; once unchecked and deleted, it cannot be re-checked)
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
                Text(loc("No skills to show (the skill library is empty and the current directory has no loaded skills)",
                         "没有可显示的技能(技能库为空,且当前目录未加载任何技能)"))
                    .font(.caption).foregroundStyle(.secondary)
            } else {
                Divider()
                ScrollView {
                    VStack(alignment: .leading, spacing: 6) {
                        // First group: skills already loaded in the project
                        skillRows(loaded: true)
                        // Separator: shown only when both groups have content
                        if hasLoadedGroup && hasLibraryOnlyGroup {
                            Divider().padding(.vertical, 2)
                        }
                        // Second group: skills in the library that are not loaded
                        skillRows(loaded: false)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.vertical, 2)
                }
                // Inside a popover, a ScrollView with no intrinsic height collapses to 0, so height must be set explicitly by item count
                .frame(height: min(CGFloat(skills.count) * 24 + 20, 300))
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

    // MARK: - Grouped rows (grouped by load state at panel-open time; toggling does not move items between groups)

    private var hasLoadedGroup: Bool { skills.contains { $0.loaded } }
    private var hasLibraryOnlyGroup: Bool { skills.contains { !$0.loaded } }

    private func skillRows(loaded: Bool) -> some View {
        ForEach($skills) { $item in
            if item.loaded == loaded {
                Toggle(isOn: $item.checked) {
                    HStack(spacing: 5) {
                        Text(item.name).lineLimit(1)
                        if !item.inLibrary {
                            Text(loc("Local only", "仅本地")).font(.caption2)
                                .foregroundStyle(.orange)
                        }
                    }
                }
                .toggleStyle(.checkbox)
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
        if currentDirectory == nil {
            loadError = loc("Unable to determine the current terminal's working directory",
                            "无法确定当前终端的工作目录")
        } else if !FileManager.default.fileExists(atPath: libraryDir) {
            // A missing library is only a warning; still list loaded skills (click 📁 to switch libraries)
            loadError = loc("Skill library does not exist: ", "技能库不存在:")
                + (libraryDir as NSString).abbreviatingWithTildeInPath
        }
        let library = Set(Self.subfolders(of: libraryDir))
        let loaded = Set(targetDir.map(Self.subfolders(of:)) ?? [])
        // Union: those in the library + local-only ones (the latter are deleted once unchecked)
        skills = library.union(loaded).sorted().map { name in
            SkillItem(name: name,
                      inLibrary: library.contains(name),
                      loaded: loaded.contains(name),
                      checked: loaded.contains(name))
        }
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
                    // Newly checked → copy from the library (skip if the target already exists, do not overwrite)
                    guard !fm.fileExists(atPath: target) else { continue }
                    try fm.createDirectory(atPath: targetDir, withIntermediateDirectories: true)
                    let source = (libraryDir as NSString).appendingPathComponent(item.name)
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
