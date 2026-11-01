use super::*;

fn state(text: &str) -> EditorState {
    let mut state = EditorState::default();
    state.sync(text);
    state
}

#[test]
fn line_index_follows_edits() {
    let mut text = "un\ndeux\r\ntrois".to_string();
    let mut s = state(&text);
    assert_eq!(s.line_count(), 3);
    assert_eq!(s.line(&text, 1), "deux");
    s.select(3, 3);
    s.insert(&mut text, "a\nb\n", false);
    assert_eq!(text, "un\na\nb\ndeux\r\ntrois");
    assert_eq!(s.line_count(), 5);
    assert_eq!(s.line(&text, 3), "deux");
    assert_eq!(s.line_of(text.len()), 4);
    let mut fresh = state(&text);
    fresh.sync(&text);
    assert_eq!(
        fresh.lines, s.lines,
        "incremental index equals a full rebuild"
    );
}

#[test]
fn typing_merges_into_one_undo_step() {
    let mut text = String::new();
    let mut s = state(&text);
    for c in ["a", "b", "c"] {
        s.type_text(&mut text, c);
    }
    s.newline(&mut text);
    assert_eq!(text, "abc\n");
    assert!(s.undo(&mut text));
    assert_eq!(text, "abc");
    assert!(s.undo(&mut text));
    assert_eq!(text, "");
    assert!(s.redo(&mut text));
    assert_eq!(text, "abc");
    assert_eq!(s.cursor, 3);
}

#[test]
fn brackets_close_and_enter_indents_between_them() {
    let mut text = "    fn f() ".to_string();
    let mut s = state(&text);
    s.select(text.len(), text.len());
    s.type_text(&mut text, "{");
    assert_eq!(text, "    fn f() {}");
    s.newline(&mut text);
    assert_eq!(text, "    fn f() {\n        \n    }");
    assert_eq!(s.position(&text), (2, 9));
    // Typing the closing bracket steps over it.
    let mut text = "()".to_string();
    let mut s = state(&text);
    s.select(1, 1);
    s.type_text(&mut text, ")");
    assert_eq!(text, "()");
    assert_eq!(s.cursor, 2);
}

#[test]
fn intellij_line_actions() {
    let mut text = "let a = 1;\n    let b = 2;\n".to_string();
    let mut s = state(&text);
    s.select(0, 15);
    s.toggle_comment(&mut text, "rs");
    assert_eq!(text, "// let a = 1;\n//     let b = 2;\n");
    s.toggle_comment(&mut text, "rs");
    assert_eq!(text, "let a = 1;\n    let b = 2;\n");
    s.select(0, 0);
    s.duplicate(&mut text);
    assert_eq!(text, "let a = 1;\nlet a = 1;\n    let b = 2;\n");
    s.delete_line(&mut text);
    assert_eq!(text, "let a = 1;\n    let b = 2;\n");
    s.select(0, 0);
    s.key(&mut text, "rs", Key::Tab, Modifiers::NONE, 10);
    assert_eq!(text, "    let a = 1;\n    let b = 2;\n");
}

#[test]
fn word_moves_and_smart_backspace() {
    let text = "let value_1 = foo(bar);".to_string();
    let s = state(&text);
    assert_eq!(s.word_right(&text, 0), 4);
    assert_eq!(s.word_right(&text, 4), 12);
    assert_eq!(s.word_left(&text, 11), 4);
    assert_eq!(s.word_at(&text, 6), 4..11);
    let mut text = "        x".to_string();
    let mut s = state(&text);
    s.select(8, 8);
    s.backspace(&mut text, false);
    assert_eq!(text, "    x");
}

#[test]
fn columns_expand_tabs() {
    assert_eq!(column_of("\tab", 1), 4);
    assert_eq!(offset_of("\tab", 4), 1);
    assert_eq!(offset_of("\tab", 5), 2);
    assert_eq!(offset_of("abc", 10), 3);
    assert_eq!(display("\tx"), "    x");
}

#[test]
fn extend_selection_steps_out_and_shrink_comes_back() {
    let text = "call(alpha, \"two words\", [x + 1]);\nnext();\n";
    let mut s = state(text);
    let at = text.find("words").unwrap() + 1;
    s.select(at, at);
    s.extend_selection(text);
    assert_eq!(&text[s.selection()], "words");
    s.extend_selection(text);
    assert_eq!(&text[s.selection()], "two words");
    s.extend_selection(text);
    assert_eq!(&text[s.selection()], "\"two words\"");
    s.extend_selection(text);
    assert_eq!(&text[s.selection()], "alpha, \"two words\", [x + 1]");
    s.extend_selection(text);
    assert_eq!(&text[s.selection()], "(alpha, \"two words\", [x + 1])");
    s.extend_selection(text);
    assert_eq!(
        &text[s.selection()],
        "call(alpha, \"two words\", [x + 1])",
        "the call"
    );
    s.extend_selection(text);
    assert_eq!(&text[s.selection()], "call(alpha, \"two words\", [x + 1]);");
    s.shrink_selection();
    s.shrink_selection();
    s.shrink_selection();
    assert_eq!(&text[s.selection()], "alpha, \"two words\", [x + 1]");
    s.select(at, at);
    s.shrink_selection();
    assert_eq!(s.selection(), at..at, "nothing to shrink to");
}

