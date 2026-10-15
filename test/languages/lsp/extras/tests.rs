use super::*;
use serde_json::json;

#[test]
fn signature_marks_the_active_parameter() {
    let result = json!({
        "signatures": [{
            "label": "fn add(a: i32, b: i32) -> i32",
            "parameters": [{"label": [7, 13]}, {"label": "b: i32"}],
            "documentation": {"kind": "markdown", "value": "Adds."},
        }],
        "activeSignature": 0,
        "activeParameter": 1,
    });
    let signature = signature_from_server(&result).expect("a signature");
    assert_eq!(signature.parameters, vec![7..13, 15..21]);
    assert_eq!(signature.active, Some(1));
    assert_eq!(signature.doc, "Adds.");
    assert_eq!(signature.count, 1);
}

#[test]
fn a_parameter_after_the_last_has_no_active_mark() {
    let result = json!({
        "signatures": [{"label": "f(a)", "parameters": [{"label": "a"}]}],
        "activeParameter": 3,
    });
    let signature = signature_from_server(&result).expect("a signature");
    assert_eq!(signature.active, None);
}

#[test]
fn hints_come_sorted_with_their_label() {
    let text = "let a = 1;\nlet b = f(2);";
    let result = json!([
        {"position": {"line": 1, "character": 10}, "label": [{"value": "x"}, {"value": ":"}], "kind": 2},
        {"position": {"line": 0, "character": 5}, "label": ": i32", "kind": 1},
        {"position": {"line": 0, "character": 0}, "label": " ", "kind": 1},
    ]);
    let hints = hints_from_server(&result, text);
    assert_eq!(hints.len(), 2, "an empty hint is left out");
    assert_eq!(hints[0].label, ": i32");
    assert_eq!(hints[0].offset, 5);
    assert_eq!(hints[0].kind, HintKind::Type);
    assert_eq!(hints[1].label, "x:");
    assert_eq!(hints[1].kind, HintKind::Parameter);
}

#[test]
fn hierarchy_items_know_their_place() {
    let raw = json!({
        "name": "Shape", "kind": 5, "uri": "file:///tmp/Shape.java",
        "range": {"start": {"line": 3, "character": 0}, "end": {"line": 9, "character": 1}},
        "selectionRange": {"start": {"line": 3, "character": 13}, "end": {"line": 3, "character": 18}},
    });
    let item = item_from_server(&raw).expect("an item");
    assert_eq!(item.name, "Shape");
    assert_eq!((item.line, item.column), (3, 13));
}
