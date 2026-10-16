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

#[test]
fn the_output_of_tsc_is_read() {
    let output = "src/a.ts(12,5): error TS2322: Type 'string' is not assignable to type 'number'.\nsrc/b.ts(3,1): warning TS6133: 'x' is declared but never used.\nFound 2 errors.\n  continued line\n";
    let found = parse_tsc(output, Path::new("/p"));
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].path, Path::new("/p").join("src/a.ts"));
    assert_eq!(
        (found[0].line, found[0].column, found[0].error),
        (11, 4, true)
    );
    assert!(found[0].message.starts_with("TS2322"));
    assert!(!found[1].error);
}
