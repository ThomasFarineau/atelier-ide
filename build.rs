//! Collects every icon of the IDE and of its built-in plugins into one table (`icon_set.rs`).
//!
//! An icon is a name and up to three files: `name.svg` (all themes), `name.dark.svg` and
//! `name.light.svg` (that theme only, they win over the plain one). PNG works as well.
//!
//! Where the files are decides the key of the icon:
//! - `assets/icons/<name>`          → `file/<name>`  (file types)
//! - `assets/icons/ui/<name>`       → `<name>`       (interface)
//! - `assets/icons/extensions/<ext>` → `file/extensions/<ext>` (file types)
//! - `src/plugins/builtins/<path>`  → `<path>`       (a plugin's own icons, e.g. `git/icons/branch`
//!   or `msoffice/icon`)
//! - `src/languages/<path>`         → `languages/<path>` (a language's own icons)
//!
//! Interface and plugin icons can also be asked for by their last word (`branch`), unless it is
//! `icon`, which every plugin uses.
//!
//! File type icons go in an `extensions` folder, whose files are named after what they cover:
//! `<plugin>/extensions/docx.svg` is the icon of every `.docx` file, with no code. Several
//! extensions or whole file names can share one file by joining them with `+`
//! (`docx+docm.svg`, `package.json+.npmrc.svg`). The folders are `assets/icons/extensions` (file
//! types of no plugin or language), `src/languages/<language>/extensions` (always on) and
//! `src/plugins/builtins/<plugin>/extensions` (only while the plugin is on, which names the
//! folder with `BuiltinPlugin::extensions_dir`). A name or extension belongs to one folder only,
//! except between a plugin and the rest, where the plugin wins.
//!
//! It also gathers the translations (`locales.rs`): every `locales/<identifier>.json` of the
//! project, the one at the root for the IDE and the `locales` folder of each plugin or language
//! (`src/plugins/builtins/git/locales/fr_FR.json`).

use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Default)]
struct Entry {
    ext: String,
    /// default, light, dark
    files: [Option<PathBuf>; 3],
}

fn collect(
    dir: &Path,
    base: &Path,
    prefix: &str,
    recursive: bool,
    out: &mut BTreeMap<String, Entry>,
) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let Ok(read) = fs::read_dir(dir) else { return };
    let mut paths: Vec<_> = read.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            if recursive {
                collect(&path, base, prefix, recursive, out);
            }
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !matches!(ext, "svg" | "png") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let (name, variant) = match stem.rsplit_once('.') {
            Some((name, "light")) => (name, 1),
            Some((name, "dark")) => (name, 2),
            _ => (stem, 0),
        };
        let relative = path.parent().unwrap().strip_prefix(base).unwrap();
        let mut key = String::from(prefix);
        for part in relative.components() {
            key.push_str(&part.as_os_str().to_string_lossy());
            key.push('/');
        }
        key.push_str(name);
        let entry = out.entry(key).or_default();
        entry.ext = ext.to_string();
        entry.files[variant] = Some(path);
    }
}

/// Every `<identifier>.json` of the `locales` folders under `dir`.
fn collect_locales(dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let Ok(read) = fs::read_dir(dir) else { return };
    let mut paths: Vec<_> = read.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_locales(&path, out);
        } else if dir.file_name().is_some_and(|n| n == "locales")
            && path.extension().is_some_and(|e| e == "json")
        {
            let identifier = path.file_stem().unwrap().to_string_lossy().into_owned();
            out.push((identifier, path));
        }
    }
}

/// The name Windows shows for the program (Task Manager, taskbar jump list): the product, not the
/// file name.
#[cfg(windows)]
fn windows_identity() {
    let mut resource = winresource::WindowsResource::new();
    resource
        .set("FileDescription", "Atelier IDE")
        .set("ProductName", "Atelier IDE")
        .set("InternalName", "atelier-ide")
        .set("OriginalFilename", "atelier-ide.exe")
        .set_icon("assets/logo/atelier.ico");
    // Without the Windows SDK the program still builds, under its file name only.
    if let Err(e) = resource.compile() {
        println!("cargo:warning=nom du produit non intégré à l'exécutable : {e}");
    }
}

fn main() {
    #[cfg(windows)]
    windows_identity();
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let mut icons = BTreeMap::new();
    let files = root.join("assets/icons");
    collect(&files, &files, "file/", false, &mut icons);
    collect(
        &files.join("extensions"),
        &files,
        "file/",
        false,
        &mut icons,
    );
    let languages = root.join("src/languages");
    collect(&languages, &languages, "languages/", true, &mut icons);
    let ui = root.join("assets/icons/ui");
    collect(&ui, &ui, "", false, &mut icons);
    let plugins = root.join("src/plugins/builtins");
    collect(&plugins, &plugins, "", true, &mut icons);

    let mut shorts: BTreeMap<String, String> = BTreeMap::new();
    let mut table = String::from("pub static ICONS: &[Icon] = &[\n");
    for (key, entry) in &icons {
        let short = key.rsplit('/').next().unwrap();
        if !key.starts_with("file/")
            && short != "icon"
            && !key.contains("/extensions/")
            && let Some(other) = shorts.insert(short.to_string(), key.clone())
        {
            panic!("the icons {other} and {key} share the name {short}");
        }
        table.push_str(&format!("    Icon {{ key: {key:?}, variants: [\n"));
        for (index, file) in entry.files.iter().enumerate() {
            match file {
                Some(path) => {
                    let suffix = ["", ".light", ".dark"][index];
                    let uri = format!("bytes://icon/{key}{suffix}.{}", entry.ext);
                    let path = path.display().to_string().replace('\\', "/");
                    table.push_str(&format!(
                        "        Some(Variant {{ uri: {uri:?}, bytes: include_bytes!({path:?}) }}),\n"
                    ));
                }
                None => table.push_str("        None,\n"),
            }
        }
        table.push_str("    ] },\n");
    }
    table.push_str("];\n\npub static SHORT_NAMES: &[(&str, &str)] = &[\n");
    for (short, key) in &shorts {
        table.push_str(&format!("    ({short:?}, {key:?}),\n"));
    }
    table.push_str("];\n\npub static EXTENSIONS: &[(&str, &str, &str)] = &[\n");
    let mut extensions: BTreeMap<(String, String), String> = BTreeMap::new();
    for key in icons.keys() {
        let Some((dir, stem)) = key.split_once("/extensions/") else {
            continue;
        };
        // Languages and the assets share the owner `file`: they are not plugins.
        let dir = if dir == "file" || dir.starts_with("languages/") {
            "file"
        } else {
            dir
        };
        for ext in stem.split('+') {
            let ext = ext.to_ascii_lowercase();
            if let Some(other) = extensions.insert((dir.to_string(), ext.clone()), key.clone()) {
                panic!("the icons {other} and {key} both claim .{ext}");
            }
        }
    }
    for ((dir, ext), key) in &extensions {
        table.push_str(&format!("    ({dir:?}, {ext:?}, {key:?}),\n"));
    }
    table.push_str("];\n");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("icon_set.rs");
    fs::write(out, table).unwrap();

    let mut locales = Vec::new();
    collect_locales(&root.join("locales"), &mut locales);
    collect_locales(&root.join("src"), &mut locales);
    let mut table = String::from("pub static LOCALES: &[(&str, &str)] = &[\n");
    for (identifier, path) in &locales {
        let path = path.display().to_string().replace('\\', "/");
        table.push_str(&format!("    ({identifier:?}, include_str!({path:?})),\n"));
    }
    table.push_str("];\n");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("locales.rs");
    fs::write(out, table).unwrap();
}
