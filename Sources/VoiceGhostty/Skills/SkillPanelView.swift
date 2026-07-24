import SwiftUI
import AppKit

/// Skill 动态加载面板:技能库(源)↔ cwd/.claude/skills(目标)的复选同步器。
/// - 列出技能库目录下的所有子文件夹,每项一个复选框
/// - 当前终端 cwd 下 .claude/skills/ 已有的技能默认打勾
/// - 点「应用」:新勾选 → 从库拷贝到 cwd;取消勾选 → 删除 cwd 下对应技能文件夹
/// 技能库路径可在面板里点文件夹图标选择(存 UserDefaults),
/// 未选择过则回落 config 的 skill-library-dir(默认 ~/.claude/skills)。
struct SkillPanelView: View {
    let currentDirectory: String?
    var onDone: () -> Void = {}
    /// 同步成功后在活动终端执行命令(如 /reload-skills);nil 则不执行
    var runInTerminal: ((String) -> Void)? = nil

    @State private var libraryDir: String = Self.resolveLibraryDir()
    @State private var skills: [SkillItem] = []
    @State private var statusText = ""
    @State private var loadError: String?

    /// 界面选过的路径(UserDefaults)优先,其次 config,再默认 ~/.claude/skills
    private static func resolveLibraryDir() -> String {
        UserDefaults.standard.string(forKey: "skill-library-dir")
            ?? Config.load().skillLibraryDir
    }

    struct SkillItem: Identifiable {
        let name: String
        let inLibrary: Bool      // 技能库里存在(不存在 = 仅本地,取消勾选删除后无法再勾回)
        let loaded: Bool         // 打开面板时 cwd 是否已加载
        var checked: Bool
        var id: String { name }
    }

    private var targetDir: String? {
        guard let cwd = currentDirectory else { return nil }
        return (cwd as NSString).appendingPathComponent(".claude/skills")
    }

    /// 勾选状态相对初始是否有变化(决定「应用」是否可点)
    private var hasChanges: Bool {
        skills.contains { $0.checked != $0.loaded }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("Skill 动态加载").font(.headline)
                Spacer()
                Button {
                    reload()
                } label: {
                    Image(systemName: "arrow.clockwise")
                }
                .buttonStyle(.plain)
                .help("重新扫描")
            }

            if let cwd = currentDirectory {
                Text("当前目录:\((cwd as NSString).abbreviatingWithTildeInPath)")
                    .font(.caption).foregroundStyle(.secondary)
                    .lineLimit(1).truncationMode(.middle)
                    .help("技能同步到 \(cwd)/.claude/skills/")
            }

            if let err = loadError {
                Text(err).font(.caption).foregroundStyle(.orange)
            }

            if skills.isEmpty {
                Text("没有可显示的技能(技能库为空,当前目录也没有已加载技能)")
                    .font(.caption).foregroundStyle(.secondary)
            } else {
                Divider()
                ScrollView {
                    VStack(alignment: .leading, spacing: 6) {
                        // 第一组:项目中已加载的技能
                        skillRows(loaded: true)
                        // 间隔线:两组都有内容才显示
                        if hasLoadedGroup && hasLibraryOnlyGroup {
                            Divider().padding(.vertical, 2)
                        }
                        // 第二组:技能库中未加载的技能
                        skillRows(loaded: false)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.vertical, 2)
                }
                // popover 里 ScrollView 无固有高度会塌缩成 0,必须按条目数显式给高
                .frame(height: min(CGFloat(skills.count) * 24 + 20, 300))
                Divider()
            }

            if !statusText.isEmpty {
                Text(statusText).font(.caption).foregroundStyle(.secondary)
            }

