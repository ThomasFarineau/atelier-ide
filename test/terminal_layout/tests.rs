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
