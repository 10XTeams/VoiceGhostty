//! 🧩 加载器:扫描设置里添加的文件夹,把其中的技能 / Agent / 插件加载进当前项目。
//!
//! 扫描只问文件夹"里面有什么",不问"它是什么":插件看 `.claude-plugin/`,技能看带 `SKILL.md` 的文件夹,
//! Agent 看带 front matter 的 `.md`——在文件夹本层或其 `skills/` / `agents/` 下。
//! 唯一的二选一是插件:**声明了插件的文件夹只贡献这个插件**,里面的技能和 Agent 属于插件、随插件加载,
//! 单独再列一遍只会诱导重复加载。
//!
//! 应用时两类东西去两个地方:技能和 Agent 复制进项目的 `.claude/`;插件在项目的
//! `.claude/settings.local.json` 里开关(用 `.local` 是因为注册插件文件夹要写绝对路径,那是本机的事实,
//! 不属于仓库)。
//!
//! 一个名字只能加载一次(`.claude/skills/<name>` 只有一个,`enabledPlugins` 的键也只有一个),
//! 所以第一个提供某名字的文件夹拥有它,后面的同名项显示为"重名"并禁用,而不是悄悄挑一个。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::paths::home_dir;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Skill,
    Agent,
    Plugin,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Skill, Kind::Agent, Kind::Plugin];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Skill => "skill",
            Kind::Agent => "agent",
            Kind::Plugin => "plugin",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Skill => "技能",
            Kind::Agent => "Agent",
            Kind::Plugin => "插件",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginEntry {
    pub name: String,
    /// 所属 marketplace;没有 marketplace.json 的单插件文件夹为空
    pub marketplace: String,
    pub description: String,
    pub library_dir: PathBuf,
}

