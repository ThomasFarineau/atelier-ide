use super::*;

fn lines_of(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect()
}

fn spans(text: &str, language: &str) -> Vec<(usize, usize)> {
    regions(text, &lines_of(text), language)
        .iter()
        .map(|r| (r.first, r.last))
        .collect()
}

#[test]
fn braces_that_span_lines_fold_and_nested_ones_too() {
    let text = "fn a() {\n    if x {\n        y();\n    }\n}\nfn b() { 1 }\n";
    assert_eq!(spans(text, "rs"), [(0, 4), (1, 3)]);
}

#[test]
fn brackets_in_strings_and_comments_are_ignored() {
    let text = "const a = \"{\";\n// }\nfoo({\n  b: 1,\n});\n";
    assert_eq!(spans(text, "js"), [(2, 4)]);
    let regions = regions(text, &lines_of(text), "js");
    assert_eq!(
        &text[regions[0].head_end - 1..regions[0].head_end],
        "{",
        "the brace wins over the parenthesis"
    );
}

#[test]
fn lifetimes_and_chars_do_not_swallow_the_code() {
    let text = "fn f<'a>(x: &'a str) -> char {\n    let c = '{';\n    '}'\n}\n";
    assert_eq!(spans(text, "rs"), [(0, 3)]);
}

#[test]
fn block_comments_and_import_runs_fold() {
    let text =
        "import a from 'a';\nimport b from 'b';\nimport c from 'c';\n/*\n * doc\n */\nlet x;\n";
    assert_eq!(spans(text, "js"), [(0, 2), (3, 5)]);
}

#[test]
#[allow(clippy::single_range_in_vec_init)]
fn collapsed_regions_hide_their_lines_but_not_the_first() {
    let text = "a {\n b {\n  c\n }\n}\n";
    let all = regions(text, &lines_of(text), "js");
    assert_eq!(hidden(&all, &[all[1].open]), [2..4]);
    assert_eq!(hidden(&all, &[all[0].open, all[1].open]), [1..5]);
    assert!(hidden(&all, &[]).is_empty());
}
