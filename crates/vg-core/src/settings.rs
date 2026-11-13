//! GUI 可改的设置,存 `settings.json`(对应 Swift 版的 UserDefaults)。
//! 与只读配置文件共有的 key,GUI 设置优先;`None` 表示"没在 GUI 里设过",回退配置文件。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{normalize_language, Config, VoiceEngineKind};
use crate::paths::{config_dir, settings_file};

pub const SCROLLBACK_RANGE: std::ops::RangeInclusive<usize> = 500..=200_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Settings {
    /// 识别语言("zh" / "en" / "auto"),覆盖 voice-language
    pub default_language: Option<String>,
    /// 覆盖 voice-engine
    pub voice_engine: Option<String>,
    /// 覆盖 voice-punctuation
    pub adds_punctuation: Option<bool>,
    /// 覆盖 voice-live-caption
    pub live_caption: Option<bool>,
    /// 覆盖 voice-spoken-symbols
    pub spoken_symbols: Option<bool>,
    /// 覆盖 voice-ptt-key
    pub ptt_key: Option<String>,
    /// 覆盖 voice-commands
    pub voice_commands: Option<bool>,
    /// 个人词典(设置面板维护);None 时用配置文件里的 voice-replace
    pub voice_replacements: Option<Vec<(String, String)>>,
    /// 覆盖 scrollback
    pub scrollback: Option<usize>,
    /// 每个窗格的会话记录是否写入文件
    pub save_history: bool,
    /// 在 zsh 提示符下输入命令的颜色
    pub input_color: String,
    /// 状态灯变绿(命令结束)时是否提示音
    pub done_sound: bool,
    /// 窗口在后台时,窗格变绿 / 变橙发系统通知
    pub notify_in_background: bool,
    /// 技能库目录;写过一次后即使为空也以它为准(清空也是一种选择)
    pub skill_library_dirs: Option<Vec<PathBuf>>,
    /// 🧩 面板里折叠的分组,键为 "<页签>/<文件夹>"
    pub loader_groups_collapsed: BTreeSet<String>,
    /// 常用项目:点一下就在当前窗格 cd 进去并启动 claude
    pub claude_dirs: Vec<PathBuf>,
    /// 新标签页(⌘T)和启动时第一个窗格的起始目录;None 为家目录。分屏不用它,跟随原窗格
    pub default_dir: Option<PathBuf>,
    /// 常用项目菜单「新建项目」在哪个目录下建文件夹;None 时用 default_dir,再没有就家目录
    pub projects_dir: Option<PathBuf>,
    /// 窗格列表侧边栏是否展开(⌘B)
    pub show_sidebar: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            default_language: None,
            voice_engine: None,
            adds_punctuation: None,
            live_caption: None,
            spoken_symbols: None,
            ptt_key: None,
            voice_commands: None,
            voice_replacements: None,
            scrollback: None,
            save_history: true,
            input_color: "#5CC8FF".into(),
            done_sound: true,
            notify_in_background: true,
            skill_library_dirs: None,
            loader_groups_collapsed: BTreeSet::new(),
            claude_dirs: Vec::new(),
            default_dir: None,
            projects_dir: None,
            show_sidebar: false,
        }
    }
}

impl Settings {
    pub fn load() -> Settings {
        std::fs::read_to_string(settings_file())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(config_dir())?;
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(settings_file(), json)?;
        self.write_input_color_file()
    }

    /// zsh `line-pre-redraw` 钩子每次重绘读取的颜色文件
    pub fn input_color_file() -> PathBuf {
        config_dir().join("input-color")
    }

    /// 把输入颜色镜像到文件,这样改色能实时作用到所有已打开的标签页
    pub fn write_input_color_file(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(config_dir())?;
        std::fs::write(Self::input_color_file(), format!("{}\n", self.input_color))
    }

    /// 新建项目的父目录:项目目录 → 默认文件夹 → 家目录
    pub fn projects_root(&self) -> PathBuf {
        self.projects_dir
            .clone()
            .or_else(|| self.default_dir.clone())
            .unwrap_or_else(crate::paths::home_dir)
    }

