use super::*;

const SAMPLE: &str = "\
struct Point {
    x: i32,
    y: i32,
}

enum Shape {
    Circle(f32),
    Square { side: f32 },
}

trait Area {
    fn area(&self) -> f32;
}

impl Area for Shape {
    fn area(&self) -> f32 {
        let factor = 2.0;
        match self {
            Shape::Circle(r) => r * r * factor,
            Shape::Square { side } => side * side,
        }
    }
}

mod inner {
    pub const LIMIT: u32 = 3;
}

fn main() {
    let total = 1;
    println!(\"{}\", total);
}
";

#[test]
fn the_structure_lists_items_and_their_members() {
    let symbols = symbols(SAMPLE);
    let shown: Vec<(&str, usize)> = symbols.iter().map(|s| (s.name.as_str(), s.depth)).collect();
    assert_eq!(
        shown,
        [
            ("Point", 0),
            ("x", 1),
            ("y", 1),
            ("Shape", 0),
            ("Circle", 1),
            ("Square", 1),
            ("Area", 0),
            ("area", 1),
            ("Area for Shape", 0),
            ("area", 1),
            ("inner", 0),
            ("LIMIT", 1),
            ("main", 0),
        ]
    );
    let main = symbols.iter().find(|s| s.name == "main").unwrap();
    assert_eq!(main.kind, ItemKind::Function);
    assert_eq!(main.detail, "()");
    let area = symbols.iter().find(|s| s.name == "area").unwrap();
    assert_eq!(area.kind, ItemKind::Method);
    assert_eq!(area.detail, "(&self) -> f32");
}

#[test]
fn syntax_errors_are_found() {
    use crate::languages::Severity;
    assert!(syntax_errors(SAMPLE).is_empty());
    let broken = "fn main() {\n    let x = ;\n}\n";
    let errors = syntax_errors(broken);
    assert!(!errors.is_empty(), "{errors:?}");
    assert!(errors.iter().all(|e| e.severity == Severity::Error));
}

#[test]
fn a_name_goes_to_its_declaration() {
    let use_of_total = SAMPLE.rfind("total").unwrap();
    let found = definition(SAMPLE, use_of_total + 1);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].range_in(SAMPLE), {
        let start = SAMPLE.find("let total").unwrap() + 4;
        start..start + 5
    });
    let use_of_factor = SAMPLE.rfind("factor").unwrap();
    assert_eq!(definition(SAMPLE, use_of_factor).len(), 1);
    let shape = SAMPLE.find("Shape::Circle").unwrap();
    let found = definition(SAMPLE, shape);
    assert_eq!(found[0].range_in(SAMPLE), {
        let start = SAMPLE.find("enum Shape").unwrap() + 5;
        start..start + 5
    });
}

#[test]
fn occurrences_of_a_name_are_highlighted() {
    let at = SAMPLE.find("factor").unwrap();
    assert_eq!(highlights(SAMPLE, at).len(), 2);
    let at = SAMPLE.find("x: i32").unwrap();
    assert_eq!(
        highlights(SAMPLE, at).len(),
        1,
        "a field is not the variable x"
    );
}

#[test]
fn completion_offers_the_names_keywords_and_snippets() {
    let src = "fn helper(count: u32) {}\nfn main() {\n    let value = 1;\n    he\n}\n";
    let at = src.find("    he").unwrap() + 6;
    let completion = complete(src, at, false).unwrap();
    assert_eq!(completion.from, at - 2);
    let labels: Vec<&str> = completion.items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"helper"));
    assert!(labels.contains(&"value"));
    assert!(
        !labels.contains(&"count"),
        "a parameter of another function"
    );
    assert!(labels.contains(&"match"));
    assert!(labels.contains(&"println"));
    let helper = completion
        .items
        .iter()
        .find(|i| i.label == "helper")
        .unwrap();
    assert_eq!(helper.insert, "helper()");
    // After a dot, only the server knows the members.
    let dotted = "fn main() { let s = String::new(); s.le }";
    assert!(complete(dotted, dotted.find(".le").unwrap() + 3, true).is_none());
    let comment = "fn main() {\n    // he\n}";
    assert!(complete(comment, comment.find("he").unwrap() + 2, true).is_none());
}

#[test]
fn snippets_put_the_caret_where_the_name_goes() {
    for (trigger, template, key) in SNIPPETS {
        assert_eq!(template.matches("$0").count(), 1, "{trigger}");
        assert!(key.ends_with(trigger), "{trigger}");
    }
    assert_eq!(expand("fn $0() {}"), ("fn () {}".to_string(), 3));
    let src = "fn main() {\n    tm\n}";
    let at = src.find("tm").unwrap() + 2;
    let completion = complete(src, at, false).unwrap();
    let tmod = completion.items.iter().find(|i| i.label == "tmod").unwrap();
    let caret = tmod.caret.unwrap();
    assert_eq!(
        &tmod.insert[..caret],
        "#[cfg(test)]\nmod tests {\n    use super::*;\n\n    "
    );
}
