//! 本地离线小模型(Ollama,默认 http://127.0.0.1:11434)。两个用途:
//! 1. **转写纠错**:修同音错字、去语气词。只在自然语言路径上跑,听写模式原样上屏。
//! 2. **本地命令生成**:没有 Claude API Key 时的跨平台兜底(替代 Swift 版的 Apple 端侧模型)。
//!
//! ★ 必须绕过系统代理直连本机:系统代理(Clash/Surge 等)会把 127.0.0.1 的请求也转发出去,
//!   实测经代理后模型输出异常冗长直到超时。reqwest 用 `.no_proxy()` 关掉代理。
//! qwen3 的 think 开关在 Ollama 的 chatml 模板下不可靠,真正压住思考输出的是 format(JSON Schema 语法)约束,所以 format 必须保留。

use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::nl2command::{parse_loose, NlError, Translation};

pub const DEFAULT_MODEL: &str = "qwen3:1.7b";
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:11434";

/// Ollama 服务没开/连不上/输出无法解析。调用方据此静默回退(纠错失败就用原文)
#[derive(Debug, thiserror::Error)]
#[error("Ollama unavailable")]
pub struct Unavailable;

/// few-shot 必不可少:1.7B 级别的小模型只给规则会乱删实词,给了例子行为才稳定。
const CORRECTION_INSTRUCTIONS: &str = "\
You are a text tidier for voice dictation, responsible for cleaning up speech-recognition output. Rules:
1. Fix obvious homophones/typos
2. Remove filler words (um, uh, ah, oh, like, you know, 嗯, 啊, 那个, 就是说, and similar verbal tics) and repeated stutters
3. Keep everything else exactly as is: don't rewrite the sentence structure, don't add or remove content words, don't translate, don't answer questions. Copy English, commands, paths, and numbers verbatim.

Examples:
Input: um so restart the service for me
Output: restart the service for me
Input: delete delete this this file
Output: delete this file
Input: 嗯那个帮我看看磁盘还剩多少
Output: 帮我看看磁盘还剩多少
Input: uh show the currant port usage
Output: show the current port usage";

fn client(timeout: Duration) -> Result<reqwest::blocking::Client, Unavailable> {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(timeout)
        .build()
        .map_err(|_| Unavailable)
}

/// 纠正一句听写。模型输出异常时返回原文——纠错只能锦上添花,绝不能丢用户的话。
pub fn correct(text: &str, model: &str, base_url: &str) -> Result<String, Unavailable> {
    let schema = json!({
        "type": "object",
        "properties": {"text": {"type": "string"}},
        "required": ["text"],
    });
    // 纠错必须无感:模型热身后一句 <1 秒;超过 15 秒还不如用原文
    let raw = chat(CORRECTION_INSTRUCTIONS, text, model, base_url, schema, Duration::from_secs(15))?;
    let parsed: Value = serde_json::from_str(&raw).map_err(|_| Unavailable)?;
    let corrected = parsed.get("text").and_then(Value::as_str).ok_or(Unavailable)?;
    Ok(sanitize(corrected, text).unwrap_or_else(|| text.to_string()))
}

/// 本地模型生成命令(无 Claude Key 时的兜底)
pub fn translate(
    natural_language: &str,
    instructions: &str,
    model: &str,
    base_url: &str,
) -> Result<Translation, NlError> {
    let schema = json!({
        "type": "object",
        "properties": {
            "command": {"type": "string"},
            "explanation": {"type": "string"},
        },
        "required": ["command", "explanation"],
    });
    let raw = chat(instructions, natural_language, model, base_url, schema, Duration::from_secs(60))
        .map_err(|_| {
            NlError::Message(format!(
                "Local model unavailable: is Ollama running at {base_url} with `{model}` pulled?"
            ))
        })?;
    let t = parse_loose(&raw);
    if t.command.is_empty() {
        return Err(NlError::Message(if t.explanation.is_empty() {
            "Couldn't generate a command; try rephrasing it".into()
        } else {
            t.explanation
        }));
    }
    Ok(t)
}

/// 输出清洗 + 安全阀。
/// - 去掉 /no_think、/think:Ollama qwen3 chatml 模板注入的软开关会被模型回显
/// - 长度突变(相对原文缩太多/涨太多)视为模型跑偏,回退原文
pub fn sanitize(raw: &str, original: &str) -> Option<String> {
    let cleaned = raw.replace("/no_think", "").replace("/think", "");
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return None;
    }
    let (new_len, old_len) = (cleaned.chars().count() as f64, original.chars().count() as f64);
    // 很短的输入(如 "嗯 ls")本来就会压缩很多,只防膨胀
    if old_len > 6.0 && new_len / old_len < 0.5 {
        return None;
    }
    if new_len > old_len * 1.5 {
        return None;
    }
    Some(cleaned.to_string())
}

fn chat(
    system: &str,
    user: &str,
    model: &str,
    base_url: &str,
    format: Value,
    timeout: Duration,
) -> Result<String, Unavailable> {
    let body = json!({
        "model": model,
        "stream": false,
        // 模型常驻 30 分钟:Ollama 默认 5 分钟卸载,冷加载十几秒会像卡死
        "keep_alive": "30m",
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "format": format,
        "options": {"temperature": 0},
    });
    let response = client(timeout)?
        .post(format!("{}/api/chat", base_url.trim_end_matches('/')))
        .json(&body)
        .send()
        .map_err(|_| Unavailable)?;
    if !response.status().is_success() {
        return Err(Unavailable);
    }
    let reply: ChatResponse = response.json().map_err(|_| Unavailable)?;
    if reply.message.content.trim().is_empty() {
        return Err(Unavailable);
    }
    Ok(reply.message.content)
}

/// 预热:让 Ollama 把模型载入内存(空 prompt 的 /api/generate 只触发加载)。
/// 启动时后台调用,把冷加载挪到用户开口之前;失败静默忽略。
pub fn warm_up(model: &str, base_url: &str) {
    if let Ok(c) = client(Duration::from_secs(120)) {
        let _ = c
            .post(format!("{}/api/generate", base_url.trim_end_matches('/')))
            .json(&json!({"model": model, "keep_alive": "30m"}))
            .send();
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_think_tags() {
        assert_eq!(
            sanitize("/no_think restart the service", "um restart the service").as_deref(),
            Some("restart the service")
        );
    }

    #[test]
    fn sanitize_rejects_shrink_and_balloon() {
        assert_eq!(sanitize("hi", "please restart the web service now"), None);
        assert_eq!(sanitize("a very very long rambling answer", "ls -la"), None);
        // 短输入允许大幅压缩
        assert_eq!(sanitize("ls", "嗯 ls").as_deref(), Some("ls"));
    }
}