            HStack {
                Button {
                    chooseLibraryDir()
                } label: {
                    Image(systemName: "folder")
                }
                .buttonStyle(.plain)
                .help("选择技能库文件夹")
                Text((libraryDir as NSString).abbreviatingWithTildeInPath)
                    .font(.caption2).foregroundStyle(.tertiary)
                    .lineLimit(1).truncationMode(.middle)
                    .help("技能库:\(libraryDir)(点文件夹图标可换,或 config 里配 skill-library-dir)")
                Spacer()
                Button("取消") { onDone() }
                    .keyboardShortcut(.cancelAction)
                Button("应用") { apply() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(!hasChanges || targetDir == nil)
            }
        }
        .padding(12)
        .frame(width: 320)
        .onAppear { reload() }
    }

    // MARK: - 分组行(按打开面板时的加载状态分组,勾选变化不跳组)

    private var hasLoadedGroup: Bool { skills.contains { $0.loaded } }
    private var hasLibraryOnlyGroup: Bool { skills.contains { !$0.loaded } }

    private func skillRows(loaded: Bool) -> some View {
        ForEach($skills) { $item in
            if item.loaded == loaded {
                Toggle(isOn: $item.checked) {
                    HStack(spacing: 5) {
                        Text(item.name).lineLimit(1)
                        if !item.inLibrary {
                            Text("仅本地").font(.caption2)
                                .foregroundStyle(.orange)
                        }
                    }
                }
                .toggleStyle(.checkbox)
            }
        }
    }

    // MARK: - 技能库选择

    private func chooseLibraryDir() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.prompt = "选择"
        panel.message = "选择技能库文件夹(其下每个子文件夹是一个可加载的技能)"
        panel.directoryURL = URL(fileURLWithPath: libraryDir)
        guard panel.runModal() == .OK, let path = panel.url?.path else { return }
        libraryDir = path
        UserDefaults.standard.set(path, forKey: "skill-library-dir")   // 记住选择
        statusText = ""
        reload()
    }

    // MARK: - 扫描

    /// 列出 dir 下的非隐藏子文件夹名(每个子文件夹 = 一个技能)
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
            loadError = "无法确定当前终端的工作目录"
        } else if !FileManager.default.fileExists(atPath: libraryDir) {
            // 库缺失只警告,仍列出已加载技能(点 📁 换库)
            loadError = "技能库不存在:\((libraryDir as NSString).abbreviatingWithTildeInPath)"
        }
        let library = Set(Self.subfolders(of: libraryDir))
        let loaded = Set(targetDir.map(Self.subfolders(of:)) ?? [])
        // 并集:库里的 + 仅本地的(后者取消勾选后即被删除)
        skills = library.union(loaded).sorted().map { name in
            SkillItem(name: name,
                      inLibrary: library.contains(name),
                      loaded: loaded.contains(name),
                      checked: loaded.contains(name))
        }
    }

    // MARK: - 应用(拷贝 / 删除)

    private func apply() {
        guard let targetDir else { return }
        let fm = FileManager.default
        var copied = 0, removed = 0, failures: [String] = []

        for item in skills where item.checked != item.loaded {
            let target = (targetDir as NSString).appendingPathComponent(item.name)
            do {
                if item.checked {
                    // 新勾选 → 从库拷贝(目标已存在则跳过,不覆盖)
                    guard !fm.fileExists(atPath: target) else { continue }
                    try fm.createDirectory(atPath: targetDir, withIntermediateDirectories: true)
                    let source = (libraryDir as NSString).appendingPathComponent(item.name)
                    try fm.copyItem(atPath: source, toPath: target)
                    copied += 1
                } else if fm.fileExists(atPath: target) {
                    // 取消勾选 → 只删 targetDir 下的这个技能文件夹,不碰别处
                    try fm.removeItem(atPath: target)
                    removed += 1
                }
            } catch {
                failures.append("\(item.name): \(error.localizedDescription)")
            }
        }

        if failures.isEmpty {
            statusText = "已同步:拷贝 \(copied) 个,删除 \(removed) 个"
            // 有实际变更时通知终端里的 Claude 重载技能
            if copied + removed > 0, let run = runInTerminal {
                run("/reload-skills")
                statusText += ",已发送 /reload-skills"
            }
        } else {
            statusText = "部分失败 — " + failures.joined(separator: "; ")
        }
        reload()   // 以磁盘实际状态刷新勾选
    }
}
