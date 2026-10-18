//! 把窗格的记录追加到 `~/.config/voiceghostty/history/<目录>-<时间>.log`。
//!
//! 数据源是终端网格而不是原始 PTY 流:行进入网格时转义序列都已生效,所以不断重绘的 TUI(claude 的转圈、
//! 重新渲染的消息块)落下来就是你最后看到的那一行,而不是几十个画了一半的版本。只写已经滚出屏幕顶部的行
//! (屏幕上的行还可能被重画);`finish()` 在关闭时把屏幕上剩下的也写进去。
//!
//! 增量写入(而不是最后整体倒出)意味着即使 scrollback 把最早的行从内存里裁掉,文件里仍有完整会话。
//!
//! 定位:alacritty 不提供"累计滚出多少行"。历史没满时,历史行数的增量就是新行数(精确);历史满了开始
//! 轮转,或者行数反而变少(`clear`、改窗口宽度导致重排)时,在历史里找上次写过的最后几行作为锚点。

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Term, TermMode};

/// 锚点取历史末尾的几行(越长越不容易被重复行误配)
const ANCHOR_LINES: usize = 8;

pub struct SessionLogger {
    dir: PathBuf,
    started_at: String,
    /// 上次写完时的历史行数
    history_seen: usize,
    anchor: Vec<String>,
    file: Option<File>,
    open_failed: bool,
    /// 文件第一次创建时用来命名(窗格都从家目录开始,等到真有输出时多半已经 cd 进项目了)
    pub current_directory: Option<PathBuf>,
    pub path: Option<PathBuf>,
}

impl SessionLogger {
    pub fn new(dir: PathBuf) -> SessionLogger {
        SessionLogger {
            dir,
            started_at: chrono::Local::now().format("%Y%m%d-%H%M%S").to_string(),
            history_seen: 0,
            anchor: Vec::new(),
            file: None,
            open_failed: false,
            current_directory: None,
            path: None,
        }
    }

    /// 写入上次以来滚出屏幕的所有行
    pub fn flush<T>(&mut self, term: &Term<T>, scrollback_limit: usize) {
        self.append(term, scrollback_limit, false);
    }

    /// 最后一次写入:屏幕上的行也写,然后关文件
    pub fn finish<T>(&mut self, term: &Term<T>, scrollback_limit: usize) {
        self.append(term, scrollback_limit, true);
        self.file = None;
    }

    fn append<T>(&mut self, term: &Term<T>, scrollback_limit: usize, including_screen: bool) {
        // 备用屏(vim、less)没有历史,编号也是自己的——记它只会往记录里掺垃圾
        if term.mode().contains(TermMode::ALT_SCREEN) {
            return;
        }
        let grid = term.grid();
        let history = grid.history_size();
        let history_lines: Vec<String> =
            (0..history).map(|j| line_text(term, Line(j as i32 - history as i32))).collect();

        let mut text = String::new();
        let start = if history > self.history_seen && history < scrollback_limit {
            // 没满且在增长:精确
            self.history_seen
        } else if history == self.history_seen && history < scrollback_limit {
            history
        } else {
            match find_anchor(&history_lines, &self.anchor) {
                Some(after) => after,
                None if history >= scrollback_limit && !self.anchor.is_empty() => {
                    text.push_str("\n[… lines dropped: scrollback limit reached …]\n");
                    0
                }
                // 清屏/重排后找不到锚点:从头(历史为空时就是什么都不补)
                None => 0,
            }
        };

        let mut written: Vec<&String> = history_lines[start.min(history)..].iter().collect();
        let screen: Vec<String>;
        if including_screen {
            let mut rows: Vec<String> =
                (0..grid.screen_lines()).map(|i| line_text(term, Line(i as i32))).collect();
            while rows.last().is_some_and(|l| l.is_empty()) {
                rows.pop();
            }
            screen = rows;
            written.extend(screen.iter());
        }
        for line in &written {
            text.push_str(line);
            text.push('\n');
        }
        // 此刻历史里的行都已写过,锚点就取历史的末尾
        let tail = history_lines.len().saturating_sub(ANCHOR_LINES);
        self.anchor = history_lines[tail..].to_vec();
        self.history_seen = history;
        self.write(&text);
    }

    fn write(&mut self, text: &str) {
        if text.is_empty() || self.open_failed {
            return;
        }
        if self.file.is_none() {
            self.open();
        }
        if let Some(f) = &mut self.file {
            let _ = f.write_all(text.as_bytes());
        }
    }

    fn open(&mut self) {
        let path = self.dir.join(format!("{}-{}.log", folder_name(self.current_directory.as_deref()), self.started_at));
        let file = std::fs::create_dir_all(&self.dir)
            .and_then(|_| OpenOptions::new().create(true).append(true).open(&path));
        match file {
            Ok(f) => {
                self.file = Some(f);
                self.path = Some(path);
            }
            // 家目录只读、没权限……静默放弃,别每次 flush 都报
            Err(_) => self.open_failed = true,
        }
    }
}

