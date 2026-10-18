//! ⌘F:在滚动历史里搜索(回车下一个,⇧回车上一个)。按字面文本搜;全小写时忽略大小写。
//! 命中项用选区标出来并滚动到可见,所以 ⌘C 能直接复制它。

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Boundary, Direction, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::search::{Match, RegexSearch};

use super::session::TerminalSession;

#[derive(Default)]
pub struct SearchState {
    pub query: String,
    last: Option<Match>,
    compiled_for: String,
    regex: Option<RegexSearch>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SearchResult {
    Found,
    NotFound,
    Empty,
}

impl SearchState {
    /// 换查询词时从头(底部)开始找
    pub fn reset(&mut self) {
        self.last = None;
    }

    /// `forward`:向下(回车)还是向上(⇧回车)。首次搜索从最新的输出往上找——你要找的多半是刚才看到的
    pub fn step(&mut self, session: &TerminalSession, forward: bool) -> SearchResult {
        if self.query.is_empty() {
            return SearchResult::Empty;
        }
        if self.compiled_for != self.query || self.regex.is_none() {
            self.regex = RegexSearch::new(&escape(&self.query)).ok();
            self.compiled_for = self.query.clone();
            self.last = None;
        }
        let Some(regex) = self.regex.as_mut() else { return SearchResult::NotFound };

        let mut term = session.term.lock();
        let (direction, origin, side) = match (&self.last, forward) {
            (Some(m), true) => (Direction::Right, *m.end(), Side::Right),
            (Some(m), false) => (Direction::Left, *m.start(), Side::Left),
            // 第一次:从屏幕最底部向上
            (None, _) => (
                Direction::Left,
                Point::new(term.grid().bottommost_line(), term.grid().last_column()),
                Side::Right,
            ),
        };
        let found = term.search_next(regex, origin, direction, side, None);
        // 从当前命中项出发时,search_next 可能又返回它自己:再走一步
        let found = match (found, &self.last) {
            (Some(m), Some(last)) if m == *last => {
                let next_origin = if forward { *m.end() } else { *m.start() };
                let next_origin = if forward {
                    next_origin.add(&*term, Boundary::Grid, 1)
                } else {
                    next_origin.sub(&*term, Boundary::Grid, 1)
                };
                term.search_next(regex, next_origin, direction, side, None)
            }
            (m, _) => m,
        };
        let Some(m) = found else {
            return SearchResult::NotFound;
        };
        let mut sel = Selection::new(SelectionType::Simple, *m.start(), Side::Left);
        sel.update(*m.end(), Side::Right);
        term.selection = Some(sel);
        term.scroll_to_point(*m.start());
        self.last = Some(m);
        SearchResult::Found
    }
}

/// 把用户输入当字面文本:转义正则元字符
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_metacharacters() {
        assert_eq!(escape("a.b*(c)"), r"a\.b\*\(c\)");
        assert_eq!(escape("中文"), "中文");
    }
}
