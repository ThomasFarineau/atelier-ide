use super::*;

fn state(text: &str) -> EditorState {
    let mut state = EditorState::default();
    state.sync(text);
    state
}

#[test]
fn next_occurrence_adds_a_caret_and_typing_edits_both() {
    let mut text = "foo bar foo".to_string();
    let mut s = state(&text);
    s.select(0, 0);
    // The first time selects the word, the second adds the next occurrence.
    s.add_next_occurrence(&text);
    assert_eq!(s.selection(), 0..3);
    assert!(!s.has_extras());
    s.add_next_occurrence(&text);
    assert!(s.has_extras());
    assert_eq!(s.selection(), 8..11);
    s.each_caret(&mut text, |s, t, _| s.type_text(t, "x"));
    assert_eq!(text, "x bar x");
    assert!(s.undo(&mut text));
    assert_eq!(
        text, "foo bar foo",
        "one undo takes back the edit of every caret"
    );
    assert!(!s.has_extras());
    assert!(s.redo(&mut text));
    assert_eq!(text, "x bar x");
}

#[test]
fn backspace_at_each_caret() {
    let mut text = "ab\ncd\nef".to_string();
    let mut s = state(&text);
    s.select(2, 2);
    s.toggle_caret(5);
    s.toggle_caret(8);
    s.each_caret(&mut text, |s, t, _| s.backspace(t, false));
    assert_eq!(text, "a\nc\ne");
    assert_eq!(s.extras.len(), 2);
}

#[test]
fn all_occurrences_and_unselect() {
    let text = "a.b a.b a.b";
    let mut s = state(text);
    s.select(0, 3);
    s.select_all_occurrences(text);
    assert_eq!(s.extras.len(), 2);
    s.unselect_occurrence();
    assert_eq!(s.extras.len(), 1);
}

#[test]
fn column_selection_follows_short_lines() {
    let mut text = "abcd\nef\nghij".to_string();
    let mut s = state(&text);
    s.select_columns(&text, (0, 1), (2, 3));
    assert_eq!(s.extras.len(), 2);
    s.each_caret(&mut text, |s, t, _| s.type_text(t, "Z"));
    assert_eq!(text, "aZd\neZ\ngZj");
}

#[test]
fn carets_that_meet_merge() {
    let mut text = "ab".to_string();
    let mut s = state(&text);
    s.select(1, 1);
    s.toggle_caret(2);
    s.each_caret(&mut text, |s, t, _| s.backspace(t, false));
    assert_eq!(text, "");
    assert!(!s.has_extras());
}

#[test]
fn paste_gives_a_line_to_each_caret() {
    let mut text = "..\n..".to_string();
    let mut s = state(&text);
    s.select(0, 0);
    s.toggle_caret(3);
    let lines = s.paste_lines("1\n2").expect("a line for each");
    s.each_caret(&mut text, |s, t, rank| s.insert(t, &lines[rank], false));
    assert_eq!(text, "1..\n2..");
}

#[test]
fn move_statement_carries_the_block() {
    let mut text = "a();\nif x {\n    b();\n}\nc();".to_string();
    let mut s = state(&text);
    // On the line that opens the block: it moves whole, below c().
    s.select(5, 5);
    s.move_statement(&mut text, false);
    assert_eq!(text, "a();\nc();\nif x {\n    b();\n}");
    assert_eq!(s.line_of(s.cursor), 2);
    s.move_statement(&mut text, true);
    assert_eq!(text, "a();\nif x {\n    b();\n}\nc();");
}

#[test]
fn a_statement_does_not_leave_its_block() {
    let mut text = "if x {\n    b();\n}\n".to_string();
    let mut s = state(&text);
    s.select(10, 10);
    s.move_statement(&mut text, true);
    assert_eq!(text, "if x {\n    b();\n}\n", "b() stays inside the braces");
    s.move_statement(&mut text, false);
    assert_eq!(text, "if x {\n    b();\n}\n");
}