    /// 把 GUI 设置叠加到配置文件之上,得到最终生效的配置
    pub fn apply_to(&self, config: &mut Config) {
        if let Some(lang) = &self.default_language {
            config.voice.language = normalize_language(lang);
        }
        if let Some(engine) = &self.voice_engine {
            config.voice.engine = VoiceEngineKind::parse(engine);
        }
        if let Some(p) = self.adds_punctuation {
            config.voice.punctuation = p;
        }
        if let Some(l) = self.live_caption {
            config.voice.live_caption = l;
        }
        if let Some(s) = self.spoken_symbols {
            config.voice.spoken_symbols = s;
        }
        if let Some(k) = &self.ptt_key {
            config.voice.ptt_key = k.clone();
        }
        if let Some(c) = self.voice_commands {
            config.voice.commands = c;
        }
        if let Some(rules) = &self.voice_replacements {
            config.voice.replacements = rules.clone();
        }
        if let Some(n) = self.scrollback {
            config.scrollback = clamp_scrollback(n);
        }
        if let Some(dirs) = &self.skill_library_dirs {
            config.skill_library_dirs = dedupe(dirs);
        }
    }
}

/// 在 `root` 下新建名为 `name` 的项目目录。返回 (完整路径, 是否新建的);目录已存在不算错,
/// 直接沿用。名字只能是单层文件夹名:不能为空、不能含路径分隔符、不能是 `.` / `..`
pub fn create_project_dir(root: &Path, name: &str) -> Result<(PathBuf, bool), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("请输入项目名".into());
    }
    if name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
        return Err("项目名不能含 / 或 \\,也不能是 . 或 ..".into());
    }
    let dir = root.join(name);
    if dir.is_dir() {
        return Ok((dir, false));
    }
    if dir.exists() {
        return Err(format!("{} 已存在,但不是文件夹", dir.display()));
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建 {} 失败:{e}", dir.display()))?;
    Ok((dir, true))
}

pub fn clamp_scrollback(n: usize) -> usize {
    n.clamp(*SCROLLBACK_RANGE.start(), *SCROLLBACK_RANGE.end())
}

/// 去掉空项与重复项,保留调用方顺序(即面板分组顺序)
fn dedupe(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    dirs.iter()
        .filter(|d| !d.as_os_str().is_empty() && seen.insert((*d).clone()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_and_tolerates_missing_fields() {
        let s: Settings = serde_json::from_str(r#"{"done-sound": false, "default-dir": "/tmp"}"#).unwrap();
        assert!(!s.done_sound);
        assert!(s.save_history);
        assert_eq!(s.default_dir, Some(PathBuf::from("/tmp")));
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn overrides_config() {
        let mut c = Config::default();
        let s = Settings {
            default_language: Some("en-US".into()),
            voice_engine: Some("http".into()),
            scrollback: Some(1),
            skill_library_dirs: Some(vec!["/a".into(), "/a".into(), "".into()]),
            ..Settings::default()
        };
        s.apply_to(&mut c);
        assert_eq!(c.voice.language, "en");
        assert_eq!(c.voice.engine, VoiceEngineKind::Http);
        assert_eq!(c.scrollback, 500);
        assert_eq!(c.skill_library_dirs, vec![PathBuf::from("/a")]);
    }

    #[test]
    fn projects_root_falls_back() {
        let mut s = Settings::default();
        assert_eq!(s.projects_root(), crate::paths::home_dir());
        s.default_dir = Some("/d".into());
        assert_eq!(s.projects_root(), PathBuf::from("/d"));
        s.projects_dir = Some("/p".into());
        assert_eq!(s.projects_root(), PathBuf::from("/p"));
    }

    #[test]
    fn creates_project_dir_and_rejects_bad_names() {
        let root = std::env::temp_dir().join(format!("vg-proj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for bad in ["", "  ", ".", "..", "a/b", "a\\b"] {
            assert!(create_project_dir(&root, bad).is_err(), "{bad:?}");
        }
        let (dir, created) = create_project_dir(&root, " demo ").unwrap();
        assert_eq!(dir, root.join("demo"));
        assert!(created && dir.is_dir());
        // 第二次:已存在,沿用而不报错
        assert_eq!(create_project_dir(&root, "demo").unwrap(), (dir.clone(), false));
        std::fs::write(root.join("file"), b"x").unwrap();
        assert!(create_project_dir(&root, "file").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
