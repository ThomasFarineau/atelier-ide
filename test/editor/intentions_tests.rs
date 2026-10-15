use super::*;

/// The text after each intention offered at the caret (marked `|`): the titles are in the language
/// of the user, the results are not.
fn results(code: &str, language: &str) -> Vec<String> {
    let cursor = code.find('|').expect("a caret");
    let text = code.replacen('|', "", 1);
    intentions(&text, cursor, language)
        .iter()
        .map(|fix| {
            let mut edits = fix.edits.clone();
            edits.sort_by_key(|e| std::cmp::Reverse(e.range.start));
            let mut out = text.clone();
            for edit in edits {
                out.replace_range(edit.range, &edit.new_text);
            }
            out
        })
        .collect()
}

fn offers(code: &str, language: &str, expected: &str) -> bool {
    results(code, language).iter().any(|r| r == expected)
}

#[test]
fn quotes_change_and_the_inside_follows() {
    assert!(offers("let a = \"he|llo\";\n", "js", "let a = 'hello';\n"));
    assert!(offers("let a = 'it\\'s |here';\n", "js", "let a = \"it's here\";\n"));
    assert!(offers(
        "let a = \"say \\\"|x\\\"\";\n",
        "js",
        "let a = 'say \"x\"';\n"
    ));
    // Rust has no single-quoted strings.
    assert!(!offers("let a = \"x|\";\nlet b = 1;\n", "rs", "let a = 'x';\nlet b = 1;\n"));
}

#[test]
fn a_string_is_split_where_the_caret_is() {
    assert!(offers("f(\"ab|cd\");\n", "js", "f(\"ab\" + \"cd\");\n"));
    // At the edge of the string there is nothing to split.
    assert!(!offers("f(|\"abcd\");\n", "js", "f(\"\" + \"abcd\");\n"));
}

#[test]
fn comparisons_and_booleans_are_swapped() {
    assert!(offers("if (a =|= b) {}\n", "js", "if (a != b) {}\n"));
    assert!(offers("if (a =|= b) {}\n", "js", "if (a === b) {}\n"));
    assert!(offers("if (a <|= b) {}\n", "rs", "if (a > b) {}\n"));
    assert!(offers("if (a &|& b) {}\n", "rs", "if (a || b) {}\n"));
    assert!(offers("let x = tr|ue;\n", "rs", "let x = false;\n"));
    assert!(offers("let x = a +| b;\n", "js", "let x = a - b;\n"));
    // An arrow, a sign and an increment are not operators to swap.
    assert!(results("let f = (a) =|> a;\n", "js").is_empty());
    assert!(results("let x = -|1;\n", "js").is_empty());
    assert!(results("i+|+;\n", "js").is_empty());
    // A string's content is no operator.
    assert!(!offers("let s = \"a =|= b\";\n", "js", "let s = \"a != b\";\n"));
}

#[test]
fn var_and_mutability() {
    assert!(offers("  v|ar x = 1;\n", "js", "  let x = 1;\n"));
    assert!(offers("  v|ar x = 1;\n", "js", "  const x = 1;\n"));
    assert!(offers("  let| x = 1;\n", "rs", "  let mut x = 1;\n"));
    assert!(offers("  let| mut x = 1;\n", "rs", "  let x = 1;\n"));
}

#[test]
fn braces_are_added_and_removed() {
    assert!(offers(
        "    if (a > |1) go();\n",
        "js",
        "    if (a > 1) {\n        go();\n    }\n"
    ));
    assert!(offers(
        "    el|se stop();\n",
        "java",
        "    else {\n        stop();\n    }\n"
    ));
    assert!(results("    if (a) { go(); }|\n", "js").is_empty());
    // Rust needs its braces.
    assert!(results("    if (a|) go();\n", "rs").is_empty());
    assert!(offers(
        "    if (a|) {\n        go();\n    }\n    next();\n",
        "js",
        "    if (a) go();\n    next();\n"
    ));
    // Two statements: the braces stay.
    assert!(results("    if (a|) {\n        go();\n        stop();\n    }\n", "js").is_empty());
}
