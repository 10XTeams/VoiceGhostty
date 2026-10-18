//! 用户配置,读自 `~/.config/voiceghostty/config`(与 Swift 版同一个文件,格式兼容)。
//! 极简 `key = value`,`#` 开头为注释,未知 key 与非法行一律忽略。
//!
//! 终端/LLM:font-family, font-size, theme, foreground, background, scrollback,
//!           llm-provider, llm-model, llm-api-key, llm-local-model, llm-local-url, correction,
//!           skill-library-dir
//! 语音(Rust 版新增,识别引擎可插拔):
//!           voice-engine, voice-language, voice-punctuation, voice-api-url, voice-api-key,
//!           voice-api-model, voice-command, voice-model, voice-device, voice-hold-ms

use std::path::PathBuf;

use crate::paths::{config_file, expand_tilde, home_dir};
use crate::theme::{Rgb, Theme};
use crate::{claude, ollama};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LlmProvider {
    /// 有 Claude Key 用 Claude,否则本地 Ollama
    #[default]
    Auto,
    Claude,
    /// 旧配置里的 `apple` 也映射到这里:Apple 端侧模型不跨平台
    Ollama,
}

impl LlmProvider {
    fn parse(v: &str) -> LlmProvider {
        match v.to_ascii_lowercase().as_str() {
            "claude" => LlmProvider::Claude,
            "ollama" | "local" | "apple" => LlmProvider::Ollama,
            _ => LlmProvider::Auto,
        }
    }
}

/// 语音识别引擎。具体实现在 vg-voice crate,这里只描述"用户选了什么"。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VoiceEngineKind {
    /// 按可用性自动挑:内置 whisper(编译进来且有模型)→ 外部命令 → HTTP(有 Key 或本机服务)
    #[default]
    Auto,
    /// OpenAI 兼容的 `/v1/audio/transcriptions`(OpenAI、Groq、本地 faster-whisper-server、whisper.cpp server…)
    Http,
    /// 任意外部命令,`{wav}` `{lang}` 占位符,读 stdout 作为结果(如 whisper.cpp 的 whisper-cli)
    Command,
    /// 进程内 whisper.cpp(需编译时开启 `whisper` feature)
    Whisper,
    /// 关闭语音
    Off,
}

impl VoiceEngineKind {
    pub fn parse(v: &str) -> VoiceEngineKind {
        match v.to_ascii_lowercase().as_str() {
            "http" | "api" | "openai" => VoiceEngineKind::Http,
            "command" | "cmd" | "exec" => VoiceEngineKind::Command,
            "whisper" | "local" => VoiceEngineKind::Whisper,
            "off" | "none" | "disabled" => VoiceEngineKind::Off,
            _ => VoiceEngineKind::Auto,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            VoiceEngineKind::Auto => "auto",
            VoiceEngineKind::Http => "http",
            VoiceEngineKind::Command => "command",
            VoiceEngineKind::Whisper => "whisper",
            VoiceEngineKind::Off => "off",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VoiceConfig {
    pub engine: VoiceEngineKind,
    /// ISO 639-1:"zh" / "en" / "auto"。Swift 版的 zh-CN / en-US 写法同样接受
    pub language: String,
    /// 是否要标点(HTTP 引擎通过 prompt 提示;听写 shell 命令时句号是噪音,可关)
    pub punctuation: bool,
    pub api_url: String,
    /// 为空时回退环境变量 VOICE_API_KEY / OPENAI_API_KEY
    pub api_key: String,
    pub api_model: String,
    /// 外部命令模板,例:`whisper-cli -m ~/models/ggml-base.bin -l {lang} -nt -f {wav}`
    pub command: String,
    /// 内置 whisper 的 ggml 模型路径;不写时在 `<配置目录>/models/` 里找 `ggml-*.bin`
    pub model_path: Option<PathBuf>,
    /// 麦克风设备名(空 = 系统默认)
    pub device: String,
    /// 按住空格多久算"按住说话",低于此值视为普通空格
    pub hold_ms: u64,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        VoiceConfig {
            engine: VoiceEngineKind::Auto,
            language: "zh".into(),
            punctuation: true,
            api_url: "https://api.openai.com/v1/audio/transcriptions".into(),
            api_key: String::new(),
            api_model: "whisper-1".into(),
            command: String::new(),
            model_path: None,
            device: String::new(),
            hold_ms: 250,
        }
    }
}

impl VoiceConfig {
    /// 内置 whisper 要用的模型:配置里写了就用它(不存在则 None),否则取模型目录里的第一个 `ggml-*.bin`
    pub fn resolved_model(&self) -> Option<PathBuf> {
        if let Some(p) = &self.model_path {
            return p.is_file().then(|| p.clone());
        }
        let mut found: Vec<PathBuf> = std::fs::read_dir(models_dir())
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.file_name().is_some_and(|n| {
                        let n = n.to_string_lossy();
                        n.starts_with("ggml-") && n.ends_with(".bin")
                    })
            })
            .collect();
        found.sort();
        found.into_iter().next()
    }

