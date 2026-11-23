//! Claude Code 适配器:找会话文件,把 JSONL 解析成一轮一行的 `TurnRecord`。
//!
//! 文件:`~/.claude/projects/<cwd 换成 ->/<会话 id>.jsonl` 是主对话;
//! `<会话 id>/subagents/agent-<id>.jsonl` 是子代理各自的对话(同名 `.meta.json` 里有 `agentType`)。
//! 字段在 Claude Code 2.1.330 上核对过,解析是宽松的:缺字段就少算一项,不报错。
//!
//! 几个不显然的地方:
//! - 一次 API 响应会按内容块拆成好几条 assistant 记录,`message.id` 相同、`usage` 重复,要按 id 去重。
//! - 一"轮"从用户发出的一句话开始(user 记录,不是工具结果、不是 isMeta),到下一句为止。
//!   本地命令(`/clear` 之类)也是 user 记录,但它们后面没有请求,不会成行。
//! - `system/turn_duration` 不是每轮都有,没有时用这一轮最后一条记录的时间减开始时间。
//! - 增量解析:游标停在"最后一轮"的开头。最后一轮可能还没结束,每次都重新解析它、重新输出,
//!   台账按去重键保留最后写入的那行,所以不会重复计数。

use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::time::parse_rfc3339_ms;
use super::{Agent, TurnRecord};

/// 验证过的最高版本;遇到更新的版本面板里注明"未验证"
pub const VERIFIED_VERSION: &str = "2.1.330";

const TTL_5M_MS: i64 = 5 * 60 * 1000;
const TTL_1H_MS: i64 = 60 * 60 * 1000;

pub fn projects_root() -> PathBuf {
    crate::paths::home_dir().join(".claude").join("projects")
}

/// 一个要扫描的文件
#[derive(Debug, Clone, PartialEq)]
pub struct SourceFile {
    pub path: PathBuf,
    /// 子代理文件:(父会话 id, 子代理 id)
    pub subagent: Option<(String, String)>,
}

/// 列出 projects 根目录下所有主对话和子代理文件
pub fn list_files(root: &Path) -> Vec<SourceFile> {
    let mut out = Vec::new();
    let Ok(projects) = std::fs::read_dir(root) else { return out };
    for project in projects.flatten() {
        let dir = project.path();
        if !dir.is_dir() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "jsonl") {
                out.push(SourceFile { path: p, subagent: None });
            } else if p.is_dir() {
                let session = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let Ok(subs) = std::fs::read_dir(p.join("subagents")) else { continue };
                for s in subs.flatten() {
                    let sp = s.path();
                    if sp.extension().is_some_and(|x| x == "jsonl") {
                        let stem = sp.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        let id = stem.strip_prefix("agent-").unwrap_or(&stem).to_string();
                        out.push(SourceFile { path: sp, subagent: Some((session.clone(), id)) });
                    }
                }
            }
        }
    }
    out
}

/// 每个文件的解析进度。`offset` 指向最后一轮的开头,而不是文件末尾
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Cursor {
    pub offset: u64,
    /// 从 `offset` 开始的那一轮的序号
    pub turn: u32,
    /// `offset` 之前最后一次请求的时刻,判断缓存是否过期用
    pub last_request_ms: Option<i64>,
    /// 这个会话用过 1 小时缓存
    pub uses_1h: bool,
    /// 上次看到的文件大小和修改时间,两者都没变就跳过
    pub size: u64,
    pub mtime_ms: i64,
    /// 记录里见到的 Claude Code 版本
    #[serde(default)]
    pub version: String,
}

/// 解析一个文件从游标开始的部分。返回这次产出的轮次(含可能未结束的最后一轮)和新游标
pub fn parse_file(file: &SourceFile, cursor: &Cursor) -> std::io::Result<(Vec<TurnRecord>, Cursor)> {
    let mut f = File::open(&file.path)?;
    let meta = f.metadata()?;
    let size = meta.len();
    let mtime_ms = meta.modified().map(super::time::system_time_ms).unwrap_or(0);
    // 文件比游标短:被截短或换了个文件,从头来
    let cursor = if size < cursor.offset { Cursor::default() } else { cursor.clone() };
    f.seek(SeekFrom::Start(cursor.offset))?;
    let mut data = Vec::with_capacity((size - cursor.offset) as usize);
    f.read_to_end(&mut data)?;

    let subagent_type = file.subagent.as_ref().map(|_| subagent_type(&file.path)).unwrap_or_default();
    let mut p = Parser::new(file, &cursor, subagent_type);
    let mut start = 0usize;
    while let Some(pos) = data[start..].iter().position(|b| *b == b'\n') {
        p.line(cursor.offset + start as u64, &data[start..start + pos]);
        start += pos + 1;
    }
    // 最后一个换行之后是还在写的半行,不动它;它属于最后一轮,下次从这一轮开头重读
    let (records, mut next) = p.finish();
    next.size = size;
    next.mtime_ms = mtime_ms;
    Ok((records, next))
}

