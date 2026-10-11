use super::*;

fn labels(completion: &Completion) -> Vec<&str> {
    completion.items.iter().map(|i| i.label.as_str()).collect()
}

/// The text before `|` is the document, `|` the caret.
fn at(source: &str) -> (String, usize) {
    let offset = source.find('|').unwrap();
    (source.replacen('|', "", 1), offset)
}

#[test]
fn names_in_scope_then_globals_and_keywords() {
    let (text, offset) = at("const alpha = 1;\nfunction beta(gamma) {\n  al|\n}\n");
    let completion = complete(&text, offset, false, None).unwrap();
    assert_eq!(completion.from, offset - 2);
    let names = labels(&completion);
    for expected in ["alpha", "beta", "gamma", "console", "const", "setTimeout"] {
        assert!(names.contains(&expected), "{expected} in {names:?}");
    }
    let alpha = completion
        .items
        .iter()
        .find(|i| i.label == "alpha")
        .unwrap();
    assert_eq!(alpha.detail.as_deref(), Some("number"));
    assert!(
        completion
            .items
            .iter()
            .find(|i| i.label == "setTimeout")
            .unwrap()
            .insert
            .ends_with("()")
    );
}

#[test]
fn nothing_is_offered_before_typing_or_when_declaring_a_name() {
    let (text, offset) = at("let a = 1;\n|");
    assert!(complete(&text, offset, false, None).is_none());
    assert!(
        complete(&text, offset, true, None).is_some(),
        "explicit request"
    );
    let (text, offset) = at("const na|");
    assert!(complete(&text, offset, false, None).is_none());
    let (text, offset) = at("// a comment wi|th words\n");
    assert!(complete(&text, offset, true, None).is_none());
    let (text, offset) = at("const s = 'hel|lo';\n");
    assert!(complete(&text, offset, true, None).is_none());
}

#[test]
fn members_follow_the_type_before_the_dot() {
    let (text, offset) = at("const name = 'x';\nname.|\n");
    let completion = complete(&text, offset, false, None).unwrap();
    assert_eq!(completion.from, offset);
    let names = labels(&completion);
    assert!(
        names.contains(&"toUpperCase") && names.contains(&"length"),
        "{names:?}"
    );
    let upper = completion
        .items
        .iter()
        .find(|i| i.label == "toUpperCase")
        .unwrap();
    assert_eq!(upper.insert, "toUpperCase()");
    assert_eq!(
        upper.caret,
        Some("toUpperCase".len() + 2),
        "no parameters: caret after the parentheses"
    );
    let split = completion
        .items
        .iter()
        .find(|i| i.label == "split")
        .unwrap();
    assert_eq!(
        split.caret,
        Some("split".len() + 1),
        "caret inside the parentheses"
    );

    let (text, offset) = at("console.lo|\n");
    let completion = complete(&text, offset, false, None).unwrap();
    assert_eq!(completion.from, offset - 2);
    assert!(labels(&completion).contains(&"log"));

    let (text, offset) = at("const o = { first: 1, second() {} };\nfunction f() { o.| }\n");
    let names: Vec<String> = complete(&text, offset, false, None)
        .unwrap()
        .items
        .into_iter()
        .map(|i| i.label)
        .collect();
    assert!(
        names.contains(&"first".to_string()) && names.contains(&"second".to_string()),
        "{names:?}"
    );

    let (text, offset) = at("class A { go() {} constructor() { this.x = 1; } run() { this.| } }\n");
    let names: Vec<String> = complete(&text, offset, false, None)
        .unwrap()
        .items
        .into_iter()
        .map(|i| i.label)
        .collect();
    assert!(
        names.contains(&"go".to_string()) && names.contains(&"x".to_string()),
        "{names:?}"
    );

    let (text, offset) = at("'a,b'.split(',').|\n");
    assert!(labels(&complete(&text, offset, false, None).unwrap()).contains(&"map"));
}

#[test]
fn unknown_values_fall_back_to_the_words_of_the_file() {
    let (text, offset) = at("const data = load();\nconst userName = data.userName;\ndata.|\n");
    let completion = complete(&text, offset, false, None).unwrap();
    assert!(labels(&completion).contains(&"userName"));
}

#[test]
fn module_strings_complete_paths_and_packages() {
    let dir = std::env::temp_dir().join(format!("atelier-js-paths-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::create_dir_all(dir.join("node_modules/left-pad")).unwrap();
    std::fs::write(dir.join("util.js"), "").unwrap();
    std::fs::write(dir.join("notes.txt"), "").unwrap();
    std::fs::write(
        dir.join("package.json"),
        r#"{"dependencies":{"express":"4"}}"#,
    )
    .unwrap();
    let file = dir.join("main.js");

    let (text, offset) = at("import x from './u|';\n");
    let completion = complete(&text, offset, true, Some(&file)).unwrap();
    assert!(completion.items.is_empty() || completion.from <= offset);
    let (text, offset) = at("import x from './|';\n");
    let completion = complete(&text, offset, true, Some(&file)).unwrap();
    let names = labels(&completion);
    assert!(
        names.contains(&"util.js") && names.contains(&"lib/"),
        "{names:?}"
    );
    assert!(!names.contains(&"notes.txt"), "only modules: {names:?}");
    assert_eq!(completion.from, offset);

    let (text, offset) = at("const a = require('|');\n");
    let completion = complete(&text, offset, true, Some(&file)).unwrap();
    let names = labels(&completion);
    assert!(
        names.contains(&"left-pad") && names.contains(&"express") && names.contains(&"fs"),
        "{names:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
