use super::*;

fn state(text: &str) -> EditorState {
    let mut state = EditorState::default();
    state.sync(text);
    state.refresh_folds(text, "rs");
    state
}

const CODE: &str = "impl A {\n    fn f() {\n        one();\n        two();\n        three();\n    }\n\n    fn g() {\n        four();\n    }\n}\n";

#[test]
fn the_blocks_around_a_line_are_its_headers() {
    let s = state(CODE);
    assert_eq!(s.block_headers(CODE, 3), vec![0, 1]);
    assert_eq!(s.block_headers(CODE, 8), vec![0, 7]);
    // The line that opens a block is not in it.
    assert_eq!(s.block_headers(CODE, 1), vec![0]);
    assert_eq!(s.block_headers(CODE, 0), Vec::<usize>::new());
}

#[test]
fn sticky_lines_account_for_the_rows_they_hide() {
    let s = state(CODE);
    // With row 3 on top the two headers hide rows 3 and 4: the first line left shown is
    // inside the same blocks, so both stay.
    assert_eq!(s.sticky_lines(CODE, 3), vec![0, 1]);
    // At the top nothing is kept.
    assert_eq!(s.sticky_lines(CODE, 0), Vec::<usize>::new());
    // Past the end of the function g, only the impl.
    assert_eq!(s.sticky_lines(CODE, 6), vec![0]);
}

#[test]
fn a_signature_over_several_lines_is_kept_whole_from_its_first_line() {
    let code = "fn long(\n    a: i32,\n    b: i32,\n) -> i32 {\n    let x = a;\n    x + b\n}\n";
    let s = state(code);
    // The block opens on `) -> i32 {`, the header is the line that opened the parenthesis.
    assert_eq!(s.block_headers(code, 4), vec![0]);
}

#[test]
fn parentheses_that_span_lines_are_not_blocks() {
    let code = "fn f() {\n    call(\n        one,\n        two,\n    );\n}\n";
    let s = state(code);
    assert_eq!(s.block_headers(code, 2), vec![0], "only the function, not `call(`");
}
