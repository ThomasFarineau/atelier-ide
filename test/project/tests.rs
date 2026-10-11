use super::*;
#[test]
fn rename_move_create_and_delete_stay_in_the_project() {
    let base = std::env::temp_dir().join(format!("atelier-ops-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(base.join("a")).unwrap();
    fs::write(base.join("a/f.txt"), "x").unwrap();
    let renamed = rename_path(&base.join("a/f.txt"), "g.txt").unwrap();
    assert!(renamed.is_file() && !base.join("a/f.txt").exists());
    assert!(rename_path(&renamed, "../x").is_err());
    assert!(rename_path(&renamed, "").is_err());
    let dir = create_dir(&base, "b/c").unwrap();
    assert!(dir.is_dir());
    let moved = move_into(&renamed, &dir).unwrap();
    assert_eq!(moved, base.join("b/c/g.txt"));
    assert!(
        move_into(&base.join("b"), &dir).is_err(),
        "a folder cannot enter itself"
    );
    fs::write(base.join("a/g.txt"), "y").unwrap();
    assert!(
        move_into(&base.join("a/g.txt"), &dir).is_err(),
        "no overwrite"
    );
    delete_path(&base.join("b")).unwrap();
    assert!(!base.join("b").exists());
    fs::remove_dir_all(&base).unwrap();
}
#[test]
fn language_extensions() {
    for (name, expected) in [
        ("index.HTML", "html"),
        ("a.css", "css"),
        ("a.mjs", "js"),
        ("a.tsx", "ts"),
        ("Main.java", "java"),
        ("a.py", "py"),
        ("lib.rs", "rs"),
    ] {
        assert_eq!(language(Path::new(name)), expected);
    }
}
#[test]
fn refuses_parent_and_absolute_paths() {
    assert!(new_file_path(Path::new("."), "../escape.rs").is_err());
    assert!(new_file_path(Path::new("."), "/escape.rs").is_err());
    assert!(new_file_path(Path::new("."), "").is_err());
}
#[test]
fn refuses_to_overwrite_external_changes() {
    let path = std::env::temp_dir().join(format!("atelier-conflict-{}.rs", std::process::id()));
    fs::write(&path, "initial").unwrap();
    let mut doc = Document::open(path.clone()).unwrap();
    doc.text = "editor".into();
    fs::write(&path, "external").unwrap();
    assert!(doc.save(path.clone(), false).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "external");
    fs::remove_file(path).unwrap();
}
#[test]
fn external_change_is_detected_by_modification_time() {
    let path = std::env::temp_dir().join(format!("atelier-mtime-{}.rs", std::process::id()));
    fs::write(&path, "v1").unwrap();
    let mut doc = Document::open(path.clone()).unwrap();
    assert!(!doc.changed_on_disk());
    std::thread::sleep(std::time::Duration::from_millis(50));
    fs::write(&path, "v2 plus long").unwrap();
    assert!(doc.changed_on_disk());
    assert!(doc.reload().unwrap());
    assert!(!doc.changed_on_disk());
    fs::remove_file(path).unwrap();
}
#[test]
fn reload_follows_disk_only_when_unmodified() {
    let path = std::env::temp_dir().join(format!("atelier-reload-{}.rs", std::process::id()));
    fs::write(&path, "v1").unwrap();
    let mut doc = Document::open(path.clone()).unwrap();
    fs::write(&path, "v2").unwrap();
    assert!(doc.reload().unwrap());
    assert_eq!(doc.text, "v2");
    assert!(!doc.dirty());
    doc.text = "edited".into();
    fs::write(&path, "v3").unwrap();
    assert!(!doc.reload().unwrap());
    assert_eq!(doc.text, "edited");
    fs::remove_file(path).unwrap();
}
