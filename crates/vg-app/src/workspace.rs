//! 标签页 + 分屏 + 焦点。对窗格类型泛型,所以全部操作都能用假窗格单测。
//! 即时模式 UI 没有"视图被两个父级同时持有"的问题,所以在标签页之间移动窗格就是改树,不需要 Swift 版的握手。

use std::collections::HashMap;

use crate::split::{Axis, Dir, Node};

pub type PaneId = u64;

pub struct Tab {
    pub root: Node<PaneId>,
    pub active: PaneId,
    /// 手动改的标签名;空则用各窗格的目录名
    pub custom_title: Option<String>,
}

impl Tab {
    fn new(pane: PaneId) -> Tab {
        Tab { root: Node::Leaf(pane), active: pane, custom_title: None }
    }

    pub fn panes(&self) -> Vec<PaneId> {
        self.root.leaves()
    }

    pub fn is_split(&self) -> bool {
        matches!(self.root, Node::Branch(..))
    }
}

pub struct Workspace<P> {
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
    pub panes: HashMap<PaneId, P>,
    next_id: PaneId,
}

impl<P> Default for Workspace<P> {
    fn default() -> Self {
        Workspace { tabs: vec![], active_tab: 0, panes: HashMap::new(), next_id: 1 }
    }
}

impl<P> Workspace<P> {
    /// 预留下一个窗格编号(创建会话时要用它区分状态文件)
    pub fn next_pane_id(&self) -> PaneId {
        self.next_id
    }

