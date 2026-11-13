//! 标签页的分屏布局:一个终端(叶子),或一行/一列子节点(分支)。分支可嵌套,所以能拼出任意排布。
//! 纯数据 + 纯函数,规则与 Swift 版 `SplitNode` 一致。
//!
//! 分支给每个孩子一个**权重**,尺寸按权重比例分配;权重全相等就是等分。拖动分隔线只改相邻两个孩子的权重
//! (两者之和不变,其余兄弟不动)。

use egui::{pos2, Rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// 左右排列(⌘D)
    Horizontal,
    /// 上下排列(⌘⇧D)
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

impl Dir {
    fn axis(self) -> Axis {
        match self {
            Dir::Left | Dir::Right => Axis::Horizontal,
            Dir::Up | Dir::Down => Axis::Vertical,
        }
    }

    fn forward(self) -> bool {
        matches!(self, Dir::Right | Dir::Down)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node<Id> {
    Leaf(Id),
    /// 方向、孩子、各孩子的权重(与孩子一一对应,只看比例)
    Branch(Axis, Vec<Node<Id>>, Vec<f32>),
}

/// 两个相邻孩子之间的分隔线,拖它就调整这两个孩子的比例
#[derive(Debug, Clone, PartialEq)]
pub struct Divider {
    /// 从根到该分支每层走第几个孩子
    pub path: Vec<usize>,
    /// 分隔线在第 `index` 与第 `index + 1` 个孩子之间
    pub index: usize,
    pub axis: Axis,
    /// 间隙本身(1 pt 宽),鼠标判定区由调用方外扩
    pub rect: Rect,
    /// 这两个孩子合起来沿 `axis` 的起止坐标(含中间的间隙)
    pub span: (f32, f32),
}

/// 每个窗格沿拖动方向至少保留这么多点,拖不到看不见
pub const MIN_PANE: f32 = 60.0;

/// 沿 `axis` 把 `rect` 按权重切成若干格,格间留 `gap`。最后一格贴齐末端,避免浮点误差留缝
fn slots(rect: Rect, axis: Axis, weights: &[f32], gap: f32) -> Vec<Rect> {
    let n = weights.len();
    let total: f32 = weights.iter().sum::<f32>().max(f32::EPSILON);
    let (start, end) = match axis {
        Axis::Horizontal => (rect.min.x, rect.max.x),
        Axis::Vertical => (rect.min.y, rect.max.y),
    };
    let avail = (end - start - gap * (n as f32 - 1.0)).max(0.0);
    let mut at = start;
    weights
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let to = if i + 1 == n { end } else { at + avail * w / total };
            let r = match axis {
                Axis::Horizontal => Rect::from_min_max(pos2(at, rect.min.y), pos2(to, rect.max.y)),
                Axis::Vertical => Rect::from_min_max(pos2(rect.min.x, at), pos2(rect.max.x, to)),
            };
            at = to + gap;
            r
        })
        .collect()
}

impl<Id: Copy + PartialEq> Node<Id> {
    /// 等分的分支
    pub fn branch(axis: Axis, children: Vec<Node<Id>>) -> Node<Id> {
        let weights = vec![1.0; children.len()];
        Node::Branch(axis, children, weights)
    }

    /// 所有终端,按视觉顺序(左→右 / 上→下)
    pub fn leaves(&self) -> Vec<Id> {
        match self {
            Node::Leaf(id) => vec![*id],
            Node::Branch(_, children, _) => children.iter().flat_map(Node::leaves).collect(),
        }
    }

    pub fn contains(&self, id: Id) -> bool {
        self.leaves().contains(&id)
    }

    /// 每个窗格在 `rect` 里的位置,兄弟之间留 `gap`(间隙露出底色,就是分隔线)。顺序与 `leaves` 一致
    pub fn frames(&self, rect: Rect, gap: f32) -> Vec<(Id, Rect)> {
        match self {
            Node::Leaf(id) => vec![(*id, rect)],
            Node::Branch(axis, children, weights) => slots(rect, *axis, weights, gap)
                .into_iter()
                .zip(children)
                .flat_map(|(slot, child)| child.frames(slot, gap))
                .collect(),
        }
    }

    /// 所有分隔线(与 `frames` 用同一套几何)
    pub fn dividers(&self, rect: Rect, gap: f32) -> Vec<Divider> {
        let mut out = vec![];
        self.collect_dividers(rect, gap, &mut vec![], &mut out);
        out
    }

    fn collect_dividers(&self, rect: Rect, gap: f32, path: &mut Vec<usize>, out: &mut Vec<Divider>) {
        let Node::Branch(axis, children, weights) = self else { return };
        let slots = slots(rect, *axis, weights, gap);
        for i in 0..slots.len().saturating_sub(1) {
            let (a, b) = (slots[i], slots[i + 1]);
            let (rect, span) = match axis {
                Axis::Horizontal => {
                    (Rect::from_min_max(pos2(a.max.x, rect.min.y), pos2(b.min.x, rect.max.y)), (a.min.x, b.max.x))
                }
                Axis::Vertical => {
                    (Rect::from_min_max(pos2(rect.min.x, a.max.y), pos2(rect.max.x, b.min.y)), (a.min.y, b.max.y))
                }
            };
            out.push(Divider { path: path.clone(), index: i, axis: *axis, rect, span });
        }
        for (i, (child, slot)) in children.iter().zip(slots).enumerate() {
            path.push(i);
            child.collect_dividers(slot, gap, path, out);
            path.pop();
        }
    }

    fn weights_at_mut(&mut self, path: &[usize]) -> Option<&mut Vec<f32>> {
        let Node::Branch(_, children, weights) = self else { return None };
        match path.split_first() {
            None => Some(weights),
            Some((i, rest)) => children.get_mut(*i)?.weights_at_mut(rest),
        }
    }

    /// 拖动分隔线:把它移到坐标 `pos`(沿分隔线的方向)。只改两侧两个孩子的权重,两者之和不变;
    /// 两边都至少留 `MIN_PANE`,空间本来就不够时不动
    pub fn drag_divider(&mut self, d: &Divider, pos: f32, gap: f32) {
        let (start, end) = d.span;
        let usable = end - start - gap;
        if usable < 2.0 * MIN_PANE {
            return;
        }
        let first = (pos - gap / 2.0 - start).clamp(MIN_PANE, usable - MIN_PANE);
        let Some(w) = self.weights_at_mut(&d.path) else { return };
        if d.index + 1 >= w.len() {
            return;
        }
        let pair = w[d.index] + w[d.index + 1];
        w[d.index] = pair * first / usable;
        w[d.index + 1] = pair - w[d.index];
    }

    /// 把 `path` 处这个分支的孩子恢复等分(双击分隔线)
    pub fn equalize_at(&mut self, path: &[usize]) {
        if let Some(w) = self.weights_at_mut(path) {
            w.iter_mut().for_each(|x| *x = 1.0);
        }
    }

    /// 整棵树恢复等分
    pub fn equalize(&mut self) {
        if let Node::Branch(_, children, weights) = self {
            weights.iter_mut().for_each(|x| *x = 1.0);
            children.iter_mut().for_each(Node::equalize);
        }
    }

    /// 在 `target` 旁边插入 `new`。沿窗格所在的同一方向再分,是加一个**兄弟**——连按三次 ⌘D 得到三等分,
    /// 而不是 ½ + ¼ + ¼ 的歪嵌套;垂直方向分则把叶子换成两个孩子的分支,网格就是这样形成的。
    /// 尺寸:这一排还是等分的,就继续等分;拖过了,新窗格从 `target` 的那一份里对半分走,别的兄弟不受影响。
    /// 找不到 `target` 时原样返回。
    pub fn inserting(&self, new: Id, target: Id, axis: Axis) -> Node<Id> {
        match self {
            Node::Leaf(id) if *id == target => Node::branch(axis, vec![Node::Leaf(*id), Node::Leaf(new)]),
            Node::Leaf(_) => self.clone(),
            Node::Branch(branch_axis, children, weights) => {
                if *branch_axis == axis {
                    if let Some(i) = children.iter().position(|c| *c == Node::Leaf(target)) {
                        let mut siblings = children.clone();
                        siblings.insert(i + 1, Node::Leaf(new));
                        let mut w = weights.clone();
                        if w.iter().all(|x| *x == w[0]) {
                            w.insert(i + 1, w[0]);
                        } else {
                            w[i] /= 2.0;
                            w.insert(i + 1, w[i]);
                        }
                        return Node::Branch(*branch_axis, siblings, w);
                    }
                }
                Node::Branch(
                    *branch_axis,
                    children.iter().map(|c| c.inserting(new, target, axis)).collect(),
                    weights.clone(),
                )
            }
        }
    }

    /// 交换两个窗格的位置(布局不变,只是谁在哪个格子里换了)
    pub fn swapping(&self, a: Id, b: Id) -> Node<Id> {
        match self {
            Node::Leaf(id) if *id == a => Node::Leaf(b),
            Node::Leaf(id) if *id == b => Node::Leaf(a),
            Node::Leaf(_) => self.clone(),
            Node::Branch(axis, children, weights) => {
                Node::Branch(*axis, children.iter().map(|c| c.swapping(a, b)).collect(), weights.clone())
            }
        }
    }

    /// 移除一个终端。只剩一个孩子的分支收缩成那个孩子,关窗格不会留下空架子。子树空了返回 None。
    /// 被关掉的那一份还给紧挨着的前一个兄弟(它是第一个时给后一个),其余兄弟大小不变
    pub fn removing(&self, target: Id) -> Option<Node<Id>> {
        match self {
            Node::Leaf(id) => (*id != target).then(|| self.clone()),
            Node::Branch(axis, children, weights) => {
                let mut kept = vec![];
                let mut w: Vec<f32> = vec![];
                let mut orphan = 0.0;
                for (c, wt) in children.iter().zip(weights) {
                    match c.removing(target) {
                        Some(c) => {
                            kept.push(c);
                            w.push(*wt + orphan);
                            orphan = 0.0;
                        }
                        None => match w.last_mut() {
                            Some(prev) => *prev += *wt,
                            None => orphan = *wt,
                        },
                    }
                }
                match kept.len() {
                    0 => None,
                    1 => kept.pop(),
                    _ => Some(Node::Branch(*axis, kept, w)),
                }
            }
        }
    }

    /// `target` 在 `dir` 方向上相邻的窗格:向上找最近的沿该方向排列、且那一侧还有兄弟的祖先,
    /// 跨到兄弟,再下到它朝向来处的边缘
    pub fn neighbor(&self, target: Id, dir: Dir) -> Option<Id> {
        let path = self.chain(target)?;
        for (branch, index) in path.into_iter().rev() {
            let Node::Branch(axis, children, _) = branch else { continue };
            if *axis != dir.axis() {
                continue;
            }
            let next = if dir.forward() { index.checked_add(1) } else { index.checked_sub(1) };
            if let Some(sibling) = next.and_then(|i| children.get(i)) {
                return Some(sibling.edge_leaf(dir));
            }
        }
        None
    }

    /// 从这里到 `target` 经过的分支,以及每层走的是第几个孩子
    fn chain(&self, target: Id) -> Option<Vec<(&Node<Id>, usize)>> {
        match self {
            Node::Leaf(id) => (*id == target).then(Vec::new),
            Node::Branch(_, children, _) => children.iter().enumerate().find_map(|(i, child)| {
                let mut rest = child.chain(target)?;
                rest.insert(0, (self, i));
                Some(rest)
            }),
        }
    }

    /// 进入相邻子树时落在离来处最近的窗格:向左移动从右侧进入,以此类推
    fn edge_leaf(&self, dir: Dir) -> Id {
        match self {
            Node::Leaf(id) => *id,
            Node::Branch(axis, children, _) => {
                let child = if *axis == dir.axis() && !dir.forward() {
                    children.last()
                } else {
                    children.first()
                };
                child.expect("branch has children").edge_leaf(dir)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    #[test]
    fn same_axis_splits_add_siblings() {
        let t = Node::Leaf(1).inserting(2, 1, Axis::Horizontal).inserting(3, 2, Axis::Horizontal);
        assert_eq!(t, Node::branch(Axis::Horizontal, vec![Node::Leaf(1), Node::Leaf(2), Node::Leaf(3)]));
        let frames = t.frames(Rect::from_min_size(pos2(0.0, 0.0), vec2(302.0, 100.0)), 1.0);
        assert_eq!(frames.len(), 3);
        assert!((frames[1].1.min.x - 101.0).abs() < 1e-3 && (frames[1].1.width() - 100.0).abs() < 1e-3);
    }

    #[test]
    fn perpendicular_split_nests() {
        let t = Node::Leaf(1).inserting(2, 1, Axis::Horizontal).inserting(3, 2, Axis::Vertical);
        assert_eq!(
            t,
            Node::branch(
                Axis::Horizontal,
                vec![Node::Leaf(1), Node::branch(Axis::Vertical, vec![Node::Leaf(2), Node::Leaf(3)])]
            )
        );
        assert_eq!(t.leaves(), vec![1, 2, 3]);
    }

    #[test]
    fn removing_collapses_single_child_branches() {
        let t = Node::Leaf(1).inserting(2, 1, Axis::Horizontal).inserting(3, 2, Axis::Vertical);
        assert_eq!(
            t.removing(3),
            Some(Node::branch(Axis::Horizontal, vec![Node::Leaf(1), Node::Leaf(2)]))
        );
        assert_eq!(t.removing(1).unwrap().removing(2), Some(Node::Leaf(3)));
        assert_eq!(Node::Leaf(1).removing(1), None);
    }

    #[test]
    fn neighbors_across_nesting() {
        // [1 | [2 / 3]]
        let t = Node::Leaf(1).inserting(2, 1, Axis::Horizontal).inserting(3, 2, Axis::Vertical);
        assert_eq!(t.neighbor(1, Dir::Right), Some(2));
        assert_eq!(t.neighbor(3, Dir::Left), Some(1));
        assert_eq!(t.neighbor(2, Dir::Down), Some(3));
        assert_eq!(t.neighbor(3, Dir::Up), Some(2));
        assert_eq!(t.neighbor(1, Dir::Left), None);
        assert_eq!(t.neighbor(1, Dir::Up), None);
        // [[1 / 2] | 3]:从 3 向左进入左列,落在第一个
        let t = Node::Leaf(1).inserting(3, 1, Axis::Horizontal).inserting(2, 1, Axis::Vertical);
        assert_eq!(t.neighbor(3, Dir::Left), Some(1));
        // [1 | 2 | 3] 从 1 向右到 2,不跳到 3
        let t = Node::Leaf(1).inserting(2, 1, Axis::Horizontal).inserting(3, 2, Axis::Horizontal);
        assert_eq!(t.neighbor(1, Dir::Right), Some(2));
        assert_eq!(t.neighbor(3, Dir::Left), Some(2));
    }

    fn screen(w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h))
    }

    #[test]
    fn dragging_a_divider_moves_only_its_two_neighbours() {
        // [1 | 2 | 3],301 宽、间隙 1:各 99.67
        let mut t = Node::Leaf(1).inserting(2, 1, Axis::Horizontal).inserting(3, 2, Axis::Horizontal);
        let r = screen(301.0, 100.0);
        let d = t.dividers(r, 1.0);
        assert_eq!(d.len(), 2);
        assert_eq!((d[0].index, d[1].index), (0, 1));
        let width3 = t.frames(r, 1.0)[2].1.width();
        // 把 1|2 之间的线拖到 x = 120
        t.drag_divider(&d[0], 120.0, 1.0);
        let f = t.frames(r, 1.0);
        assert!((f[0].1.max.x - 119.5).abs() < 0.01, "{:?}", f[0].1);
        assert!((f[1].1.min.x - 120.5).abs() < 0.01);
        assert!((f[2].1.width() - width3).abs() < 0.01, "第三格不该动");
        // 拖过头:夹在最小尺寸
        t.drag_divider(&t.dividers(r, 1.0)[0], -500.0, 1.0);
        assert!((t.frames(r, 1.0)[0].1.width() - MIN_PANE).abs() < 0.01);
    }

    #[test]
    fn nested_divider_paths_and_equalize() {
        // [1 | [2 / 3]]
        let mut t = Node::Leaf(1).inserting(2, 1, Axis::Horizontal).inserting(3, 2, Axis::Vertical);
        let r = screen(401.0, 201.0);
        let d = t.dividers(r, 1.0);
        assert_eq!(d.len(), 2);
        assert_eq!((d[1].path.clone(), d[1].axis), (vec![1], Axis::Vertical));
        assert!((d[1].rect.min.x - 201.0).abs() < 0.01 && (d[1].rect.max.x - 401.0).abs() < 0.01);
        t.drag_divider(&d[1], 80.0, 1.0);
        assert!((t.frames(r, 1.0)[1].1.height() - 79.5).abs() < 0.01);
        t.equalize_at(&[1]);
        assert!((t.frames(r, 1.0)[1].1.height() - 100.0).abs() < 0.01);
        t.drag_divider(&d[0], 100.0, 1.0);
        t.equalize();
        assert_eq!(t, Node::Leaf(1).inserting(2, 1, Axis::Horizontal).inserting(3, 2, Axis::Vertical));
    }

    #[test]
    fn splitting_a_resized_pane_halves_its_share() {
        let mut t = Node::Leaf(1).inserting(2, 1, Axis::Horizontal);
        let r = screen(401.0, 100.0);
        t.drag_divider(&t.dividers(r, 1.0)[0], 300.0, 1.0); // 1 占 3/4
        let t = t.inserting(3, 1, Axis::Horizontal);
        let f = t.frames(r, 1.0);
        assert!((f[0].1.width() - f[1].1.width()).abs() < 0.01, "1 和 3 对半");
        assert!((f[2].1.width() - 100.0).abs() < 0.5, "2 不受影响:{:?}", f[2].1);
        // 关掉 3:它那份还给前一个兄弟(1),2 仍不动
        let t = t.removing(3).unwrap();
        let f = t.frames(r, 1.0);
        assert!((f[0].1.width() - 299.5).abs() < 0.01, "{:?}", f[0].1);
        // 关掉第一个:那份给后一个
        let t = Node::Leaf(1).inserting(2, 1, Axis::Horizontal).inserting(3, 2, Axis::Horizontal).removing(1).unwrap();
        assert_eq!(t, Node::Branch(Axis::Horizontal, vec![Node::Leaf(2), Node::Leaf(3)], vec![2.0, 1.0]));
    }

    #[test]
    fn inserting_unknown_target_is_noop() {
        let t = Node::Leaf(1);
        assert_eq!(t.inserting(2, 9, Axis::Horizontal), t);
    }
}
