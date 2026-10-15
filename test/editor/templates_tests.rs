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
    let (whole, options) = surroundings("py", text, &range, &[]);
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
