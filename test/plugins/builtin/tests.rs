use super::*;

/// Each plugin has its own `icon` files: they must not end up as one image.
#[test]
fn plugin_file_icons_have_distinct_uris() {
    let samples = [
        "a.md",
        "a.svg",
        "a.png",
        "a.csv",
        "a.docx",
        "a.xlsx",
        "a.pptx",
        "a.mmd",
        "kustomization.yaml",
        ".gitignore",
    ];
    let mut seen = std::collections::HashMap::new();
    for plugin in all() {
        for sample in samples {
            if let Some(key) = plugin.file_icon_for(Path::new(sample)) {
                let icon = crate::icon_set::source(key, true).unwrap_or_else(|| panic!("{key}"));
                let uri = icon.uri().unwrap_or_default().to_string();
                if let Some(other) = seen.insert(uri.clone(), plugin.id()) {
                    assert_eq!(other, plugin.id(), "{uri} is used by two plugins");
                }
            }
        }
    }
    assert!(seen.len() >= 7, "{seen:?}");
}