fn subagent_type(jsonl: &Path) -> String {
    let meta = jsonl.with_extension("meta.json");
    std::fs::read_to_string(meta)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get("agentType").and_then(Value::as_str).map(String::from))
        .unwrap_or_default()
}

/// 进行中的一轮
#[derive(Default)]
struct Turn {
    offset: u64,
    start_ms: Option<i64>,
    last_ms: Option<i64>,
    explicit_duration: Option<u64>,
    rec: Option<TurnRecord>,
    seen_messages: HashSet<String>,
    /// 这一轮开始时的缓存状态(游标要记)
    last_request_before: Option<i64>,
    uses_1h_before: bool,
}

struct Parser<'a> {
    file: &'a SourceFile,
    subagent_type: String,
    out: Vec<TurnRecord>,
    turn_index: u32,
    cur: Turn,
    last_request_ms: Option<i64>,
    uses_1h: bool,
    session: String,
    project: String,
    version: String,
}

impl<'a> Parser<'a> {
    fn new(file: &'a SourceFile, c: &Cursor, subagent_type: String) -> Parser<'a> {
        let session = match &file.subagent {
            Some((parent, _)) => parent.clone(),
            None => file.path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        };
        Parser {
            file,
            subagent_type,
            out: Vec::new(),
            turn_index: c.turn,
            cur: Turn { offset: c.offset, last_request_before: c.last_request_ms, uses_1h_before: c.uses_1h, ..Turn::default() },
            last_request_ms: c.last_request_ms,
            uses_1h: c.uses_1h,
            session,
            project: String::new(),
            version: c.version.clone(),
        }
    }

    fn line(&mut self, offset: u64, line: &[u8]) {
        if line.len() <= 2 {
            return;
        }
        let Ok(text) = std::str::from_utf8(line) else { return };
        // 子串预筛:只有这几类记录有用,其余(api-request-blob 之类)可能很大,别逐条解析
        let wanted = text.contains("\"type\":\"assistant\"")
            || text.contains("\"type\":\"user\"")
            || text.contains("turn_duration");
        if !wanted {
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(text) else { return };
        let ty = v.get("type").and_then(Value::as_str).unwrap_or("");
        let ts = v.get("timestamp").and_then(Value::as_str).and_then(parse_rfc3339_ms);
        if self.project.is_empty() {
            if let Some(cwd) = v.get("cwd").and_then(Value::as_str) {
                self.project = cwd.to_string();
            }
        }
        if let Some(ver) = v.get("version").and_then(Value::as_str) {
            if ver != self.version {
                self.version = ver.to_string();
            }
        }
        // 主对话文件里的 sidechain 记录是旧版本把子代理写在一起的,不归主对话
        if self.file.subagent.is_none() && v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            return;
        }
        match ty {
            "user" => {
                if is_prompt(&v) {
                    self.close_turn();
                    self.cur.offset = offset;
                    self.cur.start_ms = ts;
                } else {
                    self.tool_results(&v);
                }
            }
            "assistant" => self.assistant(&v, ts),
            "system" if v.get("subtype").and_then(Value::as_str) == Some("turn_duration") => {
                if let Some(ms) = v.get("durationMs").and_then(Value::as_u64) {
                    self.cur.explicit_duration = Some(ms);
                }
            }
            _ => {}
        }
        if let Some(t) = ts {
            self.cur.last_ms = Some(self.cur.last_ms.map_or(t, |l| l.max(t)));
        }
    }

    fn record(&mut self, ts: Option<i64>) -> &mut TurnRecord {
        let (session, project, sub) = (self.session.clone(), self.project.clone(), self.file.subagent.clone());
        let sub_type = self.subagent_type.clone();
        let index = self.turn_index;
        let start = self.cur.start_ms.or(ts).unwrap_or(0);
        self.cur.rec.get_or_insert_with(|| TurnRecord {
            at_ms: start,
            agent: Agent::Claude,
            project,
            session,
            turn: index,
            model: String::new(),
            input: 0,
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_read: 0,
            output: 0,
            requests: 0,
            duration_ms: 0,
            thinking_ms: 0,
            tool_calls: 0,
            max_tool_output: 0,
            wait_ms: None,
            first_context: 0,
            peak_context: 0,
            cache_rebuild: 0,
            subagent_type: if sub.is_some() { sub_type } else { String::new() },
            subagent_id: sub.map(|(_, id)| id).unwrap_or_default(),
            skills: String::new(),
            skill_bytes: 0,
        })
    }

    fn assistant(&mut self, v: &Value, ts: Option<i64>) {
        let Some(msg) = v.get("message") else { return };
        let model = msg.get("model").and_then(Value::as_str).unwrap_or("");
        // 本地合成的消息(出错提示之类)不是真的请求
        if model == "<synthetic>" {
            return;
        }
        let id = msg.get("id").and_then(Value::as_str).map(String::from);
        let first_sight = match &id {
            Some(id) => self.cur.seen_messages.insert(id.clone()),
            None => true,
        };
        let thinking = v.get("thinkingDurationMs").and_then(Value::as_u64).unwrap_or(0);
        let mut tool_uses = 0u32;
        let mut skills = Vec::new();
        if let Some(blocks) = msg.get("content").and_then(Value::as_array) {
            for b in blocks {
                if b.get("type").and_then(Value::as_str) == Some("tool_use") {
                    tool_uses += 1;
                    if b.get("name").and_then(Value::as_str) == Some("Skill") {
                        if let Some(s) = b.pointer("/input/skill").and_then(Value::as_str) {
                            skills.push(s.to_string());
                        }
                    }
                }
            }
        }

        let usage = if first_sight { msg.get("usage").cloned() } else { None };
        let (last_request, uses_1h) = (self.last_request_ms, self.uses_1h);
        let mut became_1h = false;
        let mut request_at = None;
        {
            let r = self.record(ts);
            if !model.is_empty() {
                r.model = model.to_string();
            }
            r.thinking_ms += thinking;
            r.tool_calls += tool_uses;
            for s in skills {
                if !r.skills.split(',').any(|x| x == s) {
                    if !r.skills.is_empty() {
                        r.skills.push(',');
                    }
                    r.skills.push_str(&s);
                }
            }
            if let Some(u) = usage {
                let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
                let input = n("input_tokens");
                let read = n("cache_read_input_tokens");
                let created = n("cache_creation_input_tokens");
                let (w1h, w5m) = match u.get("cache_creation") {
                    Some(cc) => {
                        let h = cc.get("ephemeral_1h_input_tokens").and_then(Value::as_u64).unwrap_or(0);
                        let m = cc.get("ephemeral_5m_input_tokens").and_then(Value::as_u64).unwrap_or(0);
                        // 分项和总数对不上时,差额按 5 分钟算(默认 TTL)
                        (h, m + created.saturating_sub(h + m))
                    }
                    None => (0, created),
                };
                let output = n("output_tokens");
                let context = input + read + created;
                r.input += input;
                r.cache_read += read;
                r.cache_write_5m += w5m;
                r.cache_write_1h += w1h;
                r.output += output;
                r.requests += 1;
                if r.requests == 1 {
                    r.first_context = context;
                }
                r.peak_context = r.peak_context.max(context + output);
                // 距上一次请求超过 TTL,而且这次写缓存比读缓存多:缓存过期后重建
                if let (Some(prev), Some(now)) = (last_request, ts) {
                    let ttl = if uses_1h { TTL_1H_MS } else { TTL_5M_MS };
                    if now - prev > ttl && created > read {
                        r.cache_rebuild += created;
                    }
                }
                became_1h = w1h > 0;
                request_at = ts;
            }
        }
        if became_1h {
            self.uses_1h = true;
        }
        if request_at.is_some() {
            self.last_request_ms = request_at;
        }
    }

    /// 工具结果:记最大的一项输出体积
    fn tool_results(&mut self, v: &Value) {
        let Some(blocks) = v.pointer("/message/content").and_then(Value::as_array) else { return };
        let mut biggest = 0u64;
        for b in blocks {
            if b.get("type").and_then(Value::as_str) == Some("tool_result") {
                let size = match b.get("content") {
                    Some(Value::String(s)) => s.len(),
                    Some(Value::Array(items)) => items
                        .iter()
                        .map(|i| i.get("text").and_then(Value::as_str).map_or(0, str::len))
                        .sum(),
                    _ => 0,
                } as u64;
                biggest = biggest.max(size);
            }
        }
        if biggest > 0 {
            if let Some(r) = self.cur.rec.as_mut() {
                r.max_tool_output = r.max_tool_output.max(biggest);
            }
        }
    }

    /// 结束当前这一轮:有请求才成行
    fn close_turn(&mut self) {
        let turn = std::mem::take(&mut self.cur);
        if let Some(mut r) = turn.rec {
            if r.requests > 0 {
                r.duration_ms = turn.explicit_duration.unwrap_or_else(|| {
                    match (turn.start_ms.or(Some(r.at_ms)), turn.last_ms) {
                        (Some(a), Some(b)) if b > a => (b - a) as u64,
                        _ => 0,
                    }
                });
                self.out.push(r);
                self.turn_index += 1;
            }
        }
        self.cur.last_request_before = self.last_request_ms;
        self.cur.uses_1h_before = self.uses_1h;
    }

    fn finish(mut self) -> (Vec<TurnRecord>, Cursor) {
        // 最后一轮照样输出(可能还没结束),但游标停在它开头,下次重新解析
        let offset = self.cur.offset;
        let (before, before_1h) = (self.cur.last_request_before, self.cur.uses_1h_before);
        let index = self.turn_index;
        self.close_turn();
        let cursor = Cursor {
            offset,
            turn: index,
            last_request_ms: before,
            uses_1h: before_1h,
            size: 0,
            mtime_ms: 0,
            version: self.version.clone(),
        };
        (self.out, cursor)
    }
}

/// 用户真正发出的一句话:不是工具结果、不是系统注入的 isMeta
fn is_prompt(v: &Value) -> bool {
    if v.get("isMeta").and_then(Value::as_bool) == Some(true) || v.get("toolUseResult").is_some() {
        return false;
    }
    match v.pointer("/message/content") {
        Some(Value::String(_)) => true,
        Some(Value::Array(items)) => {
            !items.iter().any(|i| i.get("type").and_then(Value::as_str) == Some("tool_result"))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn prompt(ts: &str) -> String {
        format!(r#"{{"type":"user","isSidechain":false,"cwd":"/w/p","sessionId":"s1","version":"2.1.330","timestamp":"{ts}","message":{{"role":"user","content":"hi"}}}}"#)
    }

    fn reply(ts: &str, id: &str, u: (u64, u64, u64, u64), one_hour: bool) -> String {
        let (w5, w1) = if one_hour { (0, u.1) } else { (u.1, 0) };
        format!(
            r#"{{"type":"assistant","isSidechain":false,"timestamp":"{ts}","thinkingDurationMs":100,"message":{{"model":"claude-opus-5-5","id":"{id}","content":[{{"type":"tool_use","name":"Skill","input":{{"skill":"pdf"}}}}],"usage":{{"input_tokens":{},"cache_creation_input_tokens":{},"cache_read_input_tokens":{},"output_tokens":{},"cache_creation":{{"ephemeral_5m_input_tokens":{w5},"ephemeral_1h_input_tokens":{w1}}}}}}}}}"#,
            u.0, u.1, u.2, u.3
        )
    }

    fn tool_result(ts: &str, size: usize) -> String {
        format!(
            r#"{{"type":"user","timestamp":"{ts}","toolUseResult":{{}},"message":{{"role":"user","content":[{{"type":"tool_result","content":"{}"}}]}}}}"#,
            "x".repeat(size)
        )
    }

    fn write(path: &Path, lines: &[String]) {
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
    }

    #[test]
    fn splits_turns_dedupes_messages_and_resumes_from_last_turn() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("s1.jsonl");
        let file = SourceFile { path: path.clone(), subagent: None };
        write(
            &path,
            &[
                prompt("2026-11-22T10:00:00Z"),
                reply("2026-11-22T10:00:05Z", "m1", (2, 1000, 0, 50), true),
                // 同一条响应拆成的第二条记录:usage 不能再算一次
                reply("2026-11-22T10:00:05Z", "m1", (2, 1000, 0, 50), true),
                tool_result("2026-11-22T10:00:06Z", 300),
                reply("2026-11-22T10:00:09Z", "m2", (1, 10, 1000, 20), true),
                r#"{"type":"system","subtype":"turn_duration","durationMs":9500,"timestamp":"2026-11-22T10:00:10Z"}"#.into(),
                prompt("2026-11-22T12:00:00Z"),
            ],
        );
        let (recs, cur) = parse_file(&file, &Cursor::default()).unwrap();
        assert_eq!(recs.len(), 1, "第二轮还没有请求,不成行");
        let r = &recs[0];
        assert_eq!((r.input, r.cache_write_1h, r.cache_read, r.output, r.requests), (3, 1010, 1000, 70, 2));
        assert_eq!(r.duration_ms, 9500);
        assert_eq!(r.thinking_ms, 300, "去重只针对 usage,思考时长每条记录各自记");
        assert_eq!(r.max_tool_output, 300);
        assert_eq!(r.first_context, 1002);
        assert_eq!(r.skills, "pdf");
        assert_eq!(r.project, "/w/p");
        assert_eq!(r.session, "s1");
        assert_eq!(cur.turn, 1);
        assert!(cur.uses_1h);

        // 两小时后继续:超过 1 小时 TTL,缓存重建
        write(&path, &[reply("2026-11-22T12:00:03Z", "m3", (5, 2000, 0, 10), true)]);
        let (recs, cur2) = parse_file(&file, &cur).unwrap();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].turn, 1);
        assert_eq!(recs[0].cache_rebuild, 2000);
        assert_eq!(recs[0].duration_ms, 3000, "没有 turn_duration 时用时间戳差");
        assert_eq!(cur2.offset, cur.offset, "最后一轮可能没结束,游标不越过它");

        // 再读一次:同一轮再输出一遍(台账按键去重),数字不变
        let (again, _) = parse_file(&file, &cur2).unwrap();
        assert_eq!(again, recs);
    }

    #[test]
    fn truncated_file_restarts_from_zero() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("s1.jsonl");
        let file = SourceFile { path: path.clone(), subagent: None };
        write(&path, &[prompt("2026-11-22T10:00:00Z"), reply("2026-11-22T10:00:05Z", "m1", (1, 1, 1, 1), false)]);
        let stale = Cursor { offset: 1_000_000, turn: 7, ..Cursor::default() };
        let (recs, _) = parse_file(&file, &stale).unwrap();
        assert_eq!(recs[0].turn, 0);
    }