    pub fn resolved_api_key(&self) -> Option<String> {
        if !self.api_key.is_empty() {
            return Some(self.api_key.clone());
        }
        ["VOICE_API_KEY", "OPENAI_API_KEY"]
            .iter()
            .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
    }
}

/// 内置 whisper 默认的模型目录
pub fn models_dir() -> PathBuf {
    crate::paths::config_dir().join("models")
}

/// "zh-CN" → "zh","en-US" → "en","auto" 保持
pub fn normalize_language(v: &str) -> String {
    let v = v.trim().to_ascii_lowercase();
    if v == "auto" {
        return v;
    }
    v.split(['-', '_']).next().unwrap_or("zh").to_string()
}

fn parse_bool(v: &str) -> bool {
    !matches!(v.to_ascii_lowercase().as_str(), "off" | "false" | "no" | "0")
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// 空 = 平台默认等宽字体
    pub font_family: String,
    pub font_size: f32,
    pub theme_name: String,
    pub foreground_override: Option<Rgb>,
    pub background_override: Option<Rgb>,
    /// 每个窗格保留多少行滚出屏幕的历史
    pub scrollback: usize,

    pub llm_provider: LlmProvider,
    pub llm_model: String,
    /// 为空时回退 ANTHROPIC_API_KEY 环境变量
    pub llm_api_key: String,

    /// 转写纠错(仅自然语言模式;听写模式始终原样)
    pub correction_enabled: bool,
    pub llm_local_model: String,
    pub llm_local_url: String,

    /// 技能库目录:面板把每个目录的子文件夹列为可加载技能,一个目录一组
    pub skill_library_dirs: Vec<PathBuf>,

    pub voice: VoiceConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            font_family: String::new(),
            font_size: 13.0,
            theme_name: "dark".into(),
            foreground_override: None,
            background_override: None,
            scrollback: 10_000,
            llm_provider: LlmProvider::Auto,
            llm_model: claude::DEFAULT_MODEL.into(),
            llm_api_key: String::new(),
            correction_enabled: true,
            llm_local_model: ollama::DEFAULT_MODEL.into(),
            llm_local_url: ollama::DEFAULT_BASE_URL.into(),
            skill_library_dirs: vec![default_skill_library_dir()],
            voice: VoiceConfig::default(),
        }
    }
}

pub fn default_skill_library_dir() -> PathBuf {
    home_dir().join(".claude").join("skills")
}

impl Config {
    /// 解析后的最终主题(已应用 fg/bg 覆盖)
    pub fn theme(&self) -> Theme {
        Theme::named(&self.theme_name).overriding(self.foreground_override, self.background_override)
    }

    /// 读取并解析配置文件;文件不存在就用默认值
    pub fn load() -> Config {
        std::fs::read_to_string(config_file())
            .map(|text| Config::parse(&text))
            .unwrap_or_default()
    }

