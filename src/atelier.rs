//! The `.atelier` folder of a project: what the IDE keeps in the project, apart from its files. It
//! has its own `.gitignore`: what is generated (sessions, the context graph) stays out of version
//! control, the way to version it being to remove its line there.

use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn dir(root: &Path) -> PathBuf {
    root.join(".atelier")
}

/// Makes sure `.atelier/.gitignore` lists `entry` (`graph/`), keeping the lines already there.
pub fn ensure_ignored(root: &Path, entry: &str) {
    let folder = dir(root);
    if fs::create_dir_all(&folder).is_err() {
        return;
    }
    let file = folder.join(".gitignore");
    let current = fs::read_to_string(&file).unwrap_or_default();
    if current.lines().any(|l| l.trim() == entry) {
        return;
    }
    let mut text = current;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(entry);
    text.push('\n');
    let _ = fs::write(file, text);
}

fn config_path(root: &Path) -> PathBuf {
    dir(root).join("config.json")
}

fn config(root: &Path) -> Value {
    fs::read(config_path(root))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

/// Whether the context graph is on for the project: None when the user was never asked (or
/// said "later"), Some(false) when they declined.
pub fn graph_setting(root: &Path) -> Option<bool> {
    config(root)["graph"]["enabled"].as_bool()
}

pub fn set_graph_setting(root: &Path, enabled: bool) {
    let mut config = config(root);
    config["graph"] = json!({"enabled": enabled});
    let _ = fs::create_dir_all(dir(root));
    if let Ok(json) = serde_json::to_vec_pretty(&config) {
        let _ = fs::write(config_path(root), json);
    }
}

#[cfg(test)]
mod tests {
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
}
