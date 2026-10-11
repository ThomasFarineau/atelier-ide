use super::*;

#[test]
fn the_gitignore_gets_each_entry_once_and_keeps_the_users_lines() {
    let root = std::env::temp_dir().join(format!("atelier-folder-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(dir(&root)).unwrap();
    fs::write(dir(&root).join(".gitignore"), "mes-notes/").unwrap();
    ensure_ignored(&root, "graph/");
    ensure_ignored(&root, "sessions/");
    ensure_ignored(&root, "graph/");
    assert_eq!(
        fs::read_to_string(dir(&root).join(".gitignore")).unwrap(),
        "mes-notes/\ngraph/\nsessions/\n"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn the_graph_setting_is_remembered_in_the_project_and_nothing_else_is_lost() {
    let root = std::env::temp_dir().join(format!("atelier-config-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    assert_eq!(graph_setting(&root), None, "never asked");
    fs::create_dir_all(dir(&root)).unwrap();
    fs::write(config_path(&root), r#"{"autre": 1}"#).unwrap();
    set_graph_setting(&root, true);
    assert_eq!(graph_setting(&root), Some(true));
    set_graph_setting(&root, false);
    assert_eq!(graph_setting(&root), Some(false));
    assert_eq!(config(&root)["autre"], 1);
    let _ = fs::remove_dir_all(&root);
}
