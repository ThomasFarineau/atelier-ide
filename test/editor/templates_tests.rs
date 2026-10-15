use super::*;
use crate::languages::templates::parse;

#[test]
fn lines_are_surrounded_by_a_statement_at_their_indent() {
    let text = "fn f() {\n    one();\n    two();\n}\n";
    let range = 13..text.find("two").unwrap() + 6;
    let (whole, options) = surroundings("rs", text, &range, &[]);
    assert_eq!(&text[whole.clone()], "    one();\n    two();");
    let (_, new) = options.iter().find(|(t, _)| t == "if").expect("an if");
    assert_eq!(new, "    if condition {\n        one();\n        two();\n    }");
}

#[test]
fn a_part_of_a_line_gets_marks() {
    let text = "let s = name;";
    let start = text.find("name").unwrap();
    let range = start..start + 4;
    let (whole, options) = surroundings("txt", text, &range, &[]);
    assert_eq!(whole, range);
    let (_, new) = options.iter().find(|(t, _)| t == "( )").expect("parentheses");
    assert_eq!(new, "(name)");
    assert!(options.iter().all(|(t, _)| t != "if"), "no statements without braces");
}

#[test]
fn user_templates_surround_too() {
    let user = parse(r#"[{"description": "log", "text": "console.log($SELECTION$);"}]"#);
    let text = "    value";
    let (_, options) = surroundings("js", text, &(4..9), &user);
    assert!(options.contains(&("log".to_string(), "    console.log(    value);".to_string())));
}

#[test]
fn abbreviations_join_the_completion() {
    let user = parse(r#"[{"abbreviation": "fori", "text": "for (;;) {$END$}"}]"#);
    let mut state = EditorState::default();
    let text = "fo";
    state.sync(text);
    state.select(2, 2);
    // The global list is read from the data folder: test the pieces it is made of.
    let items: Vec<_> = user.iter().filter(|t| t.abbreviation.starts_with("fo")).collect();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].expand("").1, Some(10));
}

#[test]
fn the_expression_before_a_dot_is_found() {
    let at = |text: &str| expression_before(text, text.len() - 1).map(|r| text[r].to_string());
    assert_eq!(at("x = items.map(f).filter(g).").as_deref(), Some("items.map(f).filter(g)"));
    assert_eq!(at("return value.").as_deref(), Some("value"));
    assert_eq!(at("let s = \"abc\".").as_deref(), Some("\"abc\""));
    assert_eq!(at("foo(a, b.").as_deref(), Some("b"));
    assert_eq!(at("if (x.").as_deref(), Some("x"));
    assert_eq!(at("a + b.").as_deref(), Some("b"));
    assert_eq!(at("v[1][2].").as_deref(), Some("v[1][2]"));
    assert_eq!(at("std::mem::size.").as_deref(), Some("std::mem::size"));
    assert_eq!(at("x = .").as_deref(), None);
}

#[test]
fn a_postfix_template_replaces_the_expression_and_the_dot() {
    let text = "    compute(a).if";
    let mut state = EditorState::default();
    state.sync(text);
    state.select(text.len(), text.len());
    let completion = state.with_templates(text, None, "js", false).expect("templates");
    let item = completion.items.iter().find(|i| i.label == "if").expect("the if template");
    assert_eq!(item.insert, "if (compute(a)) {
    
}");
    // It starts where the expression does, before the dot; the caret goes inside the block.
    assert_eq!(item.start, Some(4));
    assert_eq!(item.caret, Some("if (compute(a)) {
    ".len()));
    // A variable takes a name from the expression.
    let var = completion.items.iter().find(|i| i.label == "var");
    assert!(var.is_none(), "`var` does not start with `if`");
    let text = "    order.getTotal().va";
    let mut state = EditorState::default();
    state.sync(text);
    state.select(text.len(), text.len());
    let completion = state.with_templates(text, None, "js", false).expect("templates");
    let var = completion.items.iter().find(|i| i.label == "var").expect("var");
    assert_eq!(var.insert, "const total = order.getTotal();");
}

#[test]
fn postfix_templates_follow_the_language() {
    let text = "value.pr";
    let mut state = EditorState::default();
    state.sync(text);
    state.select(text.len(), text.len());
    let rust = state.with_templates(text, None, "rs", false).expect("templates");
    assert!(rust.items.iter().any(|i| i.label == "println" && i.insert == "println!(\"{}\", value);"));
    let python = state.with_templates(text, None, "py", false).expect("templates");
    assert!(python.items.iter().any(|i| i.label == "print" && i.insert == "print(value)"));
    assert!(state.with_templates(text, None, "txt", false).is_none());
}

#[test]
fn python_lines_are_surrounded_by_an_indented_block() {
    let text = "x = 1
y = 2";
    let (whole, options) = surroundings("py", text, &(0..text.len()), &[]);
    assert_eq!(whole, 0..text.len());
    let find = |title: &str| options.iter().find(|(t, _)| t == title).map(|(_, n)| n.as_str());
    assert_eq!(find("if"), Some("if condition:
    x = 1
    y = 2"));
    assert_eq!(
        find("try / except"),
        Some("try:
    x = 1
    y = 2
except Exception:
    pass")
    );
}
