use super::*;

#[test]
fn a_bundle_keeps_the_options_of_the_user_not_those_of_the_machine() {
    let settings = json!({
        "theme": 1,
        "font_size": 15.0,
        "fonts": {"interface": "Inter", "code": "Fira Code"},
        "recent": ["/a", "/b"],
        "mcp_choices": {"x|claude": true},
    });
    let bundle = bundle(&settings, "[{\"abbreviation\": \"x\", \"text\": \"y\"}]", "foo\n\nbar\n");
    assert_eq!(bundle["settings"]["theme"], 1);
    assert_eq!(bundle["settings"]["fonts"]["code"], "Fira Code");
    assert!(bundle["settings"].get("recent").is_none());
    assert!(bundle["settings"].get("mcp_choices").is_none());
    assert_eq!(bundle["templates"][0]["abbreviation"], "x");
    assert_eq!(bundle["dictionary"], json!(["foo", "bar"]));
}

#[test]
fn applying_a_bundle_changes_only_what_differs() {
    let bundle = json!({
        "atelier_settings": 1,
        "settings": {"theme": 2, "font_size": 14.0, "fonts": {"interface": "A", "code": "B"}, "recent": ["/x"]},
        "templates": [],
        "dictionary": ["mot"],
    });
    let mut settings = json!({"theme": 0, "font_size": 14.0, "recent": ["/mine"]});
    let imported = apply(&bundle, &mut settings).unwrap();
    // The theme and the fonts changed; the size was the same; the recent projects are not taken.
    assert_eq!(imported.changed, 2);
    assert_eq!(settings["theme"], 2);
    assert_eq!(settings["fonts"]["code"], "B");
    assert_eq!(settings["recent"], json!(["/mine"]));
    assert!(imported.templates.is_none(), "an empty list does not erase the templates");
    assert_eq!(imported.dictionary.as_deref(), Some("mot\n"));
}

#[test]
fn another_file_is_refused() {
    assert!(apply(&json!({"theme": 1}), &mut json!({})).is_err());
}
