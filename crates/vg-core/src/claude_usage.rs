//! 读取窗格里 Claude Code 会话的上下文占用,来源是 claude 自己的 transcript。
//!
//! Claude Code 把每条记录追加到 `~/.claude/projects/<cwd 转成的目录名>/<session>.jsonl`,每条 assistant
//! 记录都带着该次请求的 `usage`。读它比抓 TUI 屏幕强:不花 API、不需要窗格配合、claude 改界面也不会坏。
//! 代价是看不到进行中的请求——记录在响应完成后才写,所以读数比实时会话晚一轮。
//!
//! 文件增量消费(`offset` + `carry`):第一次读完整个文件(模型信息在开头附近),之后每次只读新追加的字节。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use crate::paths::home_dir;

pub const STANDARD_LIMIT: u64 = 200_000;
pub const LONG_LIMIT: u64 = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextUsage {
    /// 上一次请求的 prompt + output tokens,约等于下一次请求要带的量
    pub tokens: u64,
    /// 上下文窗口:200K,transcript 里的模型带 `[1m]` 时为 1M
    pub limit: u64,
    pub model: Option<String>,
    pub session_id: Option<String>,
    pub transcript: PathBuf,
}

impl ContextUsage {
    pub fn fraction(&self) -> f64 {
        if self.limit == 0 {
            0.0
        } else {
            (self.tokens as f64 / self.limit as f64).min(1.0)
        }
    }

    pub fn percent(&self) -> u32 {
        (self.fraction() * 100.0).round() as u32
    }

    /// "65K / 1M"
    pub fn short(&self) -> String {
        format!("{} / {}", compact(self.tokens), compact(self.limit))
    }
}

pub fn compact(n: u64) -> String {
    if n >= 1_000_000 {
        let m = n as f64 / 1_000_000.0;
        return if m >= 10.0 || m == m.round() {
            format!("{}M", m.round() as u64)
        } else {
            format!("{m:.1}M")
        };
    }
    if n >= 1_000 {
        return format!("{}K", (n as f64 / 1_000.0).round() as u64);
    }
    n.to_string()
}

#[derive(Debug, Default)]
pub struct UsageWatcher {
    path: Option<PathBuf>,
    offset: u64,
    carry: Vec<u8>,
    tokens: Option<u64>,
    model: Option<String>,
    /// 来自 transcript 的模型附件,和 `message.model` 不同,它保留 `[1m]` 后缀
    model_id: Option<String>,
    session_id: Option<String>,
    projects_root: Option<PathBuf>,
}

impl UsageWatcher {
    /// 测试用:指定 projects 根目录
    pub fn with_projects_root(root: PathBuf) -> UsageWatcher {
        UsageWatcher { projects_root: Some(root), ..UsageWatcher::default() }
    }

    fn root(&self) -> PathBuf {
        self.projects_root
            .clone()
            .unwrap_or_else(|| home_dir().join(".claude").join("projects"))
    }

    /// 轮询 `directory` 下 Claude Code 会话的 transcript;`None`(前台不是 claude)时清空读数
    pub fn refresh(&mut self, directory: Option<&Path>) -> Option<ContextUsage> {
        let Some(directory) = directory else {
            self.reset(false);
            return None;
        };
        let root = self.root();
        let newest = project_directories(&root, directory)
            .iter()
            .filter_map(|d| newest_transcript(d))
            .max_by_key(|(_, modified)| *modified)
            .map(|(p, _)| p);
        let Some(file) = newest else {
            self.reset(false);
            return None;
        };
        if self.path.as_ref() != Some(&file) {
            self.reset(false);
            self.path = Some(file.clone());
        }
        self.consume_new_bytes(&file);
        self.reading()
    }

    fn consume_new_bytes(&mut self, file: &Path) {
        let Ok(mut f) = File::open(file) else { return };
        let Ok(size) = f.seek(SeekFrom::End(0)) else { return };
        // 文件比游标短:不是原来那个文件了(轮转/重写),从头来
        if size < self.offset {
            self.reset(true);
        }
        if size == self.offset || f.seek(SeekFrom::Start(self.offset)).is_err() {
            return;
        }
        let mut fresh = Vec::new();
        if f.read_to_end(&mut fresh).is_err() || fresh.is_empty() {
            return;
        }
        self.offset += fresh.len() as u64;

        let mut data = std::mem::take(&mut self.carry);
        data.extend_from_slice(&fresh);
        let mut start = 0;
        while let Some(pos) = data[start..].iter().position(|b| *b == b'\n') {
            self.consume(&data[start..start + pos]);
            start += pos + 1;
        }
        // 最后一个换行之后是 claude 还在写的记录,留到下次
        self.carry = data[start..].to_vec();
    }

    fn consume(&mut self, line: &[u8]) {
        if line.len() <= 2 {
            return;
        }
        let Ok(text) = std::str::from_utf8(line) else { return };
        // 先做子串筛选:多 MB 的 transcript 逐条 JSON 解析比其余开销加起来还大
        if !text.contains("\"usage\"") && !text.contains("\"modelId\"") {
            return;
        }
        let Ok(record) = serde_json::from_str::<Value>(text) else { return };

        let is_assistant = record.get("type").and_then(Value::as_str) == Some("assistant");
        // 子 agent 的轮次记在它自己的上下文里,不算这个会话的
        let sidechain = record.get("isSidechain").and_then(Value::as_bool) == Some(true);
        if is_assistant && !sidechain {
            if let Some(usage) = record.pointer("/message/usage") {
                let field = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
                self.tokens = Some(
                    field("input_tokens")
                        + field("cache_creation_input_tokens")
                        + field("cache_read_input_tokens")
                        + field("output_tokens"),
                );
                if let Some(m) = record.pointer("/message/model").and_then(Value::as_str) {
                    self.model = Some(m.to_string());
                }
                if let Some(s) = record.get("sessionId").and_then(Value::as_str) {
                    self.session_id = Some(s.to_string());
                }
            }
        }

        if let Some(att) = record.get("attachment") {
            if att.get("type").and_then(Value::as_str) == Some("model") {
                if let Some(id) = att.pointer("/identity/modelId").and_then(Value::as_str) {
                    self.model_id = Some(id.to_string());
                }
            }
        }
    }

