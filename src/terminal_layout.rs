//! Layout of the panes of a terminal tab: a binary tree of splits, as IntelliJ nests them.
//! Each split is side by side or stacked and has a ratio the user drags.

use eframe::egui::{Rect, pos2, vec2};

pub enum Node<T> {
    Leaf(T),
    Split {
        /// Children stacked top to bottom instead of side by side.
        stacked: bool,
        /// Share of the space taken by `first`, between 0 and 1.
        ratio: f32,
        first: Box<Node<T>>,
        second: Box<Node<T>>,
    },
    /// Only exists while the tree is being rebuilt.
    Empty,
}

/// A draggable line between the two children of a split.
pub struct Divider {
    /// Way down from the root: false is the first child, true the second.
    pub path: Vec<bool>,
    pub rect: Rect,
    pub stacked: bool,
    /// Area of the whole split, to turn a pointer position into a ratio.
    pub area: Rect,
}

pub const MIN_RATIO: f32 = 0.1;

impl<T> Node<T> {
    pub fn leaves(&self) -> Vec<&T> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect<'a>(&'a self, out: &mut Vec<&'a T>) {
        match self {
            Node::Leaf(t) => out.push(t),
            Node::Split { first, second, .. } => {
                first.collect(out);
                second.collect(out);
            }
            Node::Empty => {}
        }
    }

    pub fn leaves_mut(&mut self) -> Vec<&mut T> {
        let mut out = Vec::new();
        self.collect_mut(&mut out);
        out
    }

    fn collect_mut<'a>(&'a mut self, out: &mut Vec<&'a mut T>) {
        match self {
            Node::Leaf(t) => out.push(t),
            Node::Split { first, second, .. } => {
                first.collect_mut(out);
                second.collect_mut(out);
            }
            Node::Empty => {}
        }
    }

    /// Replaces the leaf `is_target` by a split holding it and `new`, which comes second.
    pub fn split(&mut self, is_target: &impl Fn(&T) -> bool, stacked: bool, new: T) -> bool {
        self.split_with(is_target, stacked, false, Node::Leaf(new))
    }

    /// Same with a whole tree, placed before the leaf (left or above) or after it.
    pub fn split_with(
        &mut self,
        is_target: &impl Fn(&T) -> bool,
        stacked: bool,
        before: bool,
        new: Node<T>,
    ) -> bool {
        match self {
            Node::Leaf(t) if is_target(t) => {
                let old = std::mem::replace(self, Node::Empty);
                let (first, second) = if before { (new, old) } else { (old, new) };
                *self = Node::Split {
                    stacked,
                    ratio: 0.5,
                    first: Box::new(first),
                    second: Box::new(second),
                };
                true
            }
            Node::Split { first, second, .. } => {
                // `new` moves into only one of the branches.
                if first.contains(is_target) {
                    first.split_with(is_target, stacked, before, new)
                } else {
                    second.split_with(is_target, stacked, before, new)
                }
            }
            _ => false,
        }
    }

    pub fn contains(&self, is_target: &impl Fn(&T) -> bool) -> bool {
        self.leaves().into_iter().any(is_target)
    }

    /// Removes the leaf `is_target`; its sibling takes the place of their split. The only
    /// remaining leaf cannot be removed.
    pub fn remove(&mut self, is_target: &impl Fn(&T) -> bool) -> bool {
        self.take(is_target).is_some()
    }

    /// Like `remove`, and gives the leaf back.
    pub fn take(&mut self, is_target: &impl Fn(&T) -> bool) -> Option<T> {
        let Node::Split { first, second, .. } = self else {
            return None;
        };
        let first_is_target = matches!(&**first, Node::Leaf(t) if is_target(t));
        let second_is_target = matches!(&**second, Node::Leaf(t) if is_target(t));
        if first_is_target || second_is_target {
            let (gone, kept) = if first_is_target {
                (first, second)
            } else {
                (second, first)
            };
            let Node::Leaf(leaf) = std::mem::replace(&mut **gone, Node::Empty) else {
                return None;
            };
            *self = std::mem::replace(&mut **kept, Node::Empty);
            Some(leaf)
        } else {
            first.take(is_target).or_else(|| second.take(is_target))
        }
    }

    pub fn ratio_mut(&mut self, path: &[bool]) -> Option<&mut f32> {
        let Node::Split {
            ratio,
            first,
            second,
            ..
        } = self
        else {
            return None;
        };
        match path.split_first() {
            None => Some(ratio),
            Some((false, rest)) => first.ratio_mut(rest),
            Some((true, rest)) => second.ratio_mut(rest),
        }
    }

    /// Rectangle of each leaf, in the order of `leaves`, and the dividers between them.
    pub fn layout(&self, rect: Rect, gap: f32) -> (Vec<Rect>, Vec<Divider>) {
        let (mut rects, mut dividers) = (Vec::new(), Vec::new());
        self.place(rect, gap, &mut Vec::new(), &mut rects, &mut dividers);
        (rects, dividers)
    }

    fn place(
        &self,
        rect: Rect,
        gap: f32,
        path: &mut Vec<bool>,
        rects: &mut Vec<Rect>,
        dividers: &mut Vec<Divider>,
    ) {
        match self {
            Node::Leaf(_) => rects.push(rect),
            Node::Empty => {}
            Node::Split {
                stacked,
                ratio,
                first,
                second,
            } => {
                let total = if *stacked {
                    rect.height()
                } else {
                    rect.width()
                } - gap;
                let size = (total * ratio).max(0.0);
                let (a, line, b) = if *stacked {
                    (
                        Rect::from_min_size(rect.min, vec2(rect.width(), size)),
                        Rect::from_min_size(
                            pos2(rect.left(), rect.top() + size),
                            vec2(rect.width(), gap),
                        ),
                        Rect::from_min_max(pos2(rect.left(), rect.top() + size + gap), rect.max),
                    )
                } else {
                    (
                        Rect::from_min_size(rect.min, vec2(size, rect.height())),
                        Rect::from_min_size(
                            pos2(rect.left() + size, rect.top()),
                            vec2(gap, rect.height()),
                        ),
                        Rect::from_min_max(pos2(rect.left() + size + gap, rect.top()), rect.max),
                    )
                };
                dividers.push(Divider {
                    path: path.clone(),
                    rect: line,
                    stacked: *stacked,
                    area: rect,
                });
                path.push(false);
                first.place(a, gap, path, rects, dividers);
                path.pop();
                path.push(true);
                second.place(b, gap, path, rects, dividers);
                path.pop();
            }
        }
    }
}

