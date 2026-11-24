//! GUI 可改的设置,存 `settings.json`(对应 Swift 版的 UserDefaults)。
//! 与只读配置文件共有的 key,GUI 设置优先;`None` 表示"没在 GUI 里设过",回退配置文件。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{normalize_language, Config, LlmProvider, VoiceEngineKind};
use crate::paths::{config_dir, settings_file};

pub const SCROLLBACK_RANGE: std::ops::RangeInclusive<usize> = 500..=200_000;
/// 测试用:之后的 `save()` 都不写盘。截图测试用真实设置启动、又会改设置,不关掉就会把你的 settings.json 改坏
static SAVING_DISABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn disable_saving() {
    SAVING_DISABLED.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// 终端字号(与 ⌘+ / ⌘- 的上下限一致)
pub const FONT_SIZE_RANGE: std::ops::RangeInclusive<f32> = 8.0..=48.0;
/// 会话记录写盘间隔(秒)
pub const HISTORY_SAVE_RANGE: std::ops::RangeInclusive<u32> = 1..=300;

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
    /// 一键下载的 whisper 模型,覆盖 voice-model。文件不在了就当没设,回到按目录找
    pub voice_model: Option<PathBuf>,
    /// 首次启动「下载语音模型」的提示点过「以后再说」,不再自动弹出(状态栏仍可点开)
    pub model_prompt_dismissed: bool,
    /// 覆盖 scrollback
    pub scrollback: Option<usize>,
    /// 每个窗格的会话记录是否写入文件
    pub save_history: bool,
    /// 会话记录每隔多少秒写一次盘(窗格关闭时总会补写一次)
    pub history_save_seconds: u32,
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
    /// 用量洞察:后台扫描 Claude Code 的会话记录、整理台账
    pub insights_enabled: bool,
    /// 用量洞察里点过「不再提示」的规则
    pub insights_muted_rules: BTreeSet<String>,
    /// 后台扫描周期(分钟)
    pub insights_scan_minutes: u32,
    /// 主题 id,覆盖 theme
    pub theme: Option<String>,
    /// 终端字体,覆盖 font-family;空串 = 平台默认等宽字体
    pub font_family: Option<String>,
    /// 终端字号,覆盖 font-size
    pub font_size: Option<f32>,
    /// HTTP 识别接口,覆盖 voice-api-url / voice-api-key / voice-api-model
    pub voice_api_url: Option<String>,
    pub voice_api_key: Option<String>,
    pub voice_api_model: Option<String>,
    /// 外部识别命令,覆盖 voice-command
    pub voice_command: Option<String>,
    /// 自然语言模式:覆盖 llm-provider / llm-model / llm-api-key / llm-local-model / llm-local-url
    pub llm_provider: Option<String>,
    pub llm_model: Option<String>,
    pub llm_api_key: Option<String>,
    pub llm_local_model: Option<String>,
    pub llm_local_url: Option<String>,
    /// 覆盖 correction
    pub llm_correction: Option<bool>,
    /// 覆盖 llm-screen-context
    pub llm_screen_context: Option<bool>,
    /// 单价覆盖:模型 id → [输入, 缓存写 5 分钟, 缓存写 1 小时, 缓存读, 输出](每百万 token 美元)
    pub insights_prices: std::collections::BTreeMap<String, [f64; 5]>,
    /// 状态栏上的快捷按钮,整个应用一套,与窗口 / 标签页无关
    pub quick_buttons: Vec<QuickButton>,
}

/// 状态栏快捷按钮:点一下把 `text` 输入到当前活动窗格
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct QuickButton {
    /// 按钮上显示的名字
    pub label: String,
    /// 输入到终端的内容
    pub text: String,
    /// 输入后是否回车执行;默认不回车,和语音一样由你来按
    pub run: bool,
}

impl QuickButton {
    /// 要写进终端的内容:不执行时去掉末尾换行,免得一点就跑;执行时补一个回车
    pub fn payload(&self) -> Option<(String, bool)> {
        let body = self.text.trim_end_matches(['\r', '\n']);
        if body.is_empty() {
            return None;
        }
        Some((body.to_string(), self.run))
    }