#[test]
fn lines_join_move_and_change_case() {
    let mut text = "let a = foo(\n    1,\n    2\n);\nlast\n".to_string();
    let mut s = state(&text);
    s.select(0, 0);
    s.join_lines(&mut text);
    assert_eq!(text, "let a = foo(1,\n    2\n);\nlast\n");
    s.select(0, text.find("2\n);").unwrap() + 4);
    s.join_lines(&mut text);
    assert_eq!(text, "let a = foo(1, 2);\nlast\n");
    s.select(0, 0);
    s.move_lines(&mut text, false);
    assert_eq!(text, "last\nlet a = foo(1, 2);\n");
    assert_eq!(s.line_of(s.cursor), 1, "the caret moves with its line");
    s.move_lines(&mut text, true);
    assert_eq!(text, "let a = foo(1, 2);\nlast\n");
    s.move_lines(&mut text, true);
    assert_eq!(text, "let a = foo(1, 2);\nlast\n", "the first line stays");
    s.select(4, 4);
    s.toggle_case(&mut text);
    assert_eq!(text, "let A = foo(1, 2);\nlast\n");
    s.select(8, 11);
    s.toggle_case(&mut text);
    s.toggle_case(&mut text);
    assert_eq!(&text[8..11], "foo", "upper case, then back");
    assert!(s.key(
        &mut text,
        "rs",
        Key::ArrowDown,
        Modifiers::ALT | Modifiers::SHIFT,
        10
    ));
    assert_eq!(text, "last\nlet A = foo(1, 2);\n");
}

#[test]
fn f2_goes_to_the_next_error_round_the_end() {
    use crate::languages::{Diagnostic, Severity};
    let text = "a\nb\nc\nd\n";
    let mut s = state(text);
    let problem = |at: usize, severity| Diagnostic {
        range: at..at + 1,
        severity,
        message: format!("at {at}"),
        source: String::new(),
    };
    s.assist.diagnostics = std::sync::Arc::new(vec![
        problem(6, Severity::Error),
        problem(2, Severity::Warning),
        problem(4, Severity::Error),
    ]);
    s.select(0, 0);
    s.next_problem(true);
    assert_eq!(s.cursor, 4, "errors first: the warning is skipped");
    assert_eq!(s.assist.message.as_deref(), Some("at 4"));
    s.next_problem(true);
    assert_eq!(s.cursor, 6);
    s.next_problem(true);
    assert_eq!(s.cursor, 4, "round the end");
    s.next_problem(false);
    assert_eq!(s.cursor, 6, "Shift+F2 goes back, round the start");
    s.assist.diagnostics = std::sync::Arc::new(vec![problem(2, Severity::Warning)]);
    s.next_problem(true);
    assert_eq!(s.cursor, 2, "no error: the warnings");
    s.assist.diagnostics = std::sync::Arc::default();
    s.next_problem(true);
    assert_eq!(s.cursor, 2);
}

#[test]
fn colors_have_swatches_in_every_language() {
    let shown = |text: &str, language: &str| -> Vec<String> {
        swatches(text, language)
            .iter()
            .map(|s| text[s.range.clone()].to_string())
            .collect()
    };
    // Code: hexadecimal and functions, in strings or not.
    assert_eq!(
        shown(
            "let c = \"#e34f26\"; let d = rgb(0, 128, 255); // #fff",
            "rs"
        ),
        ["#e34f26", "rgb(0, 128, 255)", "#fff"]
    );
    assert_eq!(
        shown("COLOR = '#123'\nfix #123 and #abc", "py"),
        ["#123", "#abc"]
    );
    // Not a color: an entity, a word that only starts like one, a name outside a sheet.
    assert!(shown("&#123; #ffz red", "md").is_empty());
    // A page: its styles as CSS (names included), the rest as code.
    let page = "<style>p { color: red; }</style><div style=\"background: #00f\" data-c=\"#0f0\">";
    assert_eq!(shown(page, "html"), ["red", "#00f", "#0f0"]);
}

#[test]
fn a_style_sheet_has_the_swatches_of_its_gradients_shadows_and_curves() {
    use crate::languages::css::colors::Special;
    let sheet = "a {\n  background: linear-gradient(red, blue);\n  box-shadow: 0 1px 2px #000;\n  \
                 transition: color 1s ease;\n  border-radius: 4px;\n  transform: rotate(5deg);\n}";
    for language in ["css", "scss", "less"] {
        let kinds: Vec<&str> = swatches(sheet, language)
            .iter()
            .filter_map(|s| s.special.as_deref())
            .map(|special| match special {
                Special::Gradient(_) => "gradient",
                Special::Shadows(_) => "shadows",
                Special::Easing(_) => "easing",
                Special::Radius(_) => "radius",
                Special::Effects(_) => "effects",
            })
            .collect();
        assert_eq!(
            kinds,
            ["gradient", "shadows", "easing", "radius", "effects"],
            "{language}"
        );
    }
}
