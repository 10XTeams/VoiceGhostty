//! 台账:`~/.config/voiceghostty/usage/`,按月一个 `YYYY-MM.tsv`,每轮一行,只存数字。
//! 旁边的 `cursors.json` 记每个源文件读到哪、上次成功扫描的时刻。
//!
//! 只追加不改写:同一轮(去重键相同)可能被写多次(最后一轮没结束时每次扫描都会再写),
//! 读取时保留最后一行。先追加台账、再更新游标,中途崩溃最多导致重读。
//!
//! 为什么要自己存:Claude Code 默认约 30 天后清理会话记录,只读原始文件看不了更久的历史。

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::claude::Cursor;
use super::time::month_key;
use super::{Agent, TurnRecord};

const HEADER: &str = "# v1\tat_ms\tagent\tproject\tsession\tturn\tmodel\tinput\tcache_write_5m\tcache_write_1h\tcache_read\toutput\trequests\tduration_ms\tthinking_ms\ttool_calls\tmax_tool_output\twait_ms\tfirst_context\tpeak_context\tcache_rebuild\tsubagent_type\tsubagent_id\tskills\tskill_bytes";

pub fn usage_dir() -> PathBuf {
    crate::paths::usage_dir()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Cursors {
    /// 上次成功扫描完成的时刻(Unix 毫秒,墙上时钟)
    pub last_scan_ms: Option<i64>,
    pub files: BTreeMap<String, Cursor>,
}

impl Cursors {
    pub fn load(dir: &Path) -> Cursors {
        std::fs::read_to_string(dir.join("cursors.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// 先写临时文件再改名,不会留下写了一半的 JSON
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join("cursors.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(self).map_err(std::io::Error::other)?)?;
        std::fs::rename(tmp, dir.join("cursors.json"))
    }
}

/// 追加若干行,按月份分到各自的文件
pub fn append(dir: &Path, records: &[TurnRecord]) -> std::io::Result<()> {
    if records.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    let mut by_month: BTreeMap<String, Vec<&TurnRecord>> = BTreeMap::new();
    for r in records {
        by_month.entry(month_key(r.at_ms)).or_default().push(r);
    }
    for (month, rows) in by_month {
        let path = dir.join(format!("{month}.tsv"));
        let fresh = !path.exists();
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
        let mut buf = String::new();
        if fresh {
            buf.push_str(HEADER);
            buf.push('\n');
        }
        for r in rows {
            buf.push_str(&to_line(r));
            buf.push('\n');
        }
        f.write_all(buf.as_bytes())?;
    }
    Ok(())
}

/// 读出全部台账(去重后),按开始时间排序。`since_ms` 之前月份的文件整个跳过
pub fn load(dir: &Path, since_ms: i64) -> Vec<TurnRecord> {
    let first_month = month_key(since_ms);
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    files.retain(|p| {
        p.extension().is_some_and(|x| x == "tsv")
            && p.file_stem().is_some_and(|s| s.to_string_lossy().as_ref() >= first_month.as_str())
    });
    files.sort();
    let mut index: HashMap<(Agent, String, String, u32), usize> = HashMap::new();
    let mut out: Vec<TurnRecord> = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        for line in text.lines() {
            let Some(r) = from_line(line) else { continue };
            match index.get(&r.key()) {
                Some(&i) => out[i] = r,
                None => {
                    index.insert(r.key(), out.len());
                    out.push(r);
                }
            }
        }
    }
    out.retain(|r| r.at_ms >= since_ms);
    out.sort_by_key(|r| r.at_ms);
    out
}

/// 清除用量数据(设置里的按钮)
pub fn clear(dir: &Path) -> std::io::Result<()> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

/// 字段里不允许出现制表符和换行(项目路径理论上可能有),换成空格
fn clean(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

fn to_line(r: &TurnRecord) -> String {
    let wait = r.wait_ms.map(|w| w.to_string()).unwrap_or_default();
    format!(
        "\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        r.at_ms,
        r.agent.as_str(),
        clean(&r.project),
        clean(&r.session),
        r.turn,
        clean(&r.model),
        r.input,
        r.cache_write_5m,
        r.cache_write_1h,
        r.cache_read,
        r.output,
        r.requests,
        r.duration_ms,
        r.thinking_ms,
        r.tool_calls,
        r.max_tool_output,
        wait,
        r.first_context,
        r.peak_context,
        r.cache_rebuild,
        clean(&r.subagent_type),
        clean(&r.subagent_id),
        clean(&r.skills),
        r.skill_bytes,
    )
}

fn from_line(line: &str) -> Option<TurnRecord> {
    if line.starts_with('#') || line.is_empty() {
        return None;
    }
    // 第一列留空(对齐表头的版本标记),从第二列开始是数据
    let f: Vec<&str> = line.split('\t').collect();
    if f.len() < 25 {
        return None;
    }
    let n = |i: usize| f[i].parse::<u64>().unwrap_or(0);
    Some(TurnRecord {
        at_ms: f[1].parse().ok()?,
        agent: Agent::parse(f[2])?,
        project: f[3].to_string(),
        session: f[4].to_string(),
        turn: f[5].parse().ok()?,
        model: f[6].to_string(),
        input: n(7),
        cache_write_5m: n(8),
        cache_write_1h: n(9),
        cache_read: n(10),
        output: n(11),
        requests: n(12) as u32,
        duration_ms: n(13),
        thinking_ms: n(14),
        tool_calls: n(15) as u32,
        max_tool_output: n(16),
        wait_ms: f[17].parse().ok(),
        first_context: n(18),
        peak_context: n(19),
        cache_rebuild: n(20),
        subagent_type: f[21].to_string(),
        subagent_id: f[22].to_string(),
        skills: f[23].to_string(),
        skill_bytes: n(24),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn sample(at_ms: i64, turn: u32, output: u64) -> TurnRecord {
        TurnRecord {
            at_ms,
            agent: Agent::Claude,
            project: "/w/p".into(),
            session: "s1".into(),
            turn,
            model: "claude-opus-5-5".into(),
            input: 1,
            cache_write_5m: 2,
            cache_write_1h: 3,
            cache_read: 4,
            output,
            requests: 1,
            duration_ms: 10,
            thinking_ms: 0,
            tool_calls: 0,
            max_tool_output: 0,
            wait_ms: None,
            first_context: 10,
            peak_context: 15,
            cache_rebuild: 0,
            subagent_type: String::new(),
            subagent_id: String::new(),
            skills: String::new(),
            skill_bytes: 0,
        }
    }

    #[test]
    fn round_trips_and_keeps_last_row_per_key() {
        let d = tempfile::tempdir().unwrap();
        let t = 1_795_349_722_726;
        let mut a = sample(t, 0, 5);
        a.wait_ms = Some(42);
        a.project = "/w/has\ttab".into();
        append(d.path(), &[a.clone(), sample(t + 1, 1, 5)]).unwrap();
        // 最后一轮又写了一次,数字变大
        append(d.path(), &[sample(t + 1, 1, 99)]).unwrap();
        let all = load(d.path(), 0);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].wait_ms, Some(42));
        assert_eq!(all[0].project, "/w/has tab");
        assert_eq!(all[1].output, 99);
        // 时间筛选
        assert_eq!(load(d.path(), t + 1).len(), 1);
    }

    #[test]
    fn cursors_survive_round_trip() {
        let d = tempfile::tempdir().unwrap();
        let mut c = Cursors { last_scan_ms: Some(7), ..Cursors::default() };
        c.files.insert("/a".into(), Cursor { offset: 3, turn: 2, ..Cursor::default() });
        c.save(d.path()).unwrap();
        let back = Cursors::load(d.path());
        assert_eq!(back.last_scan_ms, Some(7));
        assert_eq!(back.files["/a"].turn, 2);
    }
}
