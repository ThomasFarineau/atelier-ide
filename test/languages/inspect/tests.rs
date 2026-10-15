use super::*;

#[test]
fn source_files_are_known_by_their_extension() {
    assert_eq!(language_of(Path::new("a/B.java")), Some("java"));
    assert_eq!(language_of(Path::new("x.TSX")), Some("ts"));
    assert_eq!(language_of(Path::new("x.mjs")), Some("js"));
    assert_eq!(language_of(Path::new("notes.txt")), None);
}

#[test]
fn syntax_errors_are_found_in_each_language() {
    assert!(!syntax_errors(Path::new("a.rs"), "fn main() {\n").is_empty());
    assert!(syntax_errors(Path::new("a.rs"), "fn main() {}\n").is_empty());
    assert!(!syntax_errors(Path::new("a.js"), "let x = (1 + ;\n").is_empty());
    assert!(syntax_errors(Path::new("a.ts"), "let x: number = 1;\n").is_empty());
    assert!(!syntax_errors(Path::new("A.java"), "class A { void f( }\n").is_empty());
    assert!(syntax_errors(Path::new("a.txt"), "fn (((").is_empty());
}