impl Divider {
    /// Ratio of the split when the divider is dragged to `pointer` (x or y by orientation).
    pub fn ratio_at(&self, pointer: f32, gap: f32) -> f32 {
        let (start, length) = if self.stacked {
            (self.area.top(), self.area.height())
        } else {
            (self.area.left(), self.area.width())
        };
        ((pointer - start - gap / 2.0) / (length - gap).max(1.0)).clamp(MIN_RATIO, 1.0 - MIN_RATIO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Node<u32> {
        let mut root = Node::Leaf(1);
        assert!(root.split(&|t| *t == 1, false, 2));
        // The second pane is split again, stacked: nested splits.
        assert!(root.split(&|t| *t == 2, true, 3));
        root
    }

    #[test]
    fn splits_nest_and_keep_the_order() {
        let root = tree();
        assert_eq!(root.leaves(), [&1, &2, &3]);
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(106.0, 100.0));
        let (rects, dividers) = root.layout(area, 6.0);
        assert_eq!(rects.len(), 3);
        assert_eq!(dividers.len(), 2);
        // Side by side at the root: 50 + gap 6 + 50.
        assert_eq!(rects[0].width(), 50.0);
        assert_eq!(rects[1].left(), 56.0);
        // The right half is cut in two, one above the other.
        assert_eq!(rects[1].width(), rects[2].width());
        assert!(rects[1].bottom() < rects[2].top());
        assert_eq!(dividers[1].path, [true]);
    }

    #[test]
    fn a_whole_tree_can_be_dropped_before_a_pane() {
        let mut root = tree();
        let mut moved = Node::Leaf(4);
        assert!(moved.split(&|t| *t == 4, true, 5));
        assert!(root.split_with(&|t| *t == 1, false, true, moved));
        assert_eq!(root.leaves(), [&4, &5, &1, &2, &3]);
    }

    #[test]
    fn a_pane_taken_out_can_be_put_back_elsewhere() {
        let mut root = tree();
        assert_eq!(root.take(&|t| *t == 2), Some(2));
        assert_eq!(root.leaves(), [&1, &3]);
        assert!(root.split_with(&|t| *t == 1, true, true, Node::Leaf(2)));
        assert_eq!(root.leaves(), [&2, &1, &3]);
        assert_eq!(root.take(&|t| *t == 9), None);
    }

    #[test]
    fn closing_a_pane_gives_its_space_to_the_sibling() {
        let mut root = tree();
        assert!(root.remove(&|t| *t == 3));
        assert_eq!(root.leaves(), [&1, &2]);
        assert!(root.remove(&|t| *t == 1));
        assert_eq!(root.leaves(), [&2]);
        assert!(!root.remove(&|t| *t == 2), "the last pane stays");
    }

    #[test]
    fn dragging_a_divider_changes_the_ratio_within_limits() {
        let mut root = tree();
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(106.0, 100.0));
        let (_, dividers) = root.layout(area, 6.0);
        let ratio = dividers[0].ratio_at(78.0, 6.0);
        assert!((ratio - 0.75).abs() < 1e-6);
        *root.ratio_mut(&dividers[0].path).unwrap() = ratio;
        let (rects, _) = root.layout(area, 6.0);
        assert_eq!(rects[0].width(), 75.0);
        assert_eq!(dividers[0].ratio_at(-50.0, 6.0), MIN_RATIO);
        assert_eq!(dividers[0].ratio_at(500.0, 6.0), 1.0 - MIN_RATIO);
    }
}
