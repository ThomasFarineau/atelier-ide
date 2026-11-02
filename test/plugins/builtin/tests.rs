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

/// A sub-plugin names a parent that exists, and that is not a sub-plugin itself.
#[test]
fn sub_plugins_have_a_parent() {
    let plugins = all();
    let mut subs = 0;
    for plugin in &plugins {
        if let Some(parent) = plugin.parent() {
            subs += 1;
            let found = plugins.iter().find(|p| p.id() == parent);
            assert!(
                found.is_some_and(|p| p.parent().is_none()),
                "{}",
                plugin.id()
            );
        }
    }
    assert!(subs >= 1);
}

/// Every plugin of the Plugins page has a card, the front matter of its documentation
/// (`doc/en-US.md` and `doc/fr-FR.md`): its name, summary and icon come from it.
#[test]
fn plugins_have_a_card() {
    for plugin in all().iter().filter(|p| !p.core()) {
        // An unknown text shows as its key.
        assert!(!plugin.name().starts_with("plugin."), "{} has no card", plugin.id());
        assert!(!plugin.description().starts_with("plugin."), "{}", plugin.id());
        assert_ne!(plugin.icon(), "settings", "{} has no icon in its card", plugin.id());
    }
}

/// The icon a plugin names exists, and so does every JSON schema it brings.
#[test]
fn icons_and_schemas_of_plugins_exist() {
    for plugin in all() {
        let icon = plugin.icon();
        if icon.contains('/') {
            assert!(crate::icon_set::source(icon, true).is_some(), "{icon}");
        }
        for schema in plugin.json_schemas() {
            assert!(
                serde_json::from_str::<serde_json::Value>(schema.text).is_ok(),
                "{}",
                schema.name
            );
        }
    }
}