    fn insert(&mut self, pane: P) -> PaneId {
        let id = self.next_id;
        self.next_id += 1;
        self.panes.insert(id, pane);
        id
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active_tab)
    }

    pub fn active_pane_id(&self) -> Option<PaneId> {
        self.active_tab().map(|t| t.active)
    }

    pub fn active_pane(&self) -> Option<&P> {
        self.panes.get(&self.active_pane_id()?)
    }

    pub fn active_pane_mut(&mut self) -> Option<&mut P> {
        let id = self.active_pane_id()?;
        self.panes.get_mut(&id)
    }

    /// 新标签页放在最后并切过去
    pub fn add_tab(&mut self, pane: P) -> PaneId {
        let id = self.insert(pane);
        self.tabs.push(Tab::new(id));
        self.active_tab = self.tabs.len() - 1;
        id
    }

    /// ⌘D / ⌘⇧D:把当前窗格沿 `axis` 分开,焦点给新窗格
    pub fn split_active(&mut self, pane: P, axis: Axis) -> Option<PaneId> {
        let target = self.active_pane_id()?;
        let id = self.insert(pane);
        let tab = &mut self.tabs[self.active_tab];
        tab.root = tab.root.inserting(id, target, axis);
        tab.active = id;
        Some(id)
    }

    /// ⌘W:分屏时先关当前窗格(焦点给视觉上的前一个),否则关整个标签页。返回被关掉的窗格
    pub fn close_active(&mut self) -> Vec<P> {
        let Some(tab) = self.tabs.get_mut(self.active_tab) else { return vec![] };
        let list = tab.panes();
        if list.len() > 1 {
            let idx = list.iter().position(|p| *p == tab.active).unwrap_or(0);
            let closing = list[idx];
            if let Some(pruned) = tab.root.removing(closing) {
                tab.root = pruned;
                tab.active = list[if idx > 0 { idx - 1 } else { 1 }];
            }
            return self.panes.remove(&closing).into_iter().collect();
        }
        self.close_tab(self.active_tab)
    }

    pub fn close_tab(&mut self, index: usize) -> Vec<P> {
        if index >= self.tabs.len() {
            return vec![];
        }
        let tab = self.tabs.remove(index);
        if self.active_tab > index || self.active_tab >= self.tabs.len() {
            self.active_tab = self.active_tab.saturating_sub(1);
        }
        tab.panes().into_iter().filter_map(|id| self.panes.remove(&id)).collect()
    }

    /// 去掉不满足 `keep` 的窗格(比如 shell 已退出),空了的标签页一并关掉
    pub fn retain(&mut self, mut keep: impl FnMut(&P) -> bool) -> Vec<P> {
        let gone: Vec<PaneId> = self.panes.iter().filter(|(_, p)| !keep(p)).map(|(id, _)| *id).collect();
        let mut removed = vec![];
        for id in gone {
            if let Some(ti) = self.tabs.iter().position(|t| t.root.contains(id)) {
                let tab = &mut self.tabs[ti];
                match tab.root.removing(id) {
                    Some(pruned) => {
                        tab.root = pruned;
                        if tab.active == id {
                            tab.active = tab.panes()[0];
                        }
                    }
                    None => {
                        self.tabs.remove(ti);
                        if self.active_tab > ti || self.active_tab >= self.tabs.len() {
                            self.active_tab = self.active_tab.saturating_sub(1);
                        }
                    }
                }
            }
            removed.extend(self.panes.remove(&id));
        }
        removed
    }

    // MARK: 焦点

    /// 点击某个窗格:它所在的标签页和它自己成为当前
    pub fn focus_pane(&mut self, id: PaneId) {
        if let Some(ti) = self.tabs.iter().position(|t| t.root.contains(id)) {
            self.active_tab = ti;
            self.tabs[ti].active = id;
        }
    }

    /// ⌘⌥方向键:按分屏树找相邻窗格;那一侧没有窗格就不动
    pub fn focus_dir(&mut self, dir: Dir) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            if let Some(next) = tab.root.neighbor(tab.active, dir) {
                tab.active = next;
            }
        }
    }

    pub fn select_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_tab = index;
        }
    }

    /// 下一个 / 上一个标签页,首尾循环
    pub fn cycle_tab(&mut self, delta: isize) {
        let n = self.tabs.len() as isize;
        if n > 0 {
            self.active_tab = (self.active_tab as isize + delta).rem_euclid(n) as usize;
        }
    }

    // MARK: 在标签页之间移动窗格(shell 与滚动历史跟着走)

    /// 从当前标签页摘下当前窗格;它是唯一窗格时整个标签页随之消失。返回 (窗格, 原标签页是否还在)
    fn detach_active(&mut self) -> Option<(PaneId, bool)> {
        let tab = self.tabs.get_mut(self.active_tab)?;
        let id = tab.active;
        match tab.root.removing(id) {
            Some(pruned) => {
                tab.root = pruned;
                tab.active = tab.panes()[0];
                Some((id, true))
            }
            None => {
                self.tabs.remove(self.active_tab);
                Some((id, false))
            }
        }
    }

    /// ⌘⌃1…8:把当前窗格移到第 N 个标签页,落在那边当前窗格的右侧(那边按 ⌘D 会放的位置)
    pub fn move_active_to_tab(&mut self, index: usize) {
        if index >= self.tabs.len() || index == self.active_tab {
            return;
        }
        let target_tab_was_after = index > self.active_tab;
        let Some((id, source_kept)) = self.detach_active() else { return };
        // 原标签页消失时,目标下标要跟着前移
        let index = if !source_kept && target_tab_was_after { index - 1 } else { index };
        let tab = &mut self.tabs[index];
        tab.root = tab.root.inserting(id, tab.active, Axis::Horizontal);
        tab.active = id;
        self.active_tab = index;
    }

    /// ⌘⌃← / ⌘⌃→:移到上一个 / 下一个标签页,首尾循环
    pub fn move_active_adjacent(&mut self, delta: isize) {
        let n = self.tabs.len() as isize;
        if n < 2 {
            return;
        }
        let target = (self.active_tab as isize + delta).rem_euclid(n) as usize;
        self.move_active_to_tab(target);
    }

    /// ⌘⌃T:把当前分屏移出成紧挨着的新标签页。没分屏时无事可做——它本来就是一个标签页
    pub fn move_active_to_new_tab(&mut self) {
        if !self.active_tab().is_some_and(Tab::is_split) {
            return;
        }
        let Some((id, _)) = self.detach_active() else { return };
        let at = self.active_tab + 1;
        self.tabs.insert(at, Tab::new(id));
        self.active_tab = at;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(tabs: &[&[&'static str]]) -> Workspace<&'static str> {
        let mut w = Workspace::default();
        for panes in tabs {
            w.add_tab(panes[0]);
            for p in &panes[1..] {
                w.split_active(p, Axis::Horizontal);
            }
        }
        w
    }

    fn names(w: &Workspace<&'static str>, tab: usize) -> Vec<&'static str> {
        w.tabs[tab].panes().iter().map(|id| w.panes[id]).collect()
    }

    fn active(w: &Workspace<&'static str>) -> &'static str {
        w.active_pane().copied().unwrap()
    }

    #[test]
    fn split_and_close_move_focus() {
        let mut w = ws(&[&["a", "b", "c"]]);
        assert_eq!(names(&w, 0), ["a", "b", "c"]);
        assert_eq!(active(&w), "c");
        assert_eq!(w.close_active(), ["c"]);
        assert_eq!(active(&w), "b");
        w.focus_dir(Dir::Left);
        assert_eq!(active(&w), "a");
        assert_eq!(w.close_active(), ["a"]);
        assert_eq!(active(&w), "b"); // 关掉最左边的:焦点给右边那个
        assert_eq!(w.close_active(), ["b"]);
        assert!(w.tabs.is_empty());
    }

    #[test]
    fn move_between_tabs() {
        let mut w = ws(&[&["a", "b"], &["c"]]);
        w.select_tab(0);
        w.move_active_to_tab(1);
        assert_eq!(names(&w, 0), ["a"]);
        assert_eq!(names(&w, 1), ["c", "b"]);
        assert_eq!((w.active_tab, active(&w)), (1, "b"));

        // 唯一窗格被移走:原标签页消失,下标修正
        w.select_tab(0);
        w.move_active_to_tab(1);
        assert_eq!(w.tabs.len(), 1);
        assert_eq!(names(&w, 0), ["c", "b", "a"]); // 落在目标标签页当前窗格(b)的右侧
        assert_eq!(active(&w), "a");
    }

    #[test]
    fn move_to_new_tab_and_adjacent() {
        let mut w = ws(&[&["a", "b"], &["c"]]);
        w.select_tab(0);
        w.move_active_to_new_tab();
        assert_eq!(w.tabs.len(), 3);
        assert_eq!((names(&w, 0), names(&w, 1)), (vec!["a"], vec!["b"]));
        assert_eq!(w.active_tab, 1);
        w.move_active_to_new_tab(); // 没分屏:不动
        assert_eq!(w.tabs.len(), 3);
        w.move_active_adjacent(-1); // b 回到第一个标签页
        assert_eq!(names(&w, 0), ["a", "b"]);
        assert_eq!(w.tabs.len(), 2);
    }

    #[test]
    fn retain_prunes_and_drops_empty_tabs() {
        let mut w = ws(&[&["a", "b"], &["c"]]);
        let removed = w.retain(|p| *p != "b" && *p != "c");
        assert_eq!(removed.len(), 2);
        assert_eq!(w.tabs.len(), 1);
        assert_eq!(names(&w, 0), ["a"]);
        assert_eq!(active(&w), "a");
        assert_eq!(w.active_tab, 0);
    }

    #[test]
    fn cycle_and_click_focus() {
        let mut w = ws(&[&["a"], &["b", "c"], &["d"]]);
        w.cycle_tab(1);
        assert_eq!(w.active_tab, 0);
        w.cycle_tab(-1);
        assert_eq!(w.active_tab, 2);
        let b = *w.panes.iter().find(|(_, p)| **p == "b").unwrap().0;
        w.focus_pane(b);
        assert_eq!((w.active_tab, active(&w)), (1, "b"));
    }
}
