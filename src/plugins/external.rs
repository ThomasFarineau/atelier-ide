use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Plugin {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub commands: Vec<PluginCommand>,
    /// A native plugin: the name of its dynamic library, next to the manifest, without the
    /// prefix and extension of the system (`gltf_viewer` for `gltf_viewer.dll` on Windows and
    /// `libgltf_viewer.so` on Linux). It is loaded once the plugin is turned on (`native.rs`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<String>,
    /// The folder of the manifest.
    #[serde(skip)]
    pub dir: PathBuf,
    /// Its library could not be loaded: it is not tried again until the manifests are read
    /// again (`native.rs`).
    #[serde(skip)]
    pub load_failed: bool,
}

impl Plugin {
    /// The file of [`Plugin::library`] on this system.
    pub fn library_path(&self) -> Option<PathBuf> {
        let name = self.library.as_deref()?;
        Some(self.dir.join(format!(
            "{}{name}{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        )))
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginCommand {
    pub name: String,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

/// The manifests of `.atelier/plugins`: its `.json` files, and the `plugin.json` of its folders
/// (a plugin that comes with files, such as the library of a native plugin).
pub fn load(root: &Path) -> (Vec<Plugin>, Vec<String>) {
    let directory = root.join(".atelier/plugins");
    let mut plugins = Vec::new();
    let mut errors = Vec::new();
    let Ok(entries) = fs::read_dir(directory) else {
        return (plugins, errors);
    };
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        let path = if path.is_dir() {
            path.join("plugin.json")
        } else {
            path
        };
        if path.extension().and_then(|s| s.to_str()) != Some("json") || !path.is_file() {
            continue;
        }
        let result = (|| -> anyhow::Result<Plugin> {
            if fs::metadata(&path)?.len() > 128 * 1024 {
                anyhow::bail!(tr!("manifest_too_large"));
            }
            let mut plugin: Plugin = serde_json::from_str(&fs::read_to_string(&path)?)?;
            if plugin.id.is_empty()
                || plugin.name.is_empty()
                || plugin.commands.iter().any(|c| c.program.is_empty())
            {
                anyhow::bail!(tr!("empty_identifier_name_program"));
            }
            // A bare name: the library is the one next to the manifest, nowhere else.
            if plugin.library.as_deref().is_some_and(|name| {
                name.is_empty()
                    || !name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            }) {
                anyhow::bail!(tr!("invalid_library_name"));
            }
            plugin.dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            Ok(plugin)
        })();
        match result {
            Ok(p) if plugins.iter().any(|other: &Plugin| other.id == p.id) => {
                errors.push(trf!("duplicate_identifier", path = path.display()))
            }
            Ok(p) => plugins.push(p),
            Err(e) => errors.push(format!("{} : {e}", path.display())),
        }
    }
    (plugins, errors)
}

pub fn shell_command(program: &str, args: &[String]) -> String {
    let parts: Vec<_> = std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(shell_quote)
        .collect();
    format!(
        "{}{}",
        if cfg!(windows) { "& " } else { "" },
        parts.join(" ")
    )
}

/// The command typed in the terminal to run a program from a folder of the project: the
/// terminal starts at the root, so a folder below it goes first.
pub fn command_in(dir: &Path, root: &Path, program: &str, args: &[String]) -> String {
    let run = shell_command(program, args);
    if dir == root {
        return run;
    }
    let folder = dir.to_string_lossy();
    if cfg!(windows) {
        format!(
            "Set-Location -LiteralPath '{}'; {run}",
            folder.replace('\'', "''")
        )
    } else {
        format!("cd '{}' && {run}", folder.replace('\'', "'\\''"))
    }
}

fn shell_quote(value: &str) -> String {
    if cfg!(windows) {
        format!("'{}'", value.replace('\'', "''"))
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
#[path = "../../test/plugins/external/tests.rs"]
mod tests;
