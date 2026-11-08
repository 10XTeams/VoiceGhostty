//! 可插拔识别引擎。所有引擎都是"整段录音 → 文本"(按住说话,松开后识别)。
//!
//! - [`HttpEngine`]:OpenAI 兼容 `/v1/audio/transcriptions`。OpenAI、Groq、Azure 等云服务,
//!   以及本机的 faster-whisper-server / whisper.cpp server / speaches 都走这个协议
//! - [`CommandEngine`]:任意外部命令,`{wav}` `{lang}` 占位,读 stdout。最灵活,例如 whisper.cpp 的 `whisper-cli`

use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use serde::Deserialize;

use crate::clip::AudioClip;

#[derive(Debug, Clone)]
pub struct TranscribeOptions {
    /// "zh" / "en" / "auto"
    pub language: String,
    pub punctuation: bool,
    /// 热词(项目名、文件名、分支名…),附在提示词末尾让模型认得这些拼法
    pub hotwords: Vec<String>,
}

impl Default for TranscribeOptions {
    fn default() -> Self {
        TranscribeOptions { language: "auto".into(), punctuation: true, hotwords: Vec::new() }
    }
}

impl TranscribeOptions {
    /// 固定部分的提示词:中文时要求简体(否则常出繁体),关标点时给无标点示例。
    /// 复读检测只跟这部分比:热词本身就是用户可能说出口的词,不能因为识别结果正好是个文件名就当成幻听
    pub fn base_prompt(&self) -> Option<String> {
        match (self.language.as_str(), self.punctuation) {
            ("zh", true) => Some("以下是普通话的句子，使用简体中文。".into()),
            ("zh", false) => Some("以下是普通话的句子 使用简体中文".into()),
            (_, false) => Some("lowercase no punctuation".into()),
            _ => None,
        }
    }

    /// 实际送给引擎的提示词:固定部分 + 热词列表
    pub fn prompt(&self) -> Option<String> {
        let base = self.base_prompt();
        if self.hotwords.is_empty() {
            return base;
        }
        let list = self.hotwords.join(", ");
        let words = if self.language == "zh" { format!("词汇:{list}") } else { format!("Vocabulary: {list}") };
        Some(match base {
            Some(b) => format!("{b} {words}"),
            None => words,
        })
    }
}

pub trait SpeechEngine: Send + Sync {
    /// 显示在状态栏的名字
    fn name(&self) -> String;
    /// `clip` 已是 16 kHz 单声道、已裁静音
    fn transcribe(&self, clip: &AudioClip, opts: &TranscribeOptions) -> anyhow::Result<String>;
    /// 录音期间能否每秒多跑几次识别做实时字幕。只有本机引擎(进程内 whisper、本机 HTTP 服务)
    /// 才值得:云端按次计费,外部命令每次要起进程
    fn supports_live(&self) -> bool {
        false
    }
}

// MARK: - HTTP(OpenAI 兼容)

pub struct HttpEngine {
    pub url: String,
    pub api_key: Option<String>,
    pub model: String,
}

impl HttpEngine {
    /// 指向本机服务(不需要 Key,且必须绕过系统代理)
    pub fn is_local(&self) -> bool {
        ["://127.0.0.1", "://localhost", "://[::1]", "://0.0.0.0"]
            .iter()
            .any(|h| self.url.contains(h))
    }
}

impl SpeechEngine for HttpEngine {
    fn name(&self) -> String {
        format!("HTTP · {}", self.model)
    }

    fn supports_live(&self) -> bool {
        self.is_local()
    }

