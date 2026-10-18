//! 最小 Claude API 客户端(Rust 没有官方 SDK,直接 POST /v1/messages)。
//! 只做一件事:自然语言 → {command, explanation} JSON。
//! 结构化输出(output_config.format json_schema)由 API 层保证输出结构,客户端无需"抠 JSON"兜底。

use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

use crate::nl2command::{NlError, Translation};

pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
/// 服务端 refusal fallback:安全分类器拒绝时,API 在同一次调用里按拒绝类别换模型重跑
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

pub fn translate(
    natural_language: &str,
    instructions: &str,
    api_key: &str,
    model: &str,
) -> Result<Translation, NlError> {
    let body = json!({
        "model": model,
        "max_tokens": 1024,
        "system": instructions,
        "fallbacks": "default",
        // 语音场景要快:effort=low 降低延迟;json_schema 保证输出恰好是目标 JSON
        "output_config": {
            "effort": "low",
            "format": {
                "type": "json_schema",
                "schema": {
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "A single directly executable shell command, the command only; an empty string when it cannot be done safely"
                        },
                        "explanation": {
                            "type": "string",
                            "description": "A one-sentence explanation, flagging the risk for dangerous operations"
                        }
                    },
                    "required": ["command", "explanation"],
                    "additionalProperties": false
                }
            }
        },
        "messages": [{"role": "user", "content": natural_language}],
    });

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| NlError::Message(format!("Network client error: {e}")))?;

    let response = client
        .post(ENDPOINT)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", FALLBACK_BETA)
        .json(&body)
        .send()
        .map_err(|e| NlError::Message(format!("Network request failed: {e}")))?;

    let status = response.status().as_u16();
    let bytes = response
        .bytes()
        .map_err(|e| NlError::Message(format!("Network request failed: {e}")))?;
    if status != 200 {
        return Err(NlError::Message(error_text(status, &bytes)));
    }
    parse_response(&bytes)
}

fn parse_response(bytes: &[u8]) -> Result<Translation, NlError> {
    let message: MessageResponse = serde_json::from_slice(bytes)
        .map_err(|_| NlError::Message("Unexpected Claude API response".into()))?;

    // 安全分类器拒绝(fallback 也没能救回):HTTP 200 + stop_reason=refusal
    if message.stop_reason.as_deref() == Some("refusal") {
        return Err(NlError::Message(
            "This request was refused by Claude's safety policy; try rephrasing it".into(),
        ));
    }
    // fallback 重跑时 content 里可能先有被中断的块,取最后一个 text 块
    let text = message
        .content
        .iter()
        .rev()
        .find(|b| b.kind == "text")
        .and_then(|b| b.text.as_deref())
        .ok_or_else(|| NlError::Message("Couldn't generate a command; try rephrasing it".into()))?;
    let parsed: RawSuggestion = serde_json::from_str(text)
        .map_err(|_| NlError::Message("Couldn't generate a command; try rephrasing it".into()))?;
    Ok(Translation {
        command: parsed.command.trim().to_string(),
        explanation: parsed.explanation,
    })
}

/// 把 HTTP 错误翻译成一句用户看得懂的话
fn error_text(status: u16, body: &[u8]) -> String {
    // 错误体:{"type":"error","error":{"type":"...","message":"..."}}
    let api_message = serde_json::from_slice::<ErrorResponse>(body)
        .ok()
        .map(|e| e.error.message);
    match status {
        401 => "Invalid API Key; please check the llm-api-key config".into(),
        403 => format!(
            "API Key lacks permission: {}",
            api_message.unwrap_or_else(|| "permission_error".into())
        ),
        429 => "Too many requests (rate limited); please try again later".into(),
        500.. => "Claude service is temporarily unavailable; please try again later".into(),
        _ => format!(
            "Claude API error ({status}): {}",
            api_message.unwrap_or_else(|| "unknown error".into())
        ),
    }
}

#[derive(Deserialize)]
struct MessageResponse {
    content: Vec<ContentBlock>,
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct RawSuggestion {
    command: String,
    explanation: String,
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: ApiError,
}

#[derive(Deserialize)]
struct ApiError {
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_last_text_block() {
        let body = br#"{"content":[{"type":"thinking","thinking":""},{"type":"text","text":"{\"command\":\" ls -la \",\"explanation\":\"list\"}"}],"stop_reason":"end_turn"}"#;
        let t = parse_response(body).unwrap();
        assert_eq!(t.command, "ls -la");
        assert_eq!(t.explanation, "list");
    }

    #[test]
    fn refusal_is_an_error() {
        let body = br#"{"content":[],"stop_reason":"refusal"}"#;
        assert!(parse_response(body).is_err());
    }

    #[test]
    fn maps_status_codes() {
        assert!(error_text(401, b"").contains("Invalid API Key"));
        assert!(error_text(503, b"").contains("temporarily unavailable"));
        let body = br#"{"type":"error","error":{"type":"x","message":"bad model"}}"#;
        assert!(error_text(400, body).contains("bad model"));
    }
}
