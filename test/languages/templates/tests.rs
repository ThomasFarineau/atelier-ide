use super::*;

#[test]
fn templates_come_from_json() {
    let list = parse(
        r#"[
            {"abbreviation": "fori", "description": "for", "languages": ["js"], "text": "for (;;) {$END$}"},
            {"description": "try", "text": "try {\n$SELECTION$\n}"}
        ]"#,
    );
    assert_eq!(list.len(), 2);
    assert!(list[0].is_for("js") && !list[0].is_for("rs"));
    assert!(list[1].is_for("rs"), "no languages means all of them");
    assert!(!list[0].surrounds() && list[1].surrounds());
    assert_eq!(list[0].expand(""), ("for (;;) {}".to_string(), Some(10)));
    assert_eq!(list[1].expand("x();").0, "try {\nx();\n}");
}

#[test]
fn a_broken_file_gives_no_templates() {
    assert!(parse("not json").is_empty());
}