    fn transcribe(&self, clip: &AudioClip, opts: &TranscribeOptions) -> anyhow::Result<String> {
        use reqwest::blocking::multipart::{Form, Part};

        let wav = clip.to_wav_bytes()?;
        let mut form = Form::new()
            .text("model", self.model.clone())
            .text("response_format", "json")
            .part("file", Part::bytes(wav).file_name("audio.wav").mime_str("audio/wav")?);
        if opts.language != "auto" {
            form = form.text("language", opts.language.clone());
        }
        if let Some(prompt) = opts.prompt() {
            form = form.text("prompt", prompt);
        }

        let mut builder = reqwest::blocking::Client::builder().timeout(Duration::from_secs(60));
        // 本机服务必须绕过系统代理(见 vg-core::ollama 的说明)
        if self.is_local() {
            builder = builder.no_proxy();
        }
        let mut request = builder.build()?.post(&self.url).multipart(form);
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }
        let response = request.send().context("speech API request failed")?;
        let status = response.status();
        let body = response.text().unwrap_or_default();
        if !status.is_success() {
            bail!("speech API error ({}): {}", status.as_u16(), api_error_message(&body));
        }
        #[derive(Deserialize)]
        struct Reply {
            text: String,
        }
        let reply: Reply = serde_json::from_str(&body)
            .map_err(|_| anyhow!("unexpected speech API response: {}", truncate(&body, 200)))?;
        Ok(clean_transcript(&reply.text))
    }
}

fn api_error_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .or_else(|| v.get("error"))
                .or_else(|| v.get("detail"))
                .map(|m| m.as_str().map(str::to_string).unwrap_or_else(|| m.to_string()))
        })
        .unwrap_or_else(|| truncate(body, 200))
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

// MARK: - 外部命令

pub struct CommandEngine {
    /// 例:`whisper-cli -m ~/models/ggml-base.bin -l {lang} -nt -f {wav}`
    pub template: String,
}

impl CommandEngine {
    /// 替换占位符。路径按平台 shell 规则加引号,防止空格/特殊字符把命令拆坏。
    fn render(&self, wav_path: &str, lang: &str) -> String {
        self.template
            .replace("{wav}", &shell_quote(wav_path))
            .replace("{lang}", &shell_quote(lang))
    }
}

impl SpeechEngine for CommandEngine {
    fn name(&self) -> String {
        let program = self.template.split_whitespace().next().unwrap_or("command");
        format!("Command · {program}")
    }

    fn transcribe(&self, clip: &AudioClip, opts: &TranscribeOptions) -> anyhow::Result<String> {
        let mut file = tempfile::Builder::new().prefix("vg-voice-").suffix(".wav").tempfile()?;
        std::io::Write::write_all(&mut file, &clip.to_wav_bytes()?)?;
        let path = file.path().to_string_lossy().into_owned();
        let command_line = self.render(&path, &opts.language);

        let output = if cfg!(windows) {
            Command::new("cmd").args(["/C", &command_line]).output()
        } else {
            Command::new("sh").args(["-c", &command_line]).output()
        }
        .with_context(|| format!("failed to run voice-command: {command_line}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "voice-command exited with {}: {}",
                output.status,
                truncate(stderr.trim(), 300)
            );
        }
        Ok(clean_transcript(&String::from_utf8_lossy(&output.stdout)))
    }
}

