use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

/// The extension of the file of a plugin to install: a zip of its manifest and its files.
pub const PACKAGE_EXTENSION: &str = "atelier-plugin";

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
    /// A native plugin: the name of its dynamic library, without the prefix and extension of the
    /// system (`gltf_viewer` for `gltf_viewer.dll` on Windows and `libgltf_viewer.so` on Linux),
    /// in the folder of this system (`windows-x86_64/`, see [`platform`]) or next to the
    /// manifest. It is loaded once the plugin is turned on (`native.rs`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<String>,
    /// The version of Atelier a native plugin was built for: it loads in that version only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atelier: Option<String>,
    /// The folder of the manifest.
    #[serde(skip)]
    pub dir: PathBuf,
    /// Its library could not be loaded: it is not tried again until the manifests are read
    /// again (`native.rs`).
    #[serde(skip)]
    pub load_failed: bool,
    /// Installed in the IDE ([`installed_dir`]), for every project, rather than in the
    /// `.atelier/plugins` of the project.
    #[serde(skip)]
    pub installed: bool,
}

impl Plugin {
    /// The file of [`Plugin::library`] on this system: in the folder of the system, else next to
    /// the manifest.
    pub fn library_path(&self) -> Option<PathBuf> {
        let file = library_file(self.library.as_deref()?);
        let own = self.dir.join(platform()).join(&file);
        Some(if own.is_file() {
            own
        } else {
            self.dir.join(file)
        })
    }
}

/// `windows-x86_64`, `linux-x86_64`, `macos-aarch64`…: the folder of the libraries of this system
/// in a plugin, as the releases name their files.
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

fn library_file(name: &str) -> String {
    format!(
        "{}{name}{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    )
}

/// Where the plugins installed in the IDE are, one folder each: in its data folder.
pub fn installed_dir() -> PathBuf {
    crate::cli::data_dir().join("plugins")
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginCommand {
    pub name: String,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

/// The manifests of the plugins: those of the project (`.atelier/plugins`: its `.json` files, and
/// the `plugin.json` of its folders, for a plugin that comes with files) and those installed in
/// the IDE ([`installed_dir`]).
pub fn load(root: &Path) -> (Vec<Plugin>, Vec<String>) {
    load_with(root, &installed_dir())
}

pub fn load_with(root: &Path, installed: &Path) -> (Vec<Plugin>, Vec<String>) {
    let mut plugins = Vec::new();
    let mut errors = Vec::new();
    scan(
        &root.join(".atelier/plugins"),
        false,
        &mut plugins,
        &mut errors,
    );
    finish_pending(installed);
    scan(installed, true, &mut plugins, &mut errors);
    (plugins, errors)
}

fn scan(directory: &Path, installed: bool, plugins: &mut Vec<Plugin>, errors: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        let path = if path.is_dir() {
            // An installation or a removal that waits for the next start.
            if path.extension().is_some() || path.join(UNINSTALL).exists() {
                continue;
            }
            path.join("plugin.json")
        } else if installed {
            continue;
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
            let mut plugin = parse(&fs::read_to_string(&path)?)?;
            plugin.dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            plugin.installed = installed;
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
}

/// A manifest, checked.
fn parse(json: &str) -> anyhow::Result<Plugin> {
    let plugin: Plugin = serde_json::from_str(json)?;
    if plugin.id.is_empty()
        || plugin.name.is_empty()
        || plugin.commands.iter().any(|c| c.program.is_empty())
    {
        anyhow::bail!(tr!("empty_identifier_name_program"));
    }
    // Bare names: the folder of the plugin, and its library in it, nowhere else.
    let bare = |name: &str| {
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    if !bare(&plugin.id) || plugin.library.as_deref().is_some_and(|name| !bare(name)) {
        anyhow::bail!(tr!("invalid_library_name"));
    }
    Ok(plugin)
}

/// The file left in the folder of a plugin uninstalled while its library was loaded (Windows
/// keeps it locked): the folder goes at the next start.
const UNINSTALL: &str = ".uninstall";

/// What [`install`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum Installed {
    /// Ready: the plugin of that name can be turned on.
    Now(String),
    /// Another version of the plugin is loaded: the new one replaces it at the next start.
    AtNextStart(String),
}

/// Installs a plugin in the IDE from its file (`.atelier-plugin`): a zip with its `plugin.json`
/// at the root and its files, such as the library of each system in `windows-x86_64/`…
pub fn install(file: &Path) -> Result<Installed, String> {
    install_into(file, &installed_dir())
}

pub fn install_into(file: &Path, installed: &Path) -> Result<Installed, String> {
    let invalid = |e: &dyn std::fmt::Display| trf!("invalid_plugin_file", e = e);
    let reader = fs::File::open(file).map_err(|e| invalid(&e))?;
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| invalid(&e))?;
    let mut json = String::new();
    zip.by_name("plugin.json")
        .map_err(|e| invalid(&e))?
        .take(128 * 1024)
        .read_to_string(&mut json)
        .map_err(|e| invalid(&e))?;
    let plugin = parse(&json).map_err(|e| invalid(&e))?;
    // A native plugin must have a library for this system.
    if let Some(library) = &plugin.library {
        let file = library_file(library);
        let here = [format!("{}/{file}", platform()), file];
        if !here.iter().any(|name| zip.by_name(name).is_ok()) {
            return Err(trf!("plugin_missing_platform", platform = platform()));
        }
    }
    let target = installed.join(&plugin.id);
    let staging = installed.join(format!("{}.new", plugin.id));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| invalid(&e))?;
        // Nothing outside the folder of the plugin.
        let Some(name) = entry.enclosed_name() else {
            return Err(invalid(&entry.name().to_string()));
        };
        let out = staging.join(name);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(|e| invalid(&e))?;
        fs::write(&out, bytes).map_err(|e| e.to_string())?;
    }
    if !target.exists() || fs::remove_dir_all(&target).is_ok() {
        fs::rename(&staging, &target).map_err(|e| e.to_string())?;
        return Ok(Installed::Now(plugin.name));
    }
    // The loaded library cannot be replaced (Windows): `<id>.new` waits for the next start.
    let _ = fs::remove_file(target.join(UNINSTALL));
    Ok(Installed::AtNextStart(plugin.name))
}

/// Uninstalls a plugin of the IDE: false when its library is loaded, and the folder goes only at
/// the next start.
pub fn uninstall(id: &str) -> Result<bool, String> {
    uninstall_from(id, &installed_dir())
}

pub fn uninstall_from(id: &str, installed: &Path) -> Result<bool, String> {
    let dir = installed.join(id);
    let _ = fs::remove_dir_all(installed.join(format!("{id}.new")));
    if fs::remove_dir_all(&dir).is_ok() || !dir.exists() {
        return Ok(true);
    }
    fs::write(dir.join(UNINSTALL), b"").map_err(|e| e.to_string())?;
    Ok(false)
}

/// The installations and removals left for the start of the IDE, when no library is loaded yet.
fn finish_pending(installed: &Path) {
    let Ok(entries) = fs::read_dir(installed) else {
        return;
    };
    for path in entries.flatten().map(|e| e.path()) {
        if path.join(UNINSTALL).exists() {
            let _ = fs::remove_dir_all(&path);
        } else if path.extension().is_some_and(|e| e == "new") {
            let target = path.with_extension("");
            if !target.exists() || fs::remove_dir_all(&target).is_ok() {
                let _ = fs::rename(&path, &target);
            }
        }
    }
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
