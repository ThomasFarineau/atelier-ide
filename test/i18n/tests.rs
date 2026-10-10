use super::*;
use std::collections::BTreeSet;

fn placeholders(text: &str) -> BTreeSet<String> {
    let text = text.replace("{{", "").replace("}}", "");
    text.split('{')
        .skip(1)
        .filter_map(|s| s.split_once('}').map(|(name, _)| name.to_string()))
        .collect()
}

#[test]
fn values_fill_their_place() {
    let n = 3;
    assert_eq!(fill("{n} files, {{ok}}", &[("n", &n)]), "3 files, {ok}");
    assert_eq!(fill("{missing} }", &[]), "{missing} }");
}

/// Every language has every key, with the same values in it.
#[test]
fn languages_have_the_same_keys() {
    let default = catalog(DEFAULT).expect("the default language has files");
    assert!(identifiers().contains(&"fr_FR"));
    for catalog in CATALOGS.iter() {
        for (key, text) in &default.texts {
            let other = catalog
                .texts
                .get(key)
                .unwrap_or_else(|| panic!("{} lacks {key}", catalog.identifier));
            assert_eq!(
                placeholders(text),
                placeholders(other),
                "{key} in {}",
                catalog.identifier
            );
        }
        for key in catalog.texts.keys() {
            assert!(default.texts.contains_key(key), "{DEFAULT} lacks {key}");
        }
    }
}

/// Every `tr!` and `trf!` of the code names a key the languages have.
#[test]
fn the_code_uses_known_keys() {
    fn sources(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                sources(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    sources(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let default = catalog(DEFAULT).unwrap();
    let mut unknown = Vec::new();
    for file in files.iter().filter(|f| !f.ends_with("i18n.rs")) {
        let text = std::fs::read_to_string(file).unwrap();
        for start in ["tr!(\"", "trf!(\""] {
            for (at, _) in text.match_indices(start) {
                let key = &text[at + start.len()..];
                let key = &key[..key.find('"').unwrap()];
                if !default.texts.contains_key(key) {
                    unknown.push(format!("{}: {key}", file.display()));
                }
            }
        }
    }
    assert!(unknown.is_empty(), "{unknown:#?}");
}

#[test]
fn unknown_keys_show_as_they_are() {
    assert_eq!(t("no.such.key"), "no.such.key");
    assert_eq!(name("fr_FR"), "Français");
    assert_eq!(name("en_US"), "English");
    assert_eq!(error_message("Erreur : x"), Some("x"));
    assert_eq!(error_message("Error: x"), Some("x"));
}

#[test]
fn shortcuts_name_the_keys_in_the_language() {
    assert_eq!(shortcut("Ctrl+Shift+F"), "Ctrl+Maj+F");
    assert_eq!(shortcut("Alt+Enter"), "Alt+Entrée");
    assert_eq!(shortcut("F5"), "F5");
}
