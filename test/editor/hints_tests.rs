use super::*;

fn state(text: &str, hints: Vec<InlayHint>) -> EditorState {
    let mut state = EditorState::default();
    state.sync(text);
    state.hints = hints;
    state
}

fn hint(offset: usize, label: &str, kind: HintKind) -> InlayHint {
    InlayHint {
        offset,
        label: label.to_string(),
        kind,
    }
}

#[test]
fn the_caret_goes_after_an_argument_hint_and_before_a_type_hint() {
    // let x = f(1);   a type after `x`, the name of the argument before `1`
    let text = "let x = f(1);";
    let s = state(
        text,
        vec![
            hint(5, ": i32", HintKind::Type),
            hint(10, "n:", HintKind::Parameter),
        ],
    );
    // Before the type hint nothing is added, after it five columns are.
    assert_eq!(s.visual_column(text, 0, 5), 5);
    assert_eq!(s.visual_column(text, 0, 6), 11);
    // At the argument, the caret is after its hint ("n: " is three columns).
    assert_eq!(s.visual_column(text, 0, 10), 5 + 10 + 3);
    assert_eq!(s.visual_column(text, 0, 9), 5 + 9);
}

#[test]
fn a_click_in_a_hint_goes_to_the_column_it_is_before() {
    let text = "let x = 1;";
    let s = state(text, vec![hint(5, ": u8", HintKind::Type)]);
    // The hint covers the columns 5..9 of the layout.
    for visual in 5..9 {
        assert_eq!(s.source_column(text, 0, visual), 5);
    }
    assert_eq!(s.source_column(text, 0, 9), 5);
    assert_eq!(s.source_column(text, 0, 10), 6);
    assert_eq!(s.source_column(text, 0, 3), 3);
}

#[test]
fn hints_follow_the_edits_of_the_text() {
    let mut text = "let x = 1;\nlet y = 2;".to_string();
    let mut s = state(&text, vec![hint(5, ": i32", HintKind::Type), hint(16, ": i32", HintKind::Type)]);
    s.select(0, 0);
    s.insert(&mut text, "// c\n", false);
    assert_eq!(s.hints[0].offset, 10);
    assert_eq!(s.hints[1].offset, 21);
    // A hint inside the replaced text goes away.
    s.replace_range(&mut text, 8..12, "");
    assert_eq!(s.hints.len(), 1);
}
