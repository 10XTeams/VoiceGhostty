//! GUI 可改的设置,存 `settings.json`(对应 Swift 版的 UserDefaults)。
//! 与只读配置文件共有的 key,GUI 设置优先;`None` 表示"没在 GUI 里设过",回退配置文件。

use std::collections::BTreeSet;
use std::path::PathBuf;

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
    /// 覆盖 scrollback
    pub scrollback: Option<usize>,
    /// 每个窗格的会话记录是否写入文件
    pub save_history: bool,
    /// 在 zsh 提示符下输入命令的颜色
    pub input_color: String,
    /// 状态灯变绿(命令结束)时是否提示音
    pub done_sound: bool,
    /// 技能库目录;写过一次后即使为空也以它为准(清空也是一种选择)
    pub skill_library_dirs: Option<Vec<PathBuf>>,
    /// 🧩 面板里折叠的分组("plugin" / "skill" / "agent")
    pub loader_groups_collapsed: BTreeSet<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            default_language: None,
            voice_engine: None,
            adds_punctuation: None,
            scrollback: None,
            save_history: true,
            input_color: "#5CC8FF".into(),
            done_sound: true,
            skill_library_dirs: None,
            loader_groups_collapsed: BTreeSet::new(),
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
        if let Some(n) = self.scrollback {
            config.scrollback = clamp_scrollback(n);
        }
        if let Some(dirs) = &self.skill_library_dirs {
            config.skill_library_dirs = dedupe(dirs);
        }
    }
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
        let s: Settings = serde_json::from_str(r#"{"done-sound": false}"#).unwrap();
        assert!(!s.done_sound);
        assert!(s.save_history);
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
}