impl PluginEntry {
    /// Claude Code 在 `enabledPlugins` 里用的 id
    pub fn id(&self) -> String {
        if self.marketplace.is_empty() {
            self.name.clone()
        } else {
            format!("{}@{}", self.name, self.marketplace)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillEntry {
    pub name: String,
    /// 源文件夹(不一定是 `<库>/<name>`,可能在 `skills/` 下)
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEntry {
    pub file_name: String,
    pub path: PathBuf,
}

impl AgentEntry {
    pub fn name(&self) -> String {
        self.file_name.strip_suffix(".md").unwrap_or(&self.file_name).to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scan {
    pub dir: PathBuf,
    /// `marketplace.json` 声明的名字;只有有名字的 marketplace 能注册进项目设置
    pub marketplace: Option<String>,
    pub plugins: Vec<PluginEntry>,
    pub skills: Vec<SkillEntry>,
    pub agents: Vec<AgentEntry>,
}

/// 扫描所有仍然存在的文件夹,保持添加顺序
pub fn scan_all(dirs: &[PathBuf]) -> Vec<Scan> {
    dirs.iter().filter(|d| d.is_dir()).map(|d| scan(d)).collect()
}

pub fn scan(dir: &Path) -> Scan {
    let (marketplace, plugins) = read_plugins(dir);
    // 以清单为准而不是插件个数:列了零个插件的 marketplace 也是插件文件夹,不能把它的插件源码当技能列出来
    if marketplace.is_some() || !plugins.is_empty() {
        return Scan { dir: dir.to_path_buf(), marketplace, plugins, skills: vec![], agents: vec![] };
    }
    Scan {
        dir: dir.to_path_buf(),
        marketplace: None,
        plugins: vec![],
        skills: read_skills(dir),
        agents: read_agents(dir),
    }
}

fn file_stem_name(dir: &Path) -> String {
    dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// marketplace 带 `.claude-plugin/marketplace.json`,单插件带 `.claude-plugin/plugin.json`
fn read_plugins(dir: &Path) -> (Option<String>, Vec<PluginEntry>) {
    let manifest = dir.join(".claude-plugin");
    if let Some(obj) = read_object(&manifest.join("marketplace.json")) {
        // 名字读不出来的 marketplace 仍是 marketplace,用文件夹名
        let name = obj.get("name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| file_stem_name(dir));
        let plugins = obj
            .get("plugins")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|p| {
                        let plugin_name = p.get("name").and_then(Value::as_str).filter(|n| !n.is_empty())?;
                        Some(PluginEntry {
                            name: plugin_name.to_string(),
                            marketplace: name.clone(),
                            description: p.get("description").and_then(Value::as_str).unwrap_or("").to_string(),
                            library_dir: dir.to_path_buf(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        return (Some(name), plugins);
    }
    if let Some(obj) = read_object(&manifest.join("plugin.json")) {
        let name = obj.get("name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| file_stem_name(dir));
        return (
            None,
            vec![PluginEntry {
                name,
                marketplace: String::new(),
                description: obj.get("description").and_then(Value::as_str).unwrap_or("").to_string(),
                library_dir: dir.to_path_buf(),
            }],
        );
    }
    (None, vec![])
}

/// 技能 = 带 `SKILL.md` 的文件夹,在 `skills/` 下或本层。
/// 兼容旧式库:哪儿都没有 `SKILL.md` 时,每个子文件夹都算一个技能。
fn read_skills(dir: &Path) -> Vec<SkillEntry> {
    let mut found = vec![];
    let mut seen = HashSet::new();
    for root in [dir.join("skills"), dir.to_path_buf()] {
        for name in subfolders(&root) {
            let path = root.join(&name);
            if path.join("SKILL.md").exists() && seen.insert(name.clone()) {
                found.push(SkillEntry { name, path });
            }
        }
    }
    if !found.is_empty() {
        return found;
    }
    subfolders(dir).into_iter().map(|name| SkillEntry { path: dir.join(&name), name }).collect()
}

/// 只在旧式兜底里用得到:文件夹自己的文档,不是 Agent
const DOC_FILES: &[&str] = &["readme.md", "license.md", "changelog.md", "contributing.md", "claude.md", "agents.md"];

/// Agent = 以 YAML front matter 开头的 `.md`,在 `agents/` 下或本层。
/// 兼容旧式库:哪儿都没有 front matter 时,本层除文档外的所有 `.md` 都算
fn read_agents(dir: &Path) -> Vec<AgentEntry> {
    let mut found = vec![];
    let mut seen = HashSet::new();
    for root in [dir.join("agents"), dir.to_path_buf()] {
        for name in markdown_files(&root) {
            let path = root.join(&name);
            if has_front_matter(&path) && seen.insert(name.clone()) {
                found.push(AgentEntry { file_name: name, path });
            }
        }
    }
    if !found.is_empty() {
        return found;
    }
    markdown_files(dir)
        .into_iter()
        .filter(|n| !DOC_FILES.contains(&n.to_lowercase().as_str()))
        .map(|name| AgentEntry { path: dir.join(&name), file_name: name })
        .collect()
}

/// 本层非隐藏子文件夹名,排序
pub fn subfolders(dir: &Path) -> Vec<String> {
    list(dir, |p| p.is_dir(), |_| true)
}

/// 本层非隐藏 `.md` 文件名,排序
pub fn markdown_files(dir: &Path) -> Vec<String> {
    list(dir, |p| p.is_file(), |n| n.to_lowercase().ends_with(".md"))
}

fn list(dir: &Path, kind: impl Fn(&Path) -> bool, name_ok: impl Fn(&str) -> bool) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (!name.starts_with('.') && name_ok(&name) && kind(&e.path())).then_some(name)
        })
        .collect();
    names.sort();
    names
}

/// 文件开头(跳过空行)是否 `---`。只读前 512 字节:前几行没声明 front matter 就是没有
fn has_front_matter(path: &Path) -> bool {
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let mut buf = vec![0u8; 512];
    let Ok(n) = f.read(&mut buf) else { return false };
    let head = String::from_utf8_lossy(&buf[..n]);
    head.lines().map(str::trim).find(|l| !l.is_empty()) == Some("---")
}

/// 缺失或损坏的清单返回 None——坏清单只意味着"这里没有插件",文件夹里的技能和 Agent 仍可加载
fn read_object(path: &Path) -> Option<Map<String, Value>> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

// MARK: - 项目插件设置(只读写 enabledPlugins 与 extraKnownMarketplaces 两个键)

/// 解析 JSON 对象文件。缺失或空 → 空对象;格式错误 → 报错,免得一次写入把手改出错的设置文件抹平
fn read_settings(path: &Path) -> Result<Map<String, Value>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(e) => return Err(e.to_string()),
    };
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str(&text) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err(format!("{} 不是合法的 JSON", path.file_name().unwrap_or_default().to_string_lossy())),
    }
}

/// 设置文件里开着的插件 id。文件或键不存在是空集,不是错误
pub fn enabled_plugin_ids(path: &Path) -> Result<BTreeSet<String>, String> {
    let root = read_settings(path)?;
    Ok(root
        .get("enabledPlugins")
        .and_then(Value::as_object)
        .map(|m| m.iter().filter(|(_, v)| v.as_bool() == Some(true)).map(|(k, _)| k.clone()).collect())
        .unwrap_or_default())
}

/// 把 `enabledPlugins` 改写为恰好 `enabled`,并维护本面板负责的那部分 `extraKnownMarketplaces`,
/// 文件里其余设置原样保留。已经没有插件开着的 marketplace 把注册撤掉,免得文件里越积越多路径。
pub fn write_plugin_settings(
    path: &Path,
    enabled: &BTreeSet<String>,
    managed_marketplaces: &[(String, PathBuf)],
) -> Result<(), String> {
    let existed = path.exists();
    let mut root = read_settings(path)?;

    if enabled.is_empty() {
        root.remove("enabledPlugins");
    } else {
        root.insert("enabledPlugins".into(), enabled.iter().map(|id| (id.clone(), Value::Bool(true))).collect::<Map<_, _>>().into());
    }

    let mut markets = root.get("extraKnownMarketplaces").and_then(Value::as_object).cloned().unwrap_or_default();
    for (name, dir) in managed_marketplaces {
        let still_enabled = enabled.iter().any(|id| id.ends_with(&format!("@{name}")));
        let dir_str = dir.to_string_lossy();
        if still_enabled {
            markets.insert(name.clone(), json!({"source": {"source": "directory", "path": dir_str}}));
        } else if markets.get(name).and_then(|m| m.pointer("/source/path")).and_then(Value::as_str) == Some(&dir_str) {
            markets.remove(name); // 是我们加的,且不再需要
        }
    }
    if markets.is_empty() {
        root.remove("extraKnownMarketplaces");
    } else {
        root.insert("extraKnownMarketplaces".into(), Value::Object(markets));
    }

    // 只是浏览面板,绝不能在项目里留下一个空的设置文件
    if root.is_empty() && !existed {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&Value::Object(root)).map_err(|e| e.to_string())?;
    // 先写临时文件再改名:写一半崩溃不会留下损坏的设置
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text + "\n").map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

// MARK: - 面板模型

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blocker {
    /// 另一个已添加的文件夹已经提供了这个名字
    Duplicate(PathBuf),
    /// 裸插件文件夹没声明 marketplace,项目设置没法引用它
    NoMarketplace,
}

#[derive(Debug, Clone)]
pub struct LoaderItem {
    pub kind: Kind,
    /// 落地时的名字:技能文件夹名、`<agent>.md`、或 `name@marketplace`
    pub key: String,
    pub name: String,
    /// 来源文件夹;None = 项目里已有、但没有任何已添加文件夹提供
    pub source_dir: Option<PathBuf>,
    pub source_path: Option<PathBuf>,
    pub detail: String,
    pub blocker: Option<Blocker>,
    /// 打开面板时就已加载
    pub loaded: bool,
    pub checked: bool,
}

pub const ORPHAN_NOTE: &str = "项目里有,但没有任何已添加的文件夹提供它 —— 取消勾选即永久移除";

/// 第一个文件夹拥有某个名字,之后的都是重名
#[derive(Default)]
struct Ownership(HashMap<(Kind, String), PathBuf>);

impl Ownership {
    fn claim(&mut self, kind: Kind, key: &str, dir: &Path) -> Option<PathBuf> {
        match self.0.get(&(kind, key.to_string())) {
            Some(owner) => Some(owner.clone()),
            None => {
                self.0.insert((kind, key.to_string()), dir.to_path_buf());
                None
            }
        }
    }

    fn is_claimed(&self, kind: Kind, key: &str) -> bool {
        self.0.contains_key(&(kind, key.to_string()))
    }
}

#[derive(Debug, Default)]
pub struct ApplyOutcome {
    pub copied: usize,
    pub removed: usize,
    pub switched_on: usize,
    pub switched_off: usize,
    pub failures: Vec<String>,
    /// 需要让正在运行的 claude 执行的重载命令(技能/Agent 与插件各一个,互不覆盖)
    pub reloads: Vec<&'static str>,
}

impl ApplyOutcome {
    pub fn summary(&self) -> String {
        let mut parts = vec![];
        if self.copied + self.removed > 0 {
            parts.push(format!("复制 {},删除 {}", self.copied, self.removed));
        }
        if self.switched_on + self.switched_off > 0 {
            parts.push(format!("插件启用 {},停用 {}", self.switched_on, self.switched_off));
        }
        if !self.failures.is_empty() {
            return format!("部分失败 —— {}", self.failures.join("; "));
        }
        if parts.is_empty() { "没有变化".into() } else { format!("已同步:{}", parts.join(" · ")) }
    }
}

pub struct LoaderState {
    pub project: PathBuf,
    pub scans: Vec<Scan>,
    pub items: Vec<LoaderItem>,
    /// 缺失的文件夹、读不了的设置文件……只提示,不阻断
    pub warning: Option<String>,
}

impl LoaderState {
    pub fn settings_path(&self) -> PathBuf {
        self.project.join(".claude").join("settings.local.json")
    }

    pub fn target_dir(&self, kind: Kind) -> Option<PathBuf> {
        match kind {
            Kind::Skill => Some(self.project.join(".claude").join("skills")),
            Kind::Agent => Some(self.project.join(".claude").join("agents")),
            Kind::Plugin => None,
        }
    }

    pub fn has_changes(&self) -> bool {
        self.items.iter().any(|i| i.checked != i.loaded)
    }

    /// 文件夹自己的名字;与另一个已添加文件夹同名时改用完整路径
    pub fn folder_title(&self, dir: &Path) -> String {
        folder_title(&self.scans, dir)
    }

    pub fn load(project: &Path, library_dirs: &[PathBuf]) -> LoaderState {
        let scans = scan_all(library_dirs);
        let mut warnings = vec![];
        let missing: Vec<String> = library_dirs.iter().filter(|d| !d.is_dir()).map(|d| tilde(d)).collect();
        if !missing.is_empty() {
            warnings.push(format!("文件夹不存在:{}", missing.join(", ")));
        }
        let mut state = LoaderState { project: project.to_path_buf(), scans, items: vec![], warning: None };

        let enabled = match enabled_plugin_ids(&state.settings_path()) {
            Ok(ids) => ids,
            Err(e) => {
                warnings.push(format!("无法读取设置:{e}"));
                BTreeSet::new()
            }
        };
        let loaded_skills: BTreeSet<String> =
            state.target_dir(Kind::Skill).map(|d| subfolders(&d)).unwrap_or_default().into_iter().collect();
        let loaded_agents: BTreeSet<String> =
            state.target_dir(Kind::Agent).map(|d| markdown_files(&d)).unwrap_or_default().into_iter().collect();

        let mut items = vec![];
        let mut owners = Ownership::default();
        let dup = |owner: PathBuf| Blocker::Duplicate(owner);

        for scan in &state.scans {
            for plugin in &scan.plugins {
                let blocker = match owners.claim(Kind::Plugin, &plugin.name, &scan.dir) {
                    Some(owner) => Some(dup(owner)),
                    None if plugin.marketplace.is_empty() => Some(Blocker::NoMarketplace),
                    None => None,
                };
                let loaded = blocker.is_none() && enabled.contains(&plugin.id());
                let fallback = if plugin.description.is_empty() { plugin.id() } else { plugin.description.clone() };
                items.push(LoaderItem {
                    kind: Kind::Plugin,
                    key: plugin.id(),
                    name: plugin.name.clone(),
                    source_dir: Some(scan.dir.clone()),
                    source_path: None,
                    detail: blocker_detail(&state.scans, blocker.as_ref()).unwrap_or(fallback),
                    blocker,
                    loaded,
                    checked: loaded,
                });
            }
            for skill in &scan.skills {
                let blocker = owners.claim(Kind::Skill, &skill.name, &scan.dir).map(dup);
                let loaded = blocker.is_none() && loaded_skills.contains(&skill.name);
                items.push(LoaderItem {
                    kind: Kind::Skill,
                    key: skill.name.clone(),
                    name: skill.name.clone(),
                    source_dir: Some(scan.dir.clone()),
                    source_path: Some(skill.path.clone()),
                    detail: blocker_detail(&state.scans, blocker.as_ref()).unwrap_or_else(|| tilde(&skill.path)),
                    blocker,
                    loaded,
                    checked: loaded,
                });
            }
            for agent in &scan.agents {
                let blocker = owners.claim(Kind::Agent, &agent.file_name, &scan.dir).map(dup);
                let loaded = blocker.is_none() && loaded_agents.contains(&agent.file_name);
                items.push(LoaderItem {
                    kind: Kind::Agent,
                    key: agent.file_name.clone(),
                    name: agent.name(),
                    source_dir: Some(scan.dir.clone()),
                    source_path: Some(agent.path.clone()),
                    detail: blocker_detail(&state.scans, blocker.as_ref()).unwrap_or_else(|| tilde(&agent.path)),
                    blocker,
                    loaded,
                    checked: loaded,
                });
            }
        }

        // 项目里已有、但没有文件夹提供的:也列出来,仍可关掉(否则被移除的文件夹留下的东西就看不见了)
        let orphan = |kind: Kind, key: String, name: String| LoaderItem {
            kind,
            key,
            name,
            source_dir: None,
            source_path: None,
            detail: ORPHAN_NOTE.into(),
            blocker: None,
            loaded: true,
            checked: true,
        };
        for name in loaded_skills.iter().filter(|n| !owners.is_claimed(Kind::Skill, n)) {
            items.push(orphan(Kind::Skill, name.clone(), name.clone()));
        }
        for name in loaded_agents.iter().filter(|n| !owners.is_claimed(Kind::Agent, n)) {
            items.push(orphan(Kind::Agent, name.clone(), name.strip_suffix(".md").unwrap_or(name).to_string()));
        }
        // 插件按名字认领、按 id 开启——所以这里按设置文件里的 id 匹配
        let offered: HashSet<String> = items.iter().filter(|i| i.kind == Kind::Plugin).map(|i| i.key.clone()).collect();
        for id in enabled.iter().filter(|id| !offered.contains(*id)) {
            items.push(orphan(Kind::Plugin, id.clone(), id.clone()));
        }

        state.items = items;
        state.warning = (!warnings.is_empty()).then(|| warnings.join(";"));
        state
    }

    /// 把勾选状态落到磁盘:技能/Agent 复制或删除(只动目标目录下的那一项,已存在的绝不覆盖);
    /// 插件只在确有开关变化时重写一次设置文件
    pub fn apply(&self) -> ApplyOutcome {
        let mut out = ApplyOutcome::default();
        for item in self.items.iter().filter(|i| i.kind != Kind::Plugin && i.checked != i.loaded) {
            let Some(dir) = self.target_dir(item.kind) else { continue };
            let target = dir.join(&item.key);
            let result = if item.checked {
                match &item.source_path {
                    None => Ok(()),                        // 孤儿:没有来源可复制
                    Some(_) if target.exists() => Ok(()), // 已存在的不覆盖
                    Some(src) => std::fs::create_dir_all(&dir).and_then(|_| copy_all(src, &target)).map(|_| out.copied += 1),
                }
            } else if target.exists() {
                remove_all(&target).map(|_| out.removed += 1)
            } else {
                Ok(())
            };
            if let Err(e) = result {
                out.failures.push(format!("{}: {e}", item.name));
            }
        }

        let plugins: Vec<&LoaderItem> = self.items.iter().filter(|i| i.kind == Kind::Plugin).collect();
        if plugins.iter().any(|i| i.checked != i.loaded) {
            for i in plugins.iter().filter(|i| i.checked != i.loaded) {
                if i.checked { out.switched_on += 1 } else { out.switched_off += 1 }
            }
            let desired: BTreeSet<String> = plugins.iter().filter(|i| i.checked).map(|i| i.key.clone()).collect();
            // 只有本面板认识的 marketplace 才可能被注册或撤销;文件里其他的是别人放的,不碰
            let managed: Vec<(String, PathBuf)> =
                self.scans.iter().filter_map(|s| Some((s.marketplace.clone()?, s.dir.clone()))).collect();
            if let Err(e) = write_plugin_settings(&self.settings_path(), &desired, &managed) {
                out.failures.push(format!("插件设置: {e}"));
            }
        }

        if out.copied + out.removed > 0 {
            out.reloads.push("/reload-skills");
        }
        if out.switched_on + out.switched_off > 0 {
            out.reloads.push("/reload-plugins");
        }
        out
    }
}

/// 设置页添加文件夹时立即提示:新加的文件夹里有多少项与上面的文件夹重名、无法从这里加载
pub fn duplicate_counts(dirs: &[PathBuf]) -> BTreeMap<PathBuf, usize> {
    let mut owners = Ownership::default();
    let mut counts = BTreeMap::new();
    for scan in scan_all(dirs) {
        let mut n = 0;
        for p in &scan.plugins {
            n += owners.claim(Kind::Plugin, &p.name, &scan.dir).is_some() as usize;
        }
        for s in &scan.skills {
            n += owners.claim(Kind::Skill, &s.name, &scan.dir).is_some() as usize;
        }
        for a in &scan.agents {
            n += owners.claim(Kind::Agent, &a.file_name, &scan.dir).is_some() as usize;
        }
        counts.insert(scan.dir.clone(), n);
    }
    counts
}

fn blocker_detail(scans: &[Scan], blocker: Option<&Blocker>) -> Option<String> {
    Some(match blocker? {
        Blocker::Duplicate(owner) => format!(
            "「{}」已经提供了同名项 —— 一个名字只能由一个文件夹加载。想改用这个,请在设置里移除那个文件夹或调整顺序。",
            folder_title(scans, owner)
        ),
        Blocker::NoMarketplace => "这个文件夹里没有 .claude-plugin/marketplace.json,项目设置无法引用它 —— 引用插件靠的是「插件名@市场名」。请改为添加收录了这个插件的 marketplace 文件夹。".into(),
    })
}

pub fn folder_title(scans: &[Scan], dir: &Path) -> String {
    let name = file_stem_name(dir);
    let collides = scans.iter().any(|s| s.dir != dir && file_stem_name(&s.dir) == name);
    if collides { tilde(dir) } else { name }
}

/// 家目录缩写成 `~`
pub fn tilde(path: &Path) -> String {
    let home = home_dir();
    match path.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

fn copy_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_all(&entry.path(), &dst.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(src, dst).map(|_| ())
    }
}

fn remove_all(path: &Path) -> std::io::Result<()> {
    if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    const AGENT: &str = "---\nname: x\ndescription: y\n---\nbody";

    #[test]
    fn scans_skills_and_agents_by_markers() {
        let d = tempfile::tempdir().unwrap();
        let lib = d.path();
        write(&lib.join("skills/alpha/SKILL.md"), "a");
        write(&lib.join("beta/SKILL.md"), "b");
        write(&lib.join("not-a-skill/notes.txt"), "n");
        write(&lib.join("agents/reviewer.md"), AGENT);
        write(&lib.join("README.md"), "# readme");
        let s = scan(lib);
        let skills: Vec<_> = s.skills.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(skills, ["alpha", "beta"]);
        assert_eq!(s.agents.len(), 1);
        assert_eq!(s.agents[0].name(), "reviewer");
        assert!(s.plugins.is_empty());
    }

    #[test]
    fn legacy_library_lists_every_subfolder_and_md() {
        let d = tempfile::tempdir().unwrap();
        write(&d.path().join("one/x.txt"), "");
        write(&d.path().join("two/y.txt"), "");
        write(&d.path().join("helper.md"), "no front matter");
        write(&d.path().join("README.md"), "doc");
        let s = scan(d.path());
        assert_eq!(s.skills.len(), 2);
        let agents: Vec<_> = s.agents.iter().map(|a| a.file_name.as_str()).collect();
        assert_eq!(agents, ["helper.md"]);
    }

    #[test]
    fn plugin_folder_contributes_only_plugins() {
        let d = tempfile::tempdir().unwrap();
        write(
            &d.path().join(".claude-plugin/marketplace.json"),
            r#"{"name":"mkt","plugins":[{"name":"p1","description":"first"},{"name":""}]}"#,
        );
        write(&d.path().join("skills/inner/SKILL.md"), "");
        let s = scan(d.path());
        assert_eq!(s.marketplace.as_deref(), Some("mkt"));
        assert_eq!(s.plugins.len(), 1);
        assert_eq!(s.plugins[0].id(), "p1@mkt");
        assert!(s.skills.is_empty() && s.agents.is_empty());
    }

    #[test]
    fn broken_manifest_leaves_skills_loadable() {
        let d = tempfile::tempdir().unwrap();
        write(&d.path().join(".claude-plugin/marketplace.json"), "{ not json");
        write(&d.path().join("s/SKILL.md"), "");
        let s = scan(d.path());
        assert!(s.marketplace.is_none());
        assert_eq!(s.skills.len(), 1);
    }

    #[test]
    fn duplicates_are_blocked_and_counted() {
        let d = tempfile::tempdir().unwrap();
        let (a, b, proj) = (d.path().join("a"), d.path().join("b"), d.path().join("proj"));
        write(&a.join("dup/SKILL.md"), "");
        write(&b.join("dup/SKILL.md"), "");
        write(&b.join("uniq/SKILL.md"), "");
        std::fs::create_dir_all(&proj).unwrap();
        let st = LoaderState::load(&proj, &[a.clone(), b.clone()]);
        let blocked: Vec<_> = st.items.iter().filter(|i| i.blocker.is_some()).collect();
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].source_dir.as_deref(), Some(b.as_path()));
        assert_eq!(duplicate_counts(&[a.clone(), b.clone()])[&b], 1);
    }

    #[test]
    fn apply_copies_removes_and_toggles_plugins() {
        let d = tempfile::tempdir().unwrap();
        let (lib, mkt, proj) = (d.path().join("lib"), d.path().join("mkt"), d.path().join("proj"));
        write(&lib.join("s1/SKILL.md"), "skill");
        write(&lib.join("s1/extra/data.txt"), "nested");
        write(&lib.join("agents/rev.md"), AGENT);
        write(&mkt.join(".claude-plugin/marketplace.json"), r#"{"name":"m","plugins":[{"name":"p"}]}"#);
        // 项目里原有别的设置,必须保留
        write(&proj.join(".claude/settings.local.json"), r#"{"permissions":{"allow":["Bash(ls)"]}}"#);

        let dirs = [lib.clone(), mkt.clone()];
        let mut st = LoaderState::load(&proj, &dirs);
        assert!(!st.has_changes());
        for item in &mut st.items {
            item.checked = true;
        }
        let out = st.apply();
        assert_eq!((out.copied, out.switched_on), (2, 1), "{:?}", out.failures);
        assert_eq!(out.reloads, ["/reload-skills", "/reload-plugins"]);
        assert_eq!(std::fs::read_to_string(proj.join(".claude/skills/s1/extra/data.txt")).unwrap(), "nested");
        assert!(proj.join(".claude/agents/rev.md").is_file());
        let settings: Value =
            serde_json::from_str(&std::fs::read_to_string(proj.join(".claude/settings.local.json")).unwrap()).unwrap();
        assert_eq!(settings["enabledPlugins"]["p@m"], true);
        assert_eq!(settings["extraKnownMarketplaces"]["m"]["source"]["path"], mkt.to_string_lossy().as_ref());
        assert_eq!(settings["permissions"]["allow"][0], "Bash(ls)");

        // 重新打开:都是已加载;全部取消 → 删除,marketplace 注册撤掉,其他设置仍在
        let mut st = LoaderState::load(&proj, &dirs);
        assert!(st.items.iter().all(|i| i.loaded));
        for item in &mut st.items {
            item.checked = false;
        }
        let out = st.apply();
        assert_eq!((out.removed, out.switched_off), (2, 1));
        assert!(!proj.join(".claude/skills/s1").exists());
        let settings: Value =
            serde_json::from_str(&std::fs::read_to_string(proj.join(".claude/settings.local.json")).unwrap()).unwrap();
        assert!(settings.get("enabledPlugins").is_none());
        assert!(settings.get("extraKnownMarketplaces").is_none());
        assert_eq!(settings["permissions"]["allow"][0], "Bash(ls)");
    }

    #[test]
    fn orphans_are_listed_and_browsing_writes_nothing() {
        let d = tempfile::tempdir().unwrap();
        let proj = d.path().join("proj");
        write(&proj.join(".claude/skills/leftover/SKILL.md"), "");
        let st = LoaderState::load(&proj, &[]);
        assert_eq!(st.items.len(), 1);
        assert!(st.items[0].source_dir.is_none() && st.items[0].loaded);
        // 没有改动就 apply:不应创建设置文件
        let out = st.apply();
        assert_eq!(out.summary(), "没有变化");
        assert!(!proj.join(".claude/settings.local.json").exists());
    }

    #[test]
    fn malformed_settings_is_reported_not_overwritten() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("settings.local.json");
        write(&path, "{ broken");
        assert!(enabled_plugin_ids(&path).is_err());
        assert!(write_plugin_settings(&path, &BTreeSet::new(), &[]).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ broken");
    }
}
