use super::*;

#[test]
fn the_edits_lead_to_the_new_text_and_touch_only_what_changed() {
    let old = "fn main(){\nlet x=1;\n    let y = 2;\n}\n";
    let new = "fn main() {\n    let x = 1;\n    let y = 2;\n}\n";
    let edits = edits_between(old, new);
    assert_eq!(apply(old, &edits), new);
    assert_eq!(edits.len(), 1, "the two first lines are one run: {edits:?}");
    assert_eq!(edits[0].range, 0..old.find("    let y").unwrap());
    assert!(edits_between(new, new).is_empty());
    let grown = "a\nb\n";
    let edits = edits_between(grown, "a\nx\nb\nc\n");
    assert_eq!(apply(grown, &edits), "a\nx\nb\nc\n");
}

#[test]
fn a_selection_keeps_only_its_lines() {
    let old = "a=1\nb=2\nc=3\n";
    let new = "a = 1\nb = 2\nc = 3\n";
    let edits: Vec<TextEdit> = old
        .lines()
        .zip(new.lines())
        .scan(0, |at, (o, n)| {
            let edit = TextEdit {
                range: *at..*at + o.len(),
                new_text: n.to_string(),
            };
            *at += o.len() + 1;
            Some(edit)
        })
        .collect();
    let middle = old.find("b=2").unwrap();
    let lines = lines_of(old, middle + 1..middle + 2);
    assert_eq!(lines, 1..2);
    let kept = within_lines(edits, old, &[lines]);
    assert_eq!(apply(old, &kept), "a=1\nb = 2\nc=3\n");
    assert_eq!(
        lines_of(old, 0..old.find("c=3").unwrap()),
        0..2,
        "the line after is not taken"
    );
}

#[test]
fn json_keeps_its_indentation_and_its_key_order() {
    assert_eq!(
        json("{\"b\":1,\"a\":[1,2]}\n").unwrap(),
        "{\n  \"b\": 1,\n  \"a\": [\n    1,\n    2\n  ]\n}\n"
    );
    assert_eq!(
        json("{\n    \"x\": {\"y\": true}}").unwrap(),
        "{\n    \"x\": {\n        \"y\": true\n    }\n}"
    );
    assert!(json("{oops").is_err());
}
