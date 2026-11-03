//! YAML built-in plugin: the coloring and Reformat Code (Ctrl+Alt+L) of YAML and TOML files,
//! and the conversion of a YAML file to JSON from the project tree menu.

mod format;

pub use format::{toml, yaml, yaml_to_json};

use crate::languages::format::Formatter;
use crate::plugins::builtin::{BuiltinPlugin, Command, Host};
use std::path::{Path, PathBuf};

/// The syntax of TOML files, which syntect does not have.
pub const TOML_SYNTAX: &str = include_str!("toml.sublime-syntax");

pub fn is_yaml(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "yaml" | "yml"))
}

/// Writes the JSON of a YAML file next to it (`config.yml` gives `config.json`), never over a
/// file that is there.
pub fn convert_to_json(path: &Path) -> Result<PathBuf, String> {
    let target = path.with_extension("json");
    if target.exists() {
        let name = target.file_name().unwrap_or_default().to_string_lossy();
        return Err(trf!("yaml.json_exists", file = name));
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let json = yaml_to_json(text.strip_prefix('\u{feff}').unwrap_or(&text))?;
    std::fs::write(&target, json).map_err(|e| e.to_string())?;
    Ok(target)
}

#[derive(Default)]
pub struct YamlPlugin;

impl BuiltinPlugin for YamlPlugin {
    fn id(&self) -> &'static str {
        "yaml"
    }
    fn languages(&self) -> &'static [&'static str] {
        &["yaml", "toml"]
    }
    fn name(&self) -> &'static str {
        tr!("plugin.yaml.name")
    }
    fn description(&self) -> &'static str {
        tr!("plugin.yaml.summary")
    }
    fn icon(&self) -> &'static str {
        crate::plugins::builtin::card_icon("yaml")
    }
    fn formatters(&self) -> &'static [(&'static str, Formatter)] {
        &[("yaml", yaml), ("toml", toml)]
    }

    fn file_commands(&self, path: &Path) -> Vec<Command> {
        if !is_yaml(path) {
            return Vec::new();
        }
        vec![Command {
            id: "yaml.to_json",
            label: tr!("yaml.convert_to_json").into(),
        }]
    }

    fn run_file_command(&mut self, id: &str, path: &Path, host: &mut dyn Host) {
        if id != "yaml.to_json" {
            return;
        }
        match convert_to_json(path) {
            Ok(target) => {
                host.reload_files();
                host.open_file(target);
            }
            Err(error) => host.balloon(tr!("yaml.convert_to_json"), error, true),
        }
    }
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/yaml/mod/tests.rs"]
mod tests;
