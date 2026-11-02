use super::*;

#[test]
fn yaml_indentation_becomes_two_spaces_and_comments_stay() {
    let text = "# top\nserver:\n    port:   8080   \n    # the hosts\n    hosts:\n        - a\n        -   b\nname: x\n";
    assert_eq!(
        yaml(text).unwrap(),
        "# top\nserver:\n  port: 8080\n  # the hosts\n  hosts:\n    - a\n    - b\nname: x\n"
    );
}

#[test]
fn yaml_keeps_sequences_written_at_the_level_of_their_key() {
    let text = "steps:\n- name: build\n  run: cargo build\n- name: test\n";
    assert_eq!(yaml(text).unwrap(), text);
}

#[test]
fn yaml_items_whose_first_key_has_children() {
    let text = "-   a:\n        x: 1\n    b: 2\n";
    assert_eq!(yaml(text).unwrap(), "- a:\n    x: 1\n  b: 2\n");
}

#[test]
fn yaml_block_scalars_keep_their_text() {
    let text = "script:\n    - |\n        echo one\n          indented\n\n        echo two\nliteral: >-\n      folded\n      text\nnext: 1\n";
    assert_eq!(
        yaml(text).unwrap(),
        "script:\n  - |\n    echo one\n      indented\n\n    echo two\nliteral: >-\n  folded\n  text\nnext: 1\n"
    );
}

#[test]
fn yaml_documents_blank_lines_and_line_breaks() {
    let text = "a: 1\n\n\n\n\nb: 2\n---\nc:   3\n\n\n";
    assert_eq!(yaml(text).unwrap(), "a: 1\n\n\nb: 2\n---\nc: 3\n");
    assert_eq!(yaml("a:\r\n    b: 1\r\n").unwrap(), "a:\r\n  b: 1\r\n");
}

#[test]
fn yaml_leaves_quotes_flow_and_urls_alone() {
    let text = "\"a: b\":   'c:  d'\nlist: [1,  2]\nurl: http://x:80/  # note\n";
    assert_eq!(
        yaml(text).unwrap(),
        "\"a: b\": 'c:  d'\nlist: [1,  2]\nurl: http://x:80/  # note\n"
    );
}

#[test]
fn yaml_with_a_syntax_error_is_not_formatted() {
    assert!(yaml("a: [1, 2\nb: 3\n").is_err());
}

#[test]
fn toml_spaces_around_equals_headers_and_arrays() {
    let text = "# top\n[ package ]  # pkg\nname=\"x\"   # the name\n  version  =   \"1\"\narr = [\n1,\n    2,\n]\n\n\n\n[[bin]]\n\"a=b\" = 1\n";
    assert_eq!(
        toml(text).unwrap(),
        "# top\n[package] # pkg\nname = \"x\"   # the name\nversion = \"1\"\narr = [\n  1,\n  2,\n]\n\n\n[[bin]]\n\"a=b\" = 1\n"
    );
}

#[test]
fn toml_multi_line_strings_keep_their_text() {
    let text = "a = \"\"\"keep   \n   this  = as is\n\"\"\"\nb=1\n";
    assert_eq!(
        toml(text).unwrap(),
        "a = \"\"\"keep   \n   this  = as is\n\"\"\"\nb = 1\n"
    );
}

#[test]
fn toml_with_a_syntax_error_is_not_formatted() {
    assert!(toml("a = = 1\n").is_err());
}

#[test]
fn yaml_to_json_keeps_the_order_and_the_types() {
    let text = "name: app\nport: 8080\nratio: 0.5\non: true\nnothing: ~\nlist:\n  - a\n  - 2\n";
    assert_eq!(
        yaml_to_json(text).unwrap(),
        "{\n  \"name\": \"app\",\n  \"port\": 8080,\n  \"ratio\": 0.5,\n  \"on\": true,\n  \"nothing\": null,\n  \"list\": [\n    \"a\",\n    2\n  ]\n}\n"
    );
}

#[test]
fn yaml_to_json_resolves_anchors_and_merge_keys() {
    let text = "base: &base\n  a: 1\n  b: 2\nchild:\n  <<: *base\n  b: 3\n";
    let json: Value = serde_json::from_str(&yaml_to_json(text).unwrap()).unwrap();
    assert_eq!(json["child"], serde_json::json!({ "b": 3, "a": 1 }));
}

#[test]
fn yaml_to_json_of_several_documents_is_an_array() {
    assert_eq!(
        yaml_to_json("a: 1\n---\nb: 2\n").unwrap(),
        "[\n  {\n    \"a\": 1\n  },\n  {\n    \"b\": 2\n  }\n]\n"
    );
    assert_eq!(
        yaml_to_json("1: x\ntrue: y\n").unwrap(),
        "{\n  \"1\": \"x\",\n  \"true\": \"y\"\n}\n"
    );
    assert!(yaml_to_json("a: *missing\n").is_err());
}