    pub fn parse(text: &str) -> Config {
        let mut config = Config::default();
        // 第一行 skill-library-dir 替换默认值,之后的行追加
        let mut saw_skill_dir = false;

        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let (key, value) = (key.trim(), value.trim());
            if value.is_empty() {
                continue;
            }
            let v = &mut config.voice;
            match key {
                "font-family" => config.font_family = value.into(),
                "font-size" => {
                    if let Ok(n) = value.parse::<f32>() {
                        if n > 0.0 {
                            config.font_size = n;
                        }
                    }
                }
                "theme" => config.theme_name = value.into(),
                "foreground" => config.foreground_override = Rgb::parse(value),
                "background" => config.background_override = Rgb::parse(value),
                "scrollback" => {
                    if let Ok(n) = value.parse() {
                        config.scrollback = n;
                    }
                }
                "llm-provider" => config.llm_provider = LlmProvider::parse(value),
                "llm-model" => config.llm_model = value.into(),
                "llm-api-key" => config.llm_api_key = value.into(),
                "llm-local-model" => config.llm_local_model = value.into(),
                "llm-local-url" => config.llm_local_url = value.into(),
                "correction" => config.correction_enabled = parse_bool(value),
                "skill-library-dir" => {
                    let dirs: Vec<PathBuf> = value
                        .split(',')
                        .map(str::trim)
                        .filter(|d| !d.is_empty())
                        .map(expand_tilde)
                        .collect();
                    if dirs.is_empty() {
                        continue;
                    }
                    if !saw_skill_dir {
                        config.skill_library_dirs.clear();
                    }
                    config.skill_library_dirs.extend(dirs);
                    saw_skill_dir = true;
                }
                "voice-engine" => v.engine = VoiceEngineKind::parse(value),
                "voice-language" => v.language = normalize_language(value),
                "voice-punctuation" => v.punctuation = parse_bool(value),
                "voice-api-url" => v.api_url = value.into(),
                "voice-api-key" => v.api_key = value.into(),
                "voice-api-model" => v.api_model = value.into(),
                "voice-command" => v.command = value.into(),
                "voice-model" => v.model_path = Some(expand_tilde(value)),
                "voice-device" => v.device = value.into(),
                "voice-hold-ms" => {
                    if let Ok(n) = value.parse() {
                        v.hold_ms = n;
                    }
                }
                _ => {} // 未知 key 忽略
            }
        }
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_gives_defaults() {
        assert_eq!(Config::parse(""), Config::default());
    }

    #[test]
    fn parses_known_keys_and_ignores_junk() {
        let c = Config::parse(
            "# comment\n\
             font-size = 15\n\
             theme = dracula\n\
             background = #000000\n\
             llm-provider = apple\n\
             correction = off\n\
             garbage line\n\
             unknown-key = 1\n\
             scrollback = -3\n",
        );
        assert_eq!(c.font_size, 15.0);
        assert_eq!(c.theme().id, "dracula");
        assert_eq!(c.theme().background, Rgb(0, 0, 0));
        assert_eq!(c.llm_provider, LlmProvider::Ollama);
        assert!(!c.correction_enabled);
        assert_eq!(c.scrollback, 10_000);
    }

    #[test]
    fn skill_dirs_replace_then_append() {
        let c = Config::parse("skill-library-dir = /a, /b\nskill-library-dir = /c\n");
        assert_eq!(
            c.skill_library_dirs,
            vec![PathBuf::from("/a"), PathBuf::from("/b"), PathBuf::from("/c")]
        );
    }

    #[test]
    fn parses_voice_section() {
        let c = Config::parse(
            "voice-engine = command\n\
             voice-language = en-US\n\
             voice-punctuation = no\n\
             voice-command = whisper-cli -l {lang} -f {wav}\n\
             voice-hold-ms = 300\n",
        );
        assert_eq!(c.voice.engine, VoiceEngineKind::Command);
        assert_eq!(c.voice.language, "en");
        assert!(!c.voice.punctuation);
        assert_eq!(c.voice.command, "whisper-cli -l {lang} -f {wav}");
        assert_eq!(c.voice.hold_ms, 300);
    }
}