    fn reading(&self) -> Option<ContextUsage> {
        let tokens = self.tokens?;
        Some(ContextUsage {
            tokens,
            limit: self.limit(tokens),
            model: self.model_id.clone().or_else(|| self.model.clone()),
            session_id: self.session_id.clone(),
            transcript: self.path.clone()?,
        })
    }

    /// 模型 id 带 `[1m]` 表示长窗口。读数超过假定窗口本身就证明是没认出来的长上下文模型——
    /// 相信 token 数而不是猜测,别把表钉死在 100%
    fn limit(&self, tokens: u64) -> u64 {
        let base = if self.model_id.as_deref().is_some_and(|m| m.contains("[1m]")) {
            LONG_LIMIT
        } else {
            STANDARD_LIMIT
        };
        if tokens > base { LONG_LIMIT } else { base }
    }

    fn reset(&mut self, keep_path: bool) {
        if !keep_path {
            self.path = None;
        }
        self.offset = 0;
        self.carry.clear();
        self.tokens = None;
        self.model = None;
        self.model_id = None;
        self.session_id = None;
    }
}

/// claude 为某个工作目录建的文件夹:路径分隔符换成 `-`(`/Users/me/app` → `-Users-me-app`)。
/// 有的版本连 `.` 也换,两种写法都给,调用方取存在的那个。Windows 上盘符的 `:` 与 `\` 同样替换。
pub fn project_directories(root: &Path, cwd: &Path) -> Vec<PathBuf> {
    let cwd = cwd.to_string_lossy();
    let dashed: String = cwd
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':') { '-' } else { c })
        .collect();
    let dotless = dashed.replace('.', "-");
    let mut names = vec![dashed.clone()];
    if dotless != dashed {
        names.push(dotless);
    }
    names.into_iter().map(|n| root.join(n)).collect()
}

/// 项目文件夹里最近写过的 `.jsonl`
fn newest_transcript(dir: &Path) -> Option<(PathBuf, SystemTime)> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|e| Some((e.path(), e.metadata().ok()?.modified().ok()?)))
        .max_by_key(|(_, t)| *t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn assistant(tokens: (u64, u64, u64, u64), sidechain: bool) -> String {
        format!(
            r#"{{"type":"assistant","isSidechain":{sidechain},"sessionId":"s1","message":{{"model":"claude-opus-5-5","usage":{{"input_tokens":{},"cache_creation_input_tokens":{},"cache_read_input_tokens":{},"output_tokens":{}}}}}}}"#,
            tokens.0, tokens.1, tokens.2, tokens.3
        )
    }

    #[test]
    fn compacts_numbers() {
        assert_eq!(compact(999), "999");
        assert_eq!(compact(65_400), "65K");
        assert_eq!(compact(1_000_000), "1M");
        assert_eq!(compact(1_500_000), "1.5M");
    }

    #[test]
    fn maps_project_directories() {
        let dirs = project_directories(Path::new("/r"), Path::new("/Users/me/my.app"));
        assert_eq!(dirs, vec![PathBuf::from("/r/-Users-me-my.app"), PathBuf::from("/r/-Users-me-my-app")]);
    }

    #[test]
    fn reads_incrementally_and_ignores_sidechains() {
        let root = tempfile::tempdir().unwrap();
        let cwd = Path::new("/work/proj");
        let dir = root.path().join("-work-proj");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("abc.jsonl");
        let mut f = File::create(&file).unwrap();
        writeln!(f, r#"{{"attachment":{{"type":"model","identity":{{"modelId":"claude-opus-5-5[1m]"}}}}}}"#).unwrap();
        writeln!(f, "{}", assistant((10, 20, 30_000, 40), false)).unwrap();
        f.flush().unwrap();

        let mut w = UsageWatcher::with_projects_root(root.path().to_path_buf());
        let u = w.refresh(Some(cwd)).unwrap();
        assert_eq!(u.tokens, 30_070);
        assert_eq!(u.limit, LONG_LIMIT);
        assert_eq!(u.model.as_deref(), Some("claude-opus-5-5[1m]"));
        assert_eq!(u.session_id.as_deref(), Some("s1"));

        // 子 agent 的记录不算;半行(还在写)先不算
        writeln!(f, "{}", assistant((1, 1, 1, 1), true)).unwrap();
        write!(f, "{}", &assistant((0, 0, 50_000, 0), false)[..20]).unwrap();
        f.flush().unwrap();
        assert_eq!(w.refresh(Some(cwd)).unwrap().tokens, 30_070);

        // 补完那一行
        writeln!(f, "{}", &assistant((0, 0, 50_000, 0), false)[20..]).unwrap();
        f.flush().unwrap();
        assert_eq!(w.refresh(Some(cwd)).unwrap().tokens, 50_000);

        // 前台不是 claude:清空
        assert!(w.refresh(None).is_none());
    }

    #[test]
    fn overflowing_standard_window_means_long_context() {
        let w = UsageWatcher::default();
        assert_eq!(w.limit(150_000), STANDARD_LIMIT);
        assert_eq!(w.limit(250_000), LONG_LIMIT);
    }
}
