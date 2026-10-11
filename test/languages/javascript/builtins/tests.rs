use super::*;

#[test]
fn tables_are_coherent() {
    for ty in [
        "String", "Array", "Number", "Promise", "Map", "Set", "Date", "RegExp", "Error", "URL",
    ] {
        assert!(!members(ty).is_empty(), "{ty}");
    }
    for name in [
        "Math", "JSON", "console", "process", "Object", "Array", "String", "Number", "Promise",
        "Date",
    ] {
        assert!(!statics(name).is_empty(), "{name}");
    }
    assert!(members("String").iter().any(|m| m.0 == "toUpperCase"));
    assert!(statics("console").iter().any(|m| m.0 == "log"));
    assert_eq!(global_type("Math"), Some("Math"));
    assert!(global_type("setTimeout").is_none());
    let items = global_items();
    assert!(
        items
            .iter()
            .any(|i| i.label == "setTimeout" && i.insert == "setTimeout()")
    );
    assert!(
        items
            .iter()
            .any(|i| i.label == "const" && i.kind == ItemKind::Keyword)
    );
}
