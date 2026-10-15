use super::*;

#[test]
fn a_problem_knows_its_line_and_column() {
    let text = "fn a() {}\nlet é = (;\n";
    let at = text.find(';').unwrap();
    let diagnostic = Diagnostic {
        range: at..at + 1,
        severity: Severity::Error,
        message: "x".into(),
        source: "syntaxe".into(),
    };
    let problem = problem_of(Path::new("/p/a.rs"), text, &diagnostic);
    assert_eq!((problem.line, problem.column, problem.length), (1, 9, 1));
    assert_eq!(problem.severity, 1);
}

#[test]
fn a_file_is_read_from_the_disk() {
    let dir = std::env::temp_dir().join(format!("atelier-inspect-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let broken = dir.join("broken.rs");
    fs::write(&broken, "fn f( {\n").unwrap();
    let fine = dir.join("fine.rs");
    fs::write(&fine, "fn f() {}\n").unwrap();
    assert!(!inspect_file(&broken).is_empty());
    assert!(inspect_file(&fine).is_empty());
    assert!(inspect_file(&dir.join("missing.rs")).is_empty());
    let _ = fs::remove_dir_all(dir);
}
