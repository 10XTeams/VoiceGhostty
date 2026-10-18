//! 标签页的分屏布局:一个终端(叶子),或一行/一列子节点(分支)。分支可嵌套,所以能拼出任意排布。
//! 纯数据 + 纯函数,规则与 Swift 版 `SplitNode` 一致。

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node<Id> {
    Leaf(Id),
    Branch(Axis, Vec<Node<Id>>),
}

impl<Id: Copy + PartialEq> Node<Id> {
    /// 所有终端,按视觉顺序(左→右 / 上→下)
    pub fn leaves(&self) -> Vec<Id> {
        match self {
            Node::Leaf(id) => vec![*id],
            Node::Branch(_, children) => children.iter().flat_map(Node::leaves).collect(),
        }
    }

    pub fn contains(&self, id: Id) -> bool {
        self.leaves().contains(&id)
    }

    /// 每个窗格在 `rect` 里的位置,兄弟之间留 `gap`(间隙露出底色,就是分隔线)。顺序与 `leaves` 一致
    pub fn frames(&self, rect: Rect, gap: f32) -> Vec<(Id, Rect)> {
        match self {
            Node::Leaf(id) => vec![(*id, rect)],
            Node::Branch(axis, children) => {
                let n = children.len() as f32;
                let gaps = gap * (n - 1.0);
                children
                    .iter()
                    .enumerate()
                    .flat_map(|(i, child)| {
                        let i = i as f32;
                        let slot = match axis {
                            Axis::Horizontal => {
                                let w = ((rect.width() - gaps) / n).max(0.0);
                                let x = rect.min.x + i * (w + gap);
                                Rect::from_min_max(pos2(x, rect.min.y), pos2(x + w, rect.max.y))
                            }
                            Axis::Vertical => {
                                let h = ((rect.height() - gaps) / n).max(0.0);
                                let y = rect.min.y + i * (h + gap);
                                Rect::from_min_max(pos2(rect.min.x, y), pos2(rect.max.x, y + h))
                            }
                        };
                        child.frames(slot, gap)
                    })
                    .collect()
            }
        }
    }

    /// 在 `target` 旁边插入 `new`。沿窗格所在的同一方向再分,是加一个**兄弟**——连按三次 ⌘D 得到三等分,
    /// 而不是 ½ + ¼ + ¼ 的歪嵌套;垂直方向分则把叶子换成两个孩子的分支,网格就是这样形成的。
    /// 找不到 `target` 时原样返回。
    pub fn inserting(&self, new: Id, target: Id, axis: Axis) -> Node<Id> {
        match self {
            Node::Leaf(id) if *id == target => Node::Branch(axis, vec![Node::Leaf(*id), Node::Leaf(new)]),
            Node::Leaf(_) => self.clone(),
            Node::Branch(branch_axis, children) => {
                if *branch_axis == axis {
                    if let Some(i) = children.iter().position(|c| *c == Node::Leaf(target)) {
                        let mut siblings = children.clone();
                        siblings.insert(i + 1, Node::Leaf(new));
                        return Node::Branch(*branch_axis, siblings);
                    }
                }
                Node::Branch(*branch_axis, children.iter().map(|c| c.inserting(new, target, axis)).collect())
            }
        }
    }

    /// 移除一个终端。只剩一个孩子的分支收缩成那个孩子,关窗格不会留下空架子。子树空了返回 None
    pub fn removing(&self, target: Id) -> Option<Node<Id>> {
        match self {
            Node::Leaf(id) => (*id != target).then(|| self.clone()),
            Node::Branch(axis, children) => {
                let mut kept: Vec<Node<Id>> = children.iter().filter_map(|c| c.removing(target)).collect();
                match kept.len() {
                    0 => None,
                    1 => kept.pop(),
                    _ => Some(Node::Branch(*axis, kept)),
                }
            }
        }
    }

    /// `target` 在 `dir` 方向上相邻的窗格:向上找最近的沿该方向排列、且那一侧还有兄弟的祖先,
    /// 跨到兄弟,再下到它朝向来处的边缘
    pub fn neighbor(&self, target: Id, dir: Dir) -> Option<Id> {
        let path = self.chain(target)?;
        for (branch, index) in path.into_iter().rev() {
            let Node::Branch(axis, children) = branch else { continue };
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
            Node::Branch(_, children) => children.iter().enumerate().find_map(|(i, child)| {
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
            Node::Branch(axis, children) => {
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
        assert_eq!(t, Node::Branch(Axis::Horizontal, vec![Node::Leaf(1), Node::Leaf(2), Node::Leaf(3)]));
        let frames = t.frames(Rect::from_min_size(pos2(0.0, 0.0), vec2(302.0, 100.0)), 1.0);
        assert_eq!(frames.len(), 3);
        assert!((frames[1].1.min.x - 101.0).abs() < 1e-3 && (frames[1].1.width() - 100.0).abs() < 1e-3);
    }

    #[test]
    fn perpendicular_split_nests() {
        let t = Node::Leaf(1).inserting(2, 1, Axis::Horizontal).inserting(3, 2, Axis::Vertical);
        assert_eq!(
            t,
            Node::Branch(
                Axis::Horizontal,
                vec![Node::Leaf(1), Node::Branch(Axis::Vertical, vec![Node::Leaf(2), Node::Leaf(3)])]
            )
        );
        assert_eq!(t.leaves(), vec![1, 2, 3]);
    }

    #[test]
    fn removing_collapses_single_child_branches() {
        let t = Node::Leaf(1).inserting(2, 1, Axis::Horizontal).inserting(3, 2, Axis::Vertical);
        assert_eq!(
            t.removing(3),
            Some(Node::Branch(Axis::Horizontal, vec![Node::Leaf(1), Node::Leaf(2)]))
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

    #[test]
    fn inserting_unknown_target_is_noop() {
        let t = Node::Leaf(1);
        assert_eq!(t.inserting(2, 9, Axis::Horizontal), t);
    }
}
