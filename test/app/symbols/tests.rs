use super::*;

#[test]
fn definitions_are_read_from_the_syntax() {
    let rust = defined(
        Path::new("a.rs"),
        "struct Counter;\nimpl Counter {\n    fn increment(&mut self) {}\n}\nfn main() {}\n",
    );
    assert!(
        rust.contains(&("Counter".into(), "struct".into(), 0)),
        "{rust:?}"
    );
    assert!(
        rust.iter()
            .any(|(n, _, l)| n.ends_with("increment") && *l == 2)
    );
    let java = defined(
        Path::new("A.java"),
        "package a;\n\npublic final class Shop {\n  record Item(int n) {}\n}\n@interface Marker {}\n",
    );
    assert_eq!(
        java,
        [
            ("Shop".to_string(), "class".to_string(), 2),
            ("Item".to_string(), "record".to_string(), 3),
            ("Marker".to_string(), "interface".to_string(), 5),
        ]
    );
    assert!(defined(Path::new("notes.md"), "# Title\n").is_empty());
    assert!(defined(Path::new("data.bin"), "x").is_empty());
}

#[test]
fn the_search_prefers_types_and_short_names() {
    let symbol = |name: &str, kind: &str| Symbol {
        name: name.into(),
        kind: kind.into(),
        path: PathBuf::from("a.rs"),
        line: 0,
    };
    let symbols = [
        symbol("render_settings_page", "function"),
        symbol("SettingsPage", "struct"),
        symbol("Ide::settings", "method"),
        symbol("Other", "struct"),
    ];
    let names: Vec<&str> = search(&symbols, "SetPa")
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names[0], "SettingsPage");
    assert!(!names.contains(&"Other"));
    let names: Vec<&str> = search(&symbols, "settings")
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names[0], "Ide::settings", "the exact last segment wins");
}

#[test]
fn the_index_reads_the_files_in_the_background() {
    let dir = std::env::temp_dir().join(format!("atelier-symbols-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("lib.rs");
    std::fs::write(&file, "pub fn alpha() {}\n").unwrap();
    let mut index = Index::default();
    let listed = vec![file.clone()];
    index.refresh(&dir, move || listed, || {});
    let start = Instant::now();
    while index.busy() && start.elapsed() < Duration::from_secs(10) {
        index.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(index.symbols.len(), 1);
    assert_eq!(index.symbols[0].name, "alpha");
    assert_eq!(index.symbols[0].path, file);
    let _ = std::fs::remove_dir_all(&dir);
}