/// 在历史行里找锚点(上次历史末尾的几行),返回锚点之后第一行的下标。
/// 先找完整锚点(取最靠下的匹配);找不到时,锚点的前半截可能已经被轮转出去,
/// 那剩下的后半截一定正好在历史最顶上。
fn find_anchor(lines: &[String], anchor: &[String]) -> Option<usize> {
    if anchor.is_empty() {
        return None;
    }
    if anchor.len() <= lines.len() {
        if let Some(i) = (0..=lines.len() - anchor.len()).rev().find(|&i| lines[i..i + anchor.len()] == *anchor) {
            return Some(i + anchor.len());
        }
    }
    (1..anchor.len())
        .rev()
        .filter(|&k| k <= lines.len())
        .find(|&k| lines[..k] == anchor[anchor.len() - k..])
}

/// 一行网格的文字:宽字符的占位格丢掉,行尾空白去掉
pub fn line_text<T>(term: &Term<T>, line: Line) -> String {
    let grid = term.grid();
    let row = &grid[line];
    let mut s = String::new();
    for col in 0..grid.columns() {
        let cell = &row[Column(col)];
        if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            continue;
        }
        s.push(cell.c);
        if let Some(extra) = cell.zerowidth() {
            s.extend(extra);
        }
    }
    s.truncate(s.trim_end().len());
    s
}

fn folder_name(dir: Option<&Path>) -> String {
    let name = dir
        .and_then(|d| d.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "home".into());
    let name: String = name.chars().map(|c| if matches!(c, '/' | '\\' | ':') { '-' } else { c }).collect();
    if name.is_empty() { "root".into() } else { name }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::vte::ansi::Processor;

    fn term(rows: usize, scrollback: usize) -> Term<VoidListener> {
        let config = Config { scrolling_history: scrollback, ..Config::default() };
        Term::new(config, &TermSize::new(20, rows), VoidListener)
    }

    fn feed(term: &mut Term<VoidListener>, p: &mut Processor, s: &str) {
        p.advance(term, s.replace('\n', "\r\n").as_bytes());
    }

    fn read(logger: &SessionLogger) -> String {
        std::fs::read_to_string(logger.path.as_ref().unwrap()).unwrap()
    }

    #[test]
    fn writes_scrolled_lines_once_then_screen_on_finish() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = term(3, 100);
        let mut p = Processor::new();
        let mut log = SessionLogger::new(dir.path().to_path_buf());
        log.current_directory = Some("/work/proj".into());

        feed(&mut t, &mut p, "a\nb\nc\nd\ne");
        log.flush(&t, 100);
        assert_eq!(read(&log), "a\nb\n");
        assert!(log.path.as_ref().unwrap().file_name().unwrap().to_string_lossy().starts_with("proj-"));

        log.flush(&t, 100); // 没有新行
        feed(&mut t, &mut p, "\nf");
        log.flush(&t, 100);
        assert_eq!(read(&log), "a\nb\nc\n");

        log.finish(&t, 100);
        assert_eq!(read(&log), "a\nb\nc\nd\ne\nf\n");
    }

    #[test]
    fn follows_rotation_with_anchor_and_marks_drops() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = term(2, 3);
        let mut p = Processor::new();
        let mut log = SessionLogger::new(dir.path().to_path_buf());

        feed(&mut t, &mut p, "l1\nl2\nl3\nl4\nl5"); // 历史 = l1 l2 l3(满)
        log.flush(&t, 3);
        assert_eq!(read(&log), "l1\nl2\nl3\n");
        feed(&mut t, &mut p, "\nl6"); // 历史轮转为 l2 l3 l4
        log.flush(&t, 3);
        assert_eq!(read(&log), "l1\nl2\nl3\nl4\n");
        feed(&mut t, &mut p, "\nx1\nx2\nx3\nx4\nx5"); // 锚点被挤出历史
        log.flush(&t, 3);
        assert!(read(&log).contains("lines dropped"), "{}", read(&log));
    }

    #[test]
    fn wide_chars_have_no_placeholder_spaces() {
        let mut t = term(3, 10);
        let mut p = Processor::new();
        feed(&mut t, &mut p, "中文ok");
        assert_eq!(line_text(&t, Line(0)), "中文ok");
    }

    #[test]
    fn alt_screen_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = term(2, 10);
        let mut p = Processor::new();
        let mut log = SessionLogger::new(dir.path().to_path_buf());
        feed(&mut t, &mut p, "\x1b[?1049hvim\nstuff\nmore");
        log.finish(&t, 10);
        assert!(log.path.is_none());
    }
}
