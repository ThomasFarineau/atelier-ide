use super::*;

fn lang(p: &str) -> Option<Lang> {
    Lang::from_path(Path::new(p))
}

#[test]
fn detects_languages_by_extension() {
    assert_eq!(lang("a.ts"), Some(Lang::Ts));
    assert_eq!(lang("a.tsx"), Some(Lang::Tsx));
    assert_eq!(lang("a.mjs"), Some(Lang::Js));
    assert_eq!(lang("a.rs"), Some(Lang::Rust));
}

#[test]
fn ignores_declarations_and_unknown_files() {
    assert_eq!(lang("a.d.ts"), None);
    assert_eq!(lang("a.py"), None);
    assert_eq!(lang("Makefile"), None);
}

#[test]
fn names_for_reports() {
    assert_eq!(Lang::Tsx.name(), "typescript");
    assert_eq!(Lang::Js.name(), "javascript");
    assert_eq!(Lang::Rust.name(), "rust");
}
