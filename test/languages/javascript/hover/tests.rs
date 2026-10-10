use super::*;

fn at(source: &str) -> (String, usize) {
    let offset = source.find('|').unwrap();
    (source.replacen('|', "", 1), offset)
}

#[test]
fn names_show_their_declaration_type_and_documentation() {
    let (text, offset) =
        at("/** Adds two numbers. */\nfunction add(a, b) { return a + b; }\nadd|(1, 2);\n");
    let info = hover(&text, offset - 1).unwrap();
    assert_eq!(info.title, "function add(a, b)");
    assert_eq!(info.body, "Adds two numbers.");

    let (text, offset) = at("const total = 5;\nconsole.log(tot|al);\n");
    let info = hover(&text, offset).unwrap();
    assert_eq!(info.title, "const total: number");
    assert_eq!(&text[info.range.clone()], "total");

    let (text, offset) = at("function f(items) { it|ems; }\n");
    assert_eq!(
        hover(&text, offset).unwrap().title,
        "(parameter) items: any"
    );

    let (text, offset) = at("class Dog extends Animal { bark() {} }\nnew Do|g();\n");
    assert_eq!(
        hover(&text, offset).unwrap().title,
        "class Dog extends Animal"
    );
}

#[test]
fn built_ins_and_members_are_documented() {
    let (text, offset) = at("setTime|out(() => {}, 10);\n");
    let info = hover(&text, offset).unwrap();
    assert!(info.title.starts_with("setTimeout: "), "{}", info.title);
    assert!(info.body.contains("délai"));

    let (text, offset) = at("const s = 'x';\ns.toUpper|Case();\n");
    let info = hover(&text, offset).unwrap();
    assert_eq!(info.title, "(method) toUpperCase(): string");
    assert!(!info.body.is_empty());

    let (text, offset) = at("Math.fl|oor(1.5);\n");
    assert!(
        hover(&text, offset)
            .unwrap()
            .title
            .starts_with("(method) floor")
    );
    let (text, offset) = at("let x = 1; // no|te\n");
    assert!(hover(&text, offset).is_none());
}
