use super::*;
#[test]
fn arguments_are_quoted_as_literals() {
    let cmd = shell_command(
        "python",
        &["a b.py".into(), "$(echo injected)".into(), "it's.py".into()],
    );
    assert!(cmd.contains("'a b.py'"));
    assert!(cmd.contains("'$(echo injected)'"));
    if cfg!(windows) {
        assert!(cmd.contains("'it''s.py'"));
    } else {
        assert!(cmd.contains("'it'\\''s.py'"));
    }
}

#[test]
fn manifests_are_files_or_folders_and_name_their_library_only() {
    let root = std::env::temp_dir().join(format!("atelier-manifests-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let plugins = root.join(".atelier/plugins");
    fs::create_dir_all(plugins.join("viewer")).unwrap();
    fs::create_dir_all(plugins.join("empty")).unwrap();
    fs::write(
        plugins.join("tools.json"),
        r#"{ "id": "tools", "name": "Tools", "version": "1" }"#,
    )
    .unwrap();
    fs::write(
        plugins.join("viewer/plugin.json"),
        r#"{ "id": "viewer", "name": "Viewer", "version": "1", "library": "viewer" }"#,
    )
    .unwrap();
    fs::write(
        plugins.join("escape.json"),
        r#"{ "id": "escape", "name": "Escape", "version": "1", "library": "../../evil" }"#,
    )
    .unwrap();
    let (loaded, errors) = load(&root);
    let ids: Vec<_> = loaded.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["tools", "viewer"]);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("escape.json"));
    assert!(loaded[0].library_path().is_none());
    assert_eq!(
        loaded[1].library_path().unwrap().parent().unwrap(),
        plugins.join("viewer")
    );
    let _ = fs::remove_dir_all(&root);
}
