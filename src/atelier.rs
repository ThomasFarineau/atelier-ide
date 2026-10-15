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
    set_setting(root, "graph", json!({"enabled": enabled}));
}

/// A section of `.atelier/config.json` (`runtimes`), `Null` when it is not there.
pub fn setting(root: &Path, key: &str) -> Value {
    config(root)[key].clone()
}

/// Replaces a section of `.atelier/config.json`, keeping the others.
pub fn set_setting(root: &Path, key: &str, value: Value) {
    let mut config = config(root);
    config[key] = value;
    let _ = fs::create_dir_all(dir(root));
    if let Ok(json) = serde_json::to_vec_pretty(&config) {
        let _ = fs::write(config_path(root), json);
    }
}

#[cfg(test)]
#[path = "../test/atelier/tests.rs"]
mod tests;
