//! 自然语言 → shell 命令。两条路径:
//!   - Claude API(云端,claude.rs):配了 API Key 时优先,能力最强
//!   - 本地 Ollama(ollama.rs):没有 Key 时的兜底,跨平台、离线
//!
//! 结果只送到终端光标处(去掉换行),**永不自动执行**——回车仍由用户自己按。
//!
//! 由配置文件的 `llm-provider` 控制:auto(默认,有 Key 用 Claude,否则本地)| claude | ollama

use serde::Deserialize;

use crate::config::{Config, LlmProvider};
use crate::{claude, ollama};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translation {
    /// 生成的 shell 命令
    pub command: String,
    /// 给用户看的一句话说明,显示在状态栏,降低误执行风险
    pub explanation: String,
}

#[derive(Debug, thiserror::Error)]
pub enum NlError {
    #[error("{0}")]
    Message(String),
}

/// `current_directory`:当前工作目录,喂给模型提高命令准确度
pub fn translate(
    natural_language: &str,
    current_directory: Option<&str>,
    config: &Config,
) -> Result<Translation, NlError> {
    let system = instructions(current_directory, &shell_name());
    let api_key = resolve_api_key(config);

    match (config.llm_provider, api_key) {
        (LlmProvider::Claude, None) => Err(NlError::Message(
            "llm-provider = claude but there is no API Key; please add llm-api-key to the config file"
                .into(),
        )),
        (LlmProvider::Claude | LlmProvider::Auto, Some(key)) => {
            claude::translate(natural_language, &system, &key, &config.llm_model)
        }
        (LlmProvider::Ollama | LlmProvider::Auto, _) => ollama::translate(
            natural_language,
            &system,
            &config.llm_local_model,
            &config.llm_local_url,
        ),
    }
}

/// API Key 来源:配置文件 llm-api-key 优先,其次环境变量 ANTHROPIC_API_KEY
/// (从 Finder/开始菜单启动的 App 看不到 shell 环境变量,所以配置文件是主路径)
pub fn resolve_api_key(config: &Config) -> Option<String> {
    if !config.llm_api_key.is_empty() {
        return Some(config.llm_api_key.clone());
    }
    std::env::var("ANTHROPIC_API_KEY").ok().filter(|k| !k.is_empty())
}

/// 用户的 shell 名(zsh/bash/fish/pwsh…),写进提示词让模型生成对应语法
pub fn shell_name() -> String {
    if cfg!(windows) {
        return "PowerShell".into();
    }
    std::env::var("SHELL")
        .ok()
        .and_then(|s| s.rsplit('/').next().map(str::to_string))
        .unwrap_or_else(|| "sh".into())
}

fn os_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "linux" => "Linux",
        "windows" => "Windows",
        other => other,
    }
}

/// 两条路径共用的系统提示词(Claude 用结构化输出由 API 保证格式;本地模型靠 format 约束 + parse_loose 容错)
pub fn instructions(current_directory: Option<&str>, shell: &str) -> String {
    let mut s = format!(
        "You are a command assistant for the {os} terminal ({shell}). Turn the user's natural-language intent into [one] directly executable shell command.\n\
         Output only a single JSON object, no markdown, no code block, no extra text, in this format:\n\
         {{\"command\": \"<a single shell command, the command only>\", \"explanation\": \"<a one-sentence explanation in the user's language, flagging the risk for dangerous operations>\"}}\n\
         When it cannot be done safely, leave command as an empty string and explain why in explanation.",
        os = os_name(),
    );
    if let Some(cwd) = current_directory.filter(|c| !c.is_empty()) {
        s.push_str("\nCurrent working directory: ");
        s.push_str(cwd);
    }
    s
}

/// 从模型输出里抠出 JSON 解析;找不到就把整段当命令(容错)。
pub fn parse_loose(raw: &str) -> Translation {
    #[derive(Deserialize)]
    struct Raw {
        command: String,
        #[serde(default)]
        explanation: String,
    }
    if let (Some(start), Some(end)) = (raw.find('{'), raw.rfind('}')) {
        if start <= end {
            if let Ok(obj) = serde_json::from_str::<Raw>(&raw[start..=end]) {
                return Translation {
                    command: obj.command.trim().to_string(),
                    explanation: obj.explanation,
                };
            }
        }
    }
    Translation {
        command: raw.trim().to_string(),
        explanation: String::new(),
    }
}

/// 安全底线:识别结果/生成的命令上屏前去掉 CR/LF,防止一个换行就触发执行
pub fn strip_newlines(text: &str) -> String {
    text.chars()
        .map(|c| if c == '\r' || c == '\n' { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_loose_extracts_json() {
        let t = parse_loose("sure! {\"command\": \" df -h \", \"explanation\": \"disk\"} done");
        assert_eq!(t.command, "df -h");
        assert_eq!(t.explanation, "disk");
    }

    #[test]
    fn parse_loose_falls_back_to_whole_text() {
        let t = parse_loose("  ls -la\n");
        assert_eq!(t.command, "ls -la");
        assert!(t.explanation.is_empty());
    }

    #[test]
    fn instructions_include_cwd() {
        let s = instructions(Some("/tmp"), "zsh");
        assert!(s.contains("(zsh)"));
        assert!(s.ends_with("Current working directory: /tmp"));
        assert!(!instructions(None, "zsh").contains("Current working directory"));
    }

    #[test]
    fn strips_newlines() {
        assert_eq!(strip_newlines("rm -rf x\n"), "rm -rf x");
        assert_eq!(strip_newlines("a\r\nb"), "a  b");
    }

    #[test]
    fn claude_without_key_errors() {
        let config = Config {
            llm_provider: LlmProvider::Claude,
            ..Config::default()
        };
        // 测试环境可能设置了 ANTHROPIC_API_KEY;只在未设置时断言
        if std::env::var("ANTHROPIC_API_KEY").map_or(true, |k| k.is_empty()) {
            assert!(translate("list files", None, &config).is_err());
        }
    }
}
