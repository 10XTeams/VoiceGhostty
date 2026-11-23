//! 扫描:台账唯一的写入路径。找出变过的源文件,各自从游标接着读,追加台账,再更新游标。
//!
//! 补扫范围用游标定,不用时间范围定:会话可能跨过扫描时刻、系统时间可能被改、旧会话可能被恢复后继续写,
//! 按时间切会重复或漏掉边界上的轮次。文件大小和修改时间都没变就跳过,不用读。

use std::path::Path;

use super::claude::{self, SourceFile};
use super::ledger::{self, Cursors};
use super::time::now_ms;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScanReport {
    pub files_total: usize,
    pub files_read: usize,
    pub rows_written: usize,
    /// 记录里见到的最高 Claude Code 版本,比验证过的新时面板注明"未验证"
    pub newest_version: Option<String>,
}

/// 扫描 `claude_root` 下的会话记录,写进 `ledger_dir`。`progress(已处理, 总数)` 在每个文件之后回调
pub fn scan(claude_root: &Path, ledger_dir: &Path, mut progress: impl FnMut(usize, usize)) -> std::io::Result<ScanReport> {
    let mut cursors = Cursors::load(ledger_dir);
    let files = claude::list_files(claude_root);
    let mut report = ScanReport { files_total: files.len(), ..ScanReport::default() };
    let mut pending = Vec::new();
    let mut since_save = 0usize;

    for (i, file) in files.iter().enumerate() {
        let key = file.path.to_string_lossy().into_owned();
        let cursor = cursors.files.get(&key).cloned().unwrap_or_default();
        if unchanged(file, &cursor) {
            progress(i + 1, files.len());
            continue;
        }
        match claude::parse_file(file, &cursor) {
            Ok((records, next)) => {
                report.files_read += 1;
                report.rows_written += records.len();
                if !next.version.is_empty() && report.newest_version.as_deref().is_none_or(|v| newer(&next.version, v)) {
                    report.newest_version = Some(next.version.clone());
                }
                pending.extend(records);
                cursors.files.insert(key, next);
                since_save += 1;
            }
            Err(e) => log::warn!("insights: {}: {e}", file.path.display()),
        }
        // 首次全量扫描可能很久:每几十个文件落一次盘,中途退出也不用从头来
        if since_save >= 40 || pending.len() >= 5_000 {
            ledger::append(ledger_dir, &pending)?;
            pending.clear();
            cursors.save(ledger_dir)?;
            since_save = 0;
        }
        progress(i + 1, files.len());
    }
    ledger::append(ledger_dir, &pending)?;
    // 已经不存在的文件(被 Claude Code 清理掉了)的游标不再需要;台账里的行保留
    let alive: std::collections::HashSet<String> = files.iter().map(|f| f.path.to_string_lossy().into_owned()).collect();
    cursors.files.retain(|k, _| alive.contains(k));
    cursors.last_scan_ms = Some(now_ms());
    cursors.save(ledger_dir)?;
    Ok(report)
}

fn unchanged(file: &SourceFile, cursor: &claude::Cursor) -> bool {
    let Ok(meta) = std::fs::metadata(&file.path) else { return true };
    let mtime = meta.modified().map(super::time::system_time_ms).unwrap_or(0);
    cursor.size == meta.len() && cursor.mtime_ms == mtime && cursor.size > 0
}

/// "2.1.330" 与 "2.1.99" 按数字比较
pub fn newer(a: &str, b: &str) -> bool {
    let parts = |s: &str| s.split('.').map(|x| x.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    parts(a) > parts(b)
}

/// 距上次成功扫描是否已超过一个周期(墙上时钟;休眠期间监视线程不跑,不能数轮次)
pub fn due(ledger_dir: &Path, period_ms: i64) -> bool {
    Cursors::load(ledger_dir).last_scan_ms.is_none_or(|t| now_ms() - t >= period_ms || now_ms() < t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn rescans_do_not_double_count() {
        let src = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let proj = src.path().join("-w-p");
        std::fs::create_dir_all(&proj).unwrap();
        let path = proj.join("s1.jsonl");
        let line = |ts: &str, kind: &str, id: &str| match kind {
            "p" => format!(r#"{{"type":"user","cwd":"/w/p","timestamp":"{ts}","message":{{"content":"hi"}}}}"#),
            _ => format!(r#"{{"type":"assistant","timestamp":"{ts}","message":{{"model":"claude-opus-5-5","id":"{id}","usage":{{"input_tokens":1,"output_tokens":10}}}}}}"#),
        };
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "{}", line("2026-11-22T10:00:00Z", "p", "")).unwrap();
        writeln!(f, "{}", line("2026-11-22T10:00:01Z", "a", "m1")).unwrap();
        f.flush().unwrap();

        let r = scan(src.path(), out.path(), |_, _| {}).unwrap();
        assert_eq!((r.files_total, r.files_read, r.rows_written), (1, 1, 1));
        // 没变:跳过
        let r = scan(src.path(), out.path(), |_, _| {}).unwrap();
        assert_eq!(r.files_read, 0);
        assert!(!due(out.path(), 60_000));

        // 同一轮又来一次请求,再开一轮
        writeln!(f, "{}", line("2026-11-22T10:00:02Z", "a", "m2")).unwrap();
        writeln!(f, "{}", line("2026-11-22T10:01:00Z", "p", "")).unwrap();
        writeln!(f, "{}", line("2026-11-22T10:01:01Z", "a", "m3")).unwrap();
        f.flush().unwrap();
        scan(src.path(), out.path(), |_, _| {}).unwrap();

        let all = ledger::load(out.path(), 0);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].output, 20);
        assert_eq!(all[1].output, 10);
    }

    #[test]
    fn compares_versions_numerically() {
        assert!(newer("2.1.330", "2.1.99"));
        assert!(!newer("2.1.99", "2.1.330"));
    }
}