fn shell_quote(s: &str) -> String {
    if cfg!(windows) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// 统一清洗:多行拼成一行,去掉 Whisper 的 [BLANK_AUDIO] / [Music] / (音乐) 之类标注
pub fn clean_transcript(raw: &str) -> String {
    let mut out = String::new();
    for line in raw.lines() {
        let line = strip_annotations(line.trim());
        if line.is_empty() {
            continue;
        }
        if !out.is_empty() && needs_space(&out, &line) {
            out.push(' ');
        }
        out.push_str(&line);
    }
    out
}

fn strip_annotations(line: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in line.chars() {
        match c {
            '[' => depth += 1,
            ']' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

/// Whisper 在静音 / 纯噪音上最常"幻听"出的句子:训练语料里字幕收尾的套话。
/// 比较前先归一化(去掉标点、空白,转小写),整句命中即视为没听到声音
const HALLUCINATED_SENTENCES: &[&str] = &[
    "谢谢观看",
    "謝謝觀看",
    "感谢观看",
    "感謝觀看",
    "谢谢收看",
    "謝謝收看",
    "谢谢大家",
    "謝謝大家",
    "谢谢",
    "謝謝",
    "再见",
    "再見",
    "thankyou",
    "thankyouverymuch",
    "thanksforwatching",
    "thankyouforwatching",
    "thankyousomuchforwatching",
    "seeyounexttime",
    "bye",
    "you",
    "ご視聴ありがとうございました",
    "시청해주셔서감사합니다",
];
/// 出现这些片段的,不管整句是什么都是幻听(字幕组署名)
const HALLUCINATION_MARKERS: &[&str] = &[
    "amaraorg",
    "明镜与点点",
    "明鏡與點點",
    "点赞订阅转发打赏",
    "點贊訂閱轉發打賞",
    "优优独播剧场",
    "優優獨播劇場",
    "字幕志愿者",
    "字幕志願者",
    "字幕由",
    "字幕提供",
    "subtitlesby",
    "字幕製作",
    "字幕制作",
];

/// 只留字母数字(含汉字)并转小写,比较时忽略标点、空白与大小写
fn normalize(text: &str) -> Vec<char> {
    text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// 识别结果是不是 Whisper 对静音 / 噪音的幻听(见 [`HALLUCINATED_SENTENCES`])
pub fn is_hallucination(text: &str) -> bool {
    let norm: String = normalize(text).into_iter().collect();
    if norm.is_empty() {
        return true;
    }
    HALLUCINATED_SENTENCES.contains(&norm.as_str())
        || HALLUCINATION_MARKERS.iter().any(|m| norm.contains(m))
}

/// 识别结果是不是在复读提示词。没有语音时 Whisper 常把 initial prompt 吐回来(噪音 → "再用简体中文")。
/// 判定:与提示词的最长公共子串至少 4 个字,且覆盖了识别结果的七成以上。
pub fn echoes_prompt(text: &str, prompt: &str) -> bool {
    let t = normalize(text);
    let p = normalize(prompt);
    if t.is_empty() || p.is_empty() {
        return false;
    }
    let mut best = 0;
    let mut prev = vec![0usize; p.len() + 1];
    for &a in &t {
        let mut cur = vec![0usize; p.len() + 1];
        for (j, &b) in p.iter().enumerate() {
            if a == b {
                cur[j + 1] = prev[j] + 1;
                best = best.max(cur[j + 1]);
            }
        }
        prev = cur;
    }
    best >= 4 && best * 10 >= t.len() * 7
}

/// 中文与中文之间不加空格,其他情况加
fn needs_space(prev: &str, next: &str) -> bool {
    let is_cjk = |c: char| ('\u{2E80}'..='\u{9FFF}').contains(&c) || ('\u{FF00}'..='\u{FFEF}').contains(&c);
    let a = prev.chars().last().map(is_cjk).unwrap_or(false);
    let b = next.chars().next().map(is_cjk).unwrap_or(false);
    !(a && b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_whisper_output() {
        assert_eq!(clean_transcript(" [BLANK_AUDIO]\n"), "");
        assert_eq!(clean_transcript(" hello world\n how are you\n"), "hello world how are you");
        assert_eq!(clean_transcript("帮我看看\n磁盘空间\n"), "帮我看看磁盘空间");
        assert_eq!(clean_transcript("[Music] list files"), "list files");
    }

    #[test]
    fn detects_whisper_hallucinations() {
        assert!(is_hallucination("谢谢观看。"));
        assert!(is_hallucination(" Thank you. "));
        assert!(is_hallucination("Thanks for watching!"));
        assert!(is_hallucination("字幕由Amara.org社区提供"));
        assert!(is_hallucination("请不吝点赞 订阅 转发 打赏支持明镜与点点栏目"));
        assert!(is_hallucination("优优独播剧场——YoYo Television Series Exclusive"));
        assert!(is_hallucination("..."));
        assert!(!is_hallucination("列出当前目录"));
        assert!(!is_hallucination("git status"));
        assert!(!is_hallucination("thank you for the review, merge it"));
    }

    #[test]
    fn detects_prompt_echo() {
        let zh = TranscribeOptions { language: "zh".into(), punctuation: true, ..Default::default() }.prompt().unwrap();
        assert!(echoes_prompt("再用简体中文。", &zh));
        assert!(echoes_prompt("以下是普通话的句子", &zh));
        assert!(echoes_prompt("使用简体中文", &zh));
        assert!(!echoes_prompt("使用简体中文写一个 README 放到仓库根目录", &zh));
        assert!(!echoes_prompt("帮我看看磁盘空间还剩多少", &zh));
        assert!(!echoes_prompt("是", &zh));
        let en = TranscribeOptions { language: "en".into(), punctuation: false, ..Default::default() }.prompt().unwrap();
        assert!(echoes_prompt("Lowercase, no punctuation.", &en));
        assert!(!echoes_prompt("list files", &en));
    }

    #[test]
    fn hotwords_append_to_prompt_but_not_to_echo_check() {
        let opts = TranscribeOptions {
            language: "zh".into(),
            punctuation: true,
            hotwords: vec!["VoiceGhostty".into(), "Cargo.toml".into()],
        };
        assert_eq!(opts.prompt().unwrap(), "以下是普通话的句子，使用简体中文。 词汇:VoiceGhostty, Cargo.toml");
        assert_eq!(opts.base_prompt().unwrap(), "以下是普通话的句子，使用简体中文。");
        // 识别结果恰好是个热词:与固定提示词不相似,不能被当成复读
        assert!(!echoes_prompt("VoiceGhostty", &opts.base_prompt().unwrap()));
        let en = TranscribeOptions { language: "en".into(), hotwords: vec!["egui".into()], ..Default::default() };
        assert_eq!(en.prompt().unwrap(), "Vocabulary: egui");
    }

    #[cfg(unix)]
    #[test]
    fn renders_template_with_quoting() {
        let e = CommandEngine { template: "whisper-cli -l {lang} -f {wav}".into() };
        assert_eq!(
            e.render("/tmp/a b's.wav", "zh"),
            r"whisper-cli -l 'zh' -f '/tmp/a b'\''s.wav'"
        );
    }

    #[cfg(unix)]
    #[test]
    fn command_engine_reads_stdout() {
        // 用 wc -c 当"识别器":输出 WAV 字节数,验证临时文件确实写入并传进了命令
        let e = CommandEngine { template: "wc -c < {wav} | tr -d ' '".into() };
        let clip = AudioClip { samples: vec![0.1; 100], sample_rate: 16_000 };
        let opts = TranscribeOptions { language: "en".into(), punctuation: true, ..Default::default() };
        assert_eq!(e.transcribe(&clip, &opts).unwrap(), "244");
    }

    #[cfg(unix)]
    #[test]
    fn command_engine_reports_failure() {
        let e = CommandEngine { template: "echo boom >&2; exit 3".into() };
        let clip = AudioClip { samples: vec![0.1; 10], sample_rate: 16_000 };
        let opts = TranscribeOptions { language: "en".into(), punctuation: true, ..Default::default() };
        let err = e.transcribe(&clip, &opts).unwrap_err().to_string();
        assert!(err.contains("boom"), "{err}");
    }

    #[test]
    fn extracts_api_error_messages() {
        assert_eq!(api_error_message(r#"{"error":{"message":"bad key"}}"#), "bad key");
        assert_eq!(api_error_message(r#"{"detail":"nope"}"#), "nope");
        assert_eq!(api_error_message("plain"), "plain");
    }
}
