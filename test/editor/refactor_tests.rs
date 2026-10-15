use super::*;

/// The text after the edits of a change.
fn applied(text: &str, change: &Change) -> String {
    let mut edits = change.edits.clone();
    edits.sort_by_key(|e| std::cmp::Reverse(e.range.start));
    let mut out = text.to_string();
    for edit in edits {
        out.replace_range(edit.range, &edit.new_text);
    }
    out
}

fn starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect()
}

fn range_of(text: &str, part: &str) -> Range<usize> {
    let start = text.find(part).expect("the part is in the text");
    start..start + part.len()
}

#[test]
fn a_variable_is_extracted_above_its_statement() {
    let text = "function f() {\n    return compute(a, b) + 1;\n}\n";
    let change = extract_variable(text, &range_of(text, "compute(a, b)"), "js").unwrap();
    let after = applied(text, &change);
    assert_eq!(
        after,
        "function f() {\n    const result = compute(a, b);\n    return result + 1;\n}\n"
    );
    // The new name is where the rename starts.
    assert!(after[change.name_at.unwrap()..].starts_with("result + 1"));
}

#[test]
fn an_argument_of_a_call_over_several_lines_goes_above_the_call() {
    let text = "foo(\n    bar(1),\n    baz(2)\n);\n";
    let change = extract_variable(text, &range_of(text, "baz(2)"), "js").unwrap();
    assert_eq!(
        applied(text, &change),
        "const result = baz(2);\nfoo(\n    bar(1),\n    result\n);\n"
    );
}

#[test]
fn names_follow_the_language_and_do_not_collide() {
    let text = "let a = 1;\nlet b = f(\"x\") + 1;\n";
    let change = extract_variable(text, &range_of(text, "\"x\""), "rs").unwrap();
    assert!(applied(text, &change).contains("let text = \"x\";\nlet b = f(text) + 1;"));
    // `text` is taken: the next free name.
    let text = "let text = 1;\nlet b = f(\"x\");\n";
    let change = extract_variable(text, &range_of(text, "\"x\""), "rs").unwrap();
    assert!(applied(text, &change).contains("let text2 = \"x\";"));
    assert_eq!(suggest_name("order.getTotal()"), "total");
    assert_eq!(suggest_name("compute(a)"), "result");
    assert_eq!(suggest_name("42"), "value");
}

#[test]
fn a_selection_that_is_not_an_expression_is_refused() {
    let text = "foo(a, (b + c));\n";
    assert!(extract_variable(text, &range_of(text, "a, (b"), "js").is_none());
    assert!(extract_variable(text, &(0..0), "js").is_none());
}

#[test]
fn a_constant_goes_above_the_function_or_into_the_class() {
    let text = "function f(x) {\n    return x * 42;\n}\n";
    let lines = starts(text);
    let change = extract_constant(text, &range_of(text, "42"), "js", &[0], &lines).unwrap();
    assert_eq!(
        applied(text, &change),
        "const CONSTANT = 42;\n\nfunction f(x) {\n    return x * CONSTANT;\n}\n"
    );
    let change = extract_constant(text, &range_of(text, "42"), "rs", &[0], &lines).unwrap();
    assert!(applied(text, &change).starts_with("const CONSTANT: i32 = 42;"));
    // Rust cannot say the type of a call.
    let call = "fn f() {\n    g(h());\n}\n";
    assert!(extract_constant(call, &range_of(call, "h()"), "rs", &[0], &starts(call)).is_none());
    let java = "class A {\n    int f() {\n        return 7;\n    }\n}\n";
    let change = extract_constant(java, &range_of(java, "7"), "java", &[0, 1], &starts(java)).unwrap();
    assert!(applied(java, &change).starts_with("class A {\n    private static final int CONSTANT = 7;\n\n    int f()"));
}

#[test]
fn a_variable_is_inlined_everywhere_it_is_used() {
    let text = "let total = a + b;\nprintln!(\"{}\", total * 2);\nlet z = total;\n";
    let change = inline_variable(text, 5, "rs", None).unwrap();
    assert_eq!(
        applied(text, &change),
        "println!(\"{}\", (a + b) * 2);\nlet z = (a + b);\n"
    );
    // From a use, the declaration above is found; a plain value needs no parentheses.
    let text = "const name = \"x\";\nlog(name);\n";
    let at = text.rfind("name").unwrap();
    let change = inline_variable(text, at + 1, "js", None).unwrap();
    assert_eq!(applied(text, &change), "log(\"x\");\n");
}

#[test]
fn a_variable_that_changes_is_not_inlined() {
    let text = "let mut n = 1;\nn += 1;\nuse_it(n);\n";
    assert!(inline_variable(text, 5, "rs", None).is_err());
    assert!(inline_variable("let a = 1;\n", 5, "rs", None).is_err(), "never used");
}

#[test]
fn lines_are_extracted_into_a_function_with_their_parameters() {
    let text = "function main(items) {\n    let sum = 0;\n    for (const item of items) {\n        sum += item;\n    }\n    console.log(sum);\n}\n";
    let lines = starts(text);
    let selection = lines[2]..lines[5];
    let end = |_: usize| text.len();
    let change = extract_method(text, &selection, &[0], &lines, &end).unwrap();
    assert_eq!(
        applied(text, &change),
        "function extracted(items, sum) {\n    for (const item of items) {\n        sum += item;\n    }\n    return sum;\n}\n\nfunction main(items) {\n    let sum = 0;\n    sum = extracted(items, sum);\n    console.log(sum);\n}\n"
    );
}

#[test]
fn a_value_made_by_the_code_and_used_after_is_returned() {
    let text = "function main(a) {\n    const doubled = a * 2;\n    console.log(doubled);\n}\n";
    let lines = starts(text);
    let end = |_: usize| text.len();
    let change = extract_method(text, &(lines[1]..lines[2]), &[0], &lines, &end).unwrap();
    assert_eq!(
        applied(text, &change),
        "function extracted(a) {\n    const doubled = a * 2;\n    return doubled;\n}\n\nfunction main(a) {\n    const doubled = extracted(a);\n    console.log(doubled);\n}\n"
    );
}

#[test]
fn code_that_returns_or_makes_two_values_is_not_extracted() {
    let text = "function main(a) {\n    if (a) { return 1; }\n    const x = 1;\n    const y = 2;\n    use(x, y);\n}\n";
    let lines = starts(text);
    let end = |_: usize| text.len();
    assert!(extract_method(text, &(lines[1]..lines[2]), &[0], &lines, &end).is_err());
    assert!(extract_method(text, &(lines[2]..lines[4]), &[0], &lines, &end).is_err());
}

#[test]
fn an_await_makes_the_function_async() {
    let text = "async function main(url) {\n    const r = await fetch(url);\n    log(r);\n}\n";
    let lines = starts(text);
    let end = |_: usize| text.len();
    let change = extract_method(text, &(lines[1]..lines[2]), &[0], &lines, &end).unwrap();
    let after = applied(text, &change);
    assert!(after.starts_with("async function extracted(url) {"));
    assert!(after.contains("const r = await extracted(url);"));
}
