use super::*;

#[test]
fn ctrl_y_deletes_the_current_line() {
    let mut text = "one\ntwo\nthree".to_string();
    let mut s = EditorState::default();
    s.sync(&text);
    s.select(5, 5);
    assert!(s.key(&mut text, "js", Key::Y, Modifiers::COMMAND, 10));
    assert_eq!(text, "one\nthree");
    s.select(text.len(), text.len());
    assert!(s.key(&mut text, "js", Key::Y, Modifiers::COMMAND, 10));
    assert_eq!(text, "one");
}

#[test]
fn ctrl_y_redoes_what_was_just_undone() {
    let mut text = "one\ntwo".to_string();
    let mut s = EditorState::default();
    s.sync(&text);
    s.select(3, 3);
    s.insert(&mut text, "!", false);
    assert_eq!(text, "one!\ntwo");
    assert!(s.undo(&mut text));
    assert_eq!(text, "one\ntwo");
    assert!(s.key(&mut text, "js", Key::Y, Modifiers::COMMAND, 10));
    assert_eq!(text, "one!\ntwo", "redone, not a deleted line");
    assert!(s.key(&mut text, "js", Key::Y, Modifiers::COMMAND, 10));
    assert_eq!(text, "two", "nothing left to redo: the line goes");
}

fn folding(text: &str) -> EditorState {
    let mut s = EditorState::default();
    s.sync(text);
    s.refresh_folds(text, "js");
    s
}

#[test]
fn a_collapsed_region_hides_its_lines_and_the_caret_skips_them() {
    let text = "function a() {
  one();
  two();
}
next();
";
    let mut s = folding(text);
    let open = s.region_at(0).unwrap().open;
    s.toggle_fold(open);
    assert_eq!(s.row_count(), 3, "the function is one row");
    assert_eq!((s.line_at(0), s.line_at(1)), (0, 4));
    assert_eq!(s.row_of(2), 0, "a hidden line is shown on its fold");
    s.select(0, 0);
    s.vertical(text, 1, false);
    assert_eq!(s.line_of(s.cursor), 4, "down goes past the hidden lines");
    s.vertical(text, -1, false);
    assert_eq!(s.line_of(s.cursor), 0);
}

#[test]
fn folds_follow_the_edits_and_open_when_the_caret_enters() {
    let mut text = "x();
function a() {
  one();
}
"
    .to_string();
    let mut s = folding(&text);
    s.fold_caret(true);
    assert!(s.folded.is_empty(), "no region around the first line");
    s.select(10, 10);
    s.fold_caret(true);
    assert_eq!(s.folded.len(), 1);
    assert_eq!(s.line_of(s.cursor), 1, "the caret stays on the first line");
    s.replace_range(
        &mut text,
        0..0,
        "y();
",
    );
    s.refresh_folds(&text, "js");
    assert_eq!(
        s.folded.len(),
        1,
        "an edit above keeps the region collapsed"
    );
    assert_eq!(s.row_count(), 4);
    s.select(text.find("one").unwrap(), text.find("one").unwrap());
    s.refresh_folds(&text, "js");
    assert!(s.folded.is_empty(), "going into it opens it");
    assert_eq!(s.row_count(), s.line_count());
}