    #[test]
    fn lists_and_labels_subagents() {
        let d = tempfile::tempdir().unwrap();
        let proj = d.path().join("-w-p");
        let subs = proj.join("s1/subagents");
        std::fs::create_dir_all(&subs).unwrap();
        write(&proj.join("s1.jsonl"), &[prompt("2026-11-22T10:00:00Z")]);
        let sub = subs.join("agent-abc.jsonl");
        let mut first = prompt("2026-11-22T10:01:00Z");
        first = first.replace("\"isSidechain\":false", "\"isSidechain\":true");
        write(&sub, &[first, reply("2026-11-22T10:01:05Z", "m9", (1, 2, 3, 4), false).replace("\"isSidechain\":false", "\"isSidechain\":true")]);
        std::fs::write(subs.join("agent-abc.meta.json"), r#"{"agentType":"Explore"}"#).unwrap();

        let mut files = list_files(d.path());
        files.sort_by(|a, b| a.path.cmp(&b.path));
        assert_eq!(files.len(), 2);
        let sf = files.iter().find(|f| f.subagent.is_some()).unwrap();
        assert_eq!(sf.subagent, Some(("s1".into(), "abc".into())));
        let (recs, _) = parse_file(sf, &Cursor::default()).unwrap();
        assert_eq!(recs.len(), 1);
        assert_eq!((recs[0].session.as_str(), recs[0].subagent_id.as_str(), recs[0].subagent_type.as_str()), ("s1", "abc", "Explore"));
        assert_eq!(recs[0].tokens(), 10);
    }
}