    /// 状态栏上显示的名字:没起名就用内容的开头
    pub fn title(&self) -> String {
        let label = self.label.trim();
        if !label.is_empty() {
            return label.to_string();
        }
        let first = self.text.trim().lines().next().unwrap_or("");
        let mut t: String = first.chars().take(12).collect();
        if first.chars().count() > 12 {
            t.push('…');
        }
        t
    }
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
            voice_model: None,
            model_prompt_dismissed: false,
            scrollback: None,
            save_history: true,
            history_save_seconds: 5,
            input_color: "#5CC8FF".into(),
            done_sound: true,
            notify_in_background: true,
            skill_library_dirs: None,
            loader_groups_collapsed: BTreeSet::new(),
            claude_dirs: Vec::new(),
            default_dir: None,
            projects_dir: None,
            show_sidebar: false,
            insights_enabled: true,
            insights_muted_rules: BTreeSet::new(),
            insights_scan_minutes: 60,
            insights_prices: std::collections::BTreeMap::new(),
            quick_buttons: Vec::new(),
            theme: None,
            font_family: None,
            font_size: None,
            voice_api_url: None,
            voice_api_key: None,
            voice_api_model: None,
            voice_command: None,
            llm_provider: None,
            llm_model: None,
            llm_api_key: None,
            llm_local_model: None,
            llm_local_url: None,
            llm_correction: None,
            llm_screen_context: None,
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
        if SAVING_DISABLED.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }
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
        if let Some(model) = self.voice_model.as_ref().filter(|p| p.is_file()) {
            config.voice.model_path = Some(model.clone());
        }
        if let Some(n) = self.scrollback {
            config.scrollback = clamp_scrollback(n);
        }
        if let Some(dirs) = &self.skill_library_dirs {
            config.skill_library_dirs = dedupe(dirs);
        }
        let set = |dst: &mut String, src: &Option<String>| {
            if let Some(v) = src {
                *dst = v.clone();
            }
        };
        set(&mut config.theme_name, &self.theme);
        set(&mut config.font_family, &self.font_family);
        if let Some(n) = self.font_size.filter(|n| FONT_SIZE_RANGE.contains(n)) {
            config.font_size = n;
        }
        set(&mut config.voice.api_url, &self.voice_api_url);
        set(&mut config.voice.api_key, &self.voice_api_key);
        set(&mut config.voice.api_model, &self.voice_api_model);
        set(&mut config.voice.command, &self.voice_command);
        if let Some(p) = &self.llm_provider {
            config.llm_provider = LlmProvider::parse(p);
        }
        set(&mut config.llm_model, &self.llm_model);
        set(&mut config.llm_api_key, &self.llm_api_key);
        set(&mut config.llm_local_model, &self.llm_local_model);
        set(&mut config.llm_local_url, &self.llm_local_url);
        if let Some(c) = self.llm_correction {
            config.correction_enabled = c;
        }
        if let Some(c) = self.llm_screen_context {
            config.llm_screen_context = c;
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
    fn overrides_appearance_and_llm() {
        let mut c = Config::default();
        let s = Settings {
            theme: Some("dracula".into()),
            font_family: Some(String::new()),
            font_size: Some(200.0), // 超出范围:忽略
            voice_api_url: Some("http://127.0.0.1:8000/v1/audio/transcriptions".into()),
            voice_command: Some("whisper-cli -f {wav}".into()),
            llm_provider: Some("ollama".into()),
            llm_api_key: Some("k".into()),
            llm_correction: Some(false),
            ..Settings::default()
        };
        s.apply_to(&mut c);
        assert_eq!(c.theme_name, "dracula");
        assert_eq!(c.font_size, 13.0);
        assert_eq!(c.voice.api_url, "http://127.0.0.1:8000/v1/audio/transcriptions");
        assert_eq!(c.voice.command, "whisper-cli -f {wav}");
        assert_eq!(c.llm_provider, LlmProvider::Ollama);
        assert_eq!(c.llm_api_key, "k");
        assert!(!c.correction_enabled);
        assert_eq!(c.llm_model, Config::default().llm_model);
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
    fn quick_button_payload_and_title() {
        let b = QuickButton { label: String::new(), text: "git status\n".into(), run: false };
        assert_eq!(b.payload(), Some(("git status".into(), false)));
        assert_eq!(b.title(), "git status");
        let b = QuickButton { label: " 构建 ".into(), text: "cargo build --release --workspace".into(), run: true };
        assert_eq!(b.payload(), Some(("cargo build --release --workspace".into(), true)));
        assert_eq!(b.title(), "构建");
        assert_eq!(QuickButton { text: "cargo build --release".into(), ..Default::default() }.title(), "cargo build …");
        assert_eq!(QuickButton { text: "\n".into(), ..Default::default() }.payload(), None);
        let s: Settings = serde_json::from_str(r#"{"quick-buttons":[{"label":"a","text":"ls"}]}"#).unwrap();
        assert_eq!(s.quick_buttons[0], QuickButton { label: "a".into(), text: "ls".into(), run: false });
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
