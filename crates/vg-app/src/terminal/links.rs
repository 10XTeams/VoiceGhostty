//! 链接:按住 ⌘ 悬停时识别指针下的 URL,⌘+点击交给系统打开。
//! 两种来源:程序用 OSC 8 标出的超链接(`ls --hyperlink`、gh 等),以及屏幕文字里按正则匹配出的 `https://…`。

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Boundary, Column, Direction, Line, Point};
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};
use alacritty_terminal::term::Term;

/// 能当链接打开的协议。正则只认这些;OSC 8 给的 URI 也得是这些之一才打开
const SCHEMES: &[&str] = &["https://", "http://", "file://", "ftp://", "git://", "ssh://", "mailto:"];

/// 协议开头,后面一直吃到空白 / 控制字符 / 引号括号之类明显不属于 URL 的字符为止
const URL_REGEX: &str =
    r#"(https?://|file://|ftp://|git://|ssh://|mailto:)[^\x00-\x1f\x7f-\x9f<>"\s{}|^⟨⟩`\\]+"#;

/// 识别到的一个链接:占的格子范围(画下划线用)与要打开的地址
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub range: Match,
    pub uri: String,
}

pub struct LinkFinder {
    regex: RegexSearch,
}

impl Default for LinkFinder {
    fn default() -> Self {
        Self { regex: RegexSearch::new(URL_REGEX).expect("built-in url regex compiles") }
    }
}

impl LinkFinder {
    /// 指针所在格子上的链接(没有则 None)
    pub fn at<T>(&mut self, term: &Term<T>, point: Point) -> Option<Link> {
        osc8_at(term, point).or_else(|| self.regex_at(term, point))
    }

    /// 只在当前可见的屏幕范围里匹配:够用,而且每帧跑也不心疼
    fn regex_at<T>(&mut self, term: &Term<T>, point: Point) -> Option<Link> {
        let start = Point::new(Line(-(term.grid().display_offset() as i32)), Column(0));
        let end = Point::new(Line(start.line.0 + term.bottommost_line().0), term.last_column());
        let m = RegexIter::new(start, end, Direction::Right, term, &mut self.regex)
            .take_while(|m| *m.start() <= point)
            .find(|m| m.contains(&point))?;
        let text = term.bounds_to_string(*m.start(), *m.end());
        let uri = trim_trailing(&text);
        let cut = text.chars().count() - uri.chars().count();
        let end = m.end().sub(term, Boundary::Grid, cut);
        Some(Link { range: *m.start()..=end, uri: uri.to_string() })
    }
}

/// OSC 8 超链接:从指针所在格子向两边扩到同一条链接的边界
fn osc8_at<T>(term: &Term<T>, point: Point) -> Option<Link> {
    let grid = term.grid();
    let link = grid[point].hyperlink()?;
    let same = |p: Point| grid[p].hyperlink().as_ref() == Some(&link);
    let mut start = point;
    loop {
        let prev = start.sub(term, Boundary::Grid, 1);
        if prev == start || !same(prev) {
            break;
        }
        start = prev;
    }
    let mut end = point;
    loop {
        let next = end.add(term, Boundary::Grid, 1);
        if next == end || !same(next) {
            break;
        }
        end = next;
    }
    Some(Link { range: start..=end, uri: link.uri().to_string() })
}

/// 句尾标点多半不是链接的一部分:`见 https://a.b/c.` 或 `(https://a.b/c)`。
/// 右括号只在没有配对的左括号时才去掉——维基那种 `…/Foo_(bar)` 是合法链接
pub fn trim_trailing(text: &str) -> &str {
    let mut s = text;
    loop {
        let Some(last) = s.chars().last() else { return s };
        let drop = match last {
            '.' | ',' | ';' | ':' | '!' | '?' | '\'' => true,
            ')' => s.matches('(').count() < s.matches(')').count(),
            ']' => s.matches('[').count() < s.matches(']').count(),
            _ => false,
        };
        if !drop {
            return s;
        }
        s = &s[..s.len() - last.len_utf8()];
    }
}

/// 交给系统默认程序打开。协议不在白名单里的(OSC 8 可以塞任何东西)不开
pub fn open(uri: &str) {
    if !SCHEMES.iter().any(|s| uri.starts_with(s)) {
        log::warn!("refusing to open link with unknown scheme: {uri}");
        return;
    }
    let result = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(uri).spawn()
    } else if cfg!(windows) {
        std::process::Command::new("cmd").args(["/C", "start", "", uri]).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(uri).spawn()
    };
    match result {
        Ok(_) => log::info!("opened link {uri}"),
        Err(e) => log::warn!("failed to open link {uri}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_sentence_punctuation_but_keeps_balanced_brackets() {
        assert_eq!(trim_trailing("https://a.b/c."), "https://a.b/c");
        assert_eq!(trim_trailing("https://a.b/c),"), "https://a.b/c");
        assert_eq!(trim_trailing("https://a.b/Foo_(bar)"), "https://a.b/Foo_(bar)");
        assert_eq!(trim_trailing("https://a.b/c?x=1"), "https://a.b/c?x=1");
        assert_eq!(trim_trailing(""), "");
    }

    #[test]
    fn url_regex_compiles() {
        let _ = LinkFinder::default();
    }
}
