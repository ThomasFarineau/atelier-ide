//! Finding, installing and driving `rust-analyzer`, the language server of Rust. It comes with
//! rustup (`rustup component add rust-analyzer`); a `rust-analyzer` on the PATH works as well.

use crate::languages::lsp::{
    Launch,
    provider::{Control, with_control},
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    thread,
};

/// What the commands of the plugin ask of the provider, and what the provider tells back.
pub static CONTROL: Mutex<Control> = Mutex::new(Control::new());

fn control<R>(f: impl FnOnce(&mut Control) -> R) -> R {
    with_control(&CONTROL, f)
}

fn quiet(program: &str) -> Command {
    let mut command = Command::new(program);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

fn executable(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// The real `rust-analyzer`: the one of the rustup toolchain, else one on the PATH that is not
/// the rustup proxy (which fails when the component is missing).
pub fn find_server() -> Option<PathBuf> {
    let from_rustup = quiet("rustup")
        .args(["which", "rust-analyzer"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
        .filter(|p| p.is_file());
    from_rustup.or_else(|| {
        let name = executable("rust-analyzer");
        let paths = std::env::var_os("PATH")?;
        std::env::split_paths(&paths)
            .map(|dir| dir.join(&name))
            .filter(|p| p.is_file())
            .find(|p| {
                !p.parent()
                    .is_some_and(|d| d.ends_with(Path::new(".cargo").join("bin")))
            })
    })
}

/// The `Cargo.toml` files of a project whose root has none, a few folders down: the server is
/// told about them, as it only looks at the root by itself.
pub fn linked_projects(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if out.len() >= 20 {
            return;
        }
        let manifest = dir.join("Cargo.toml");
        if manifest.is_file() {
            out.push(manifest);
            // A workspace lists its members itself.
            return;
        }
        if depth == 0 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .filter(|p| {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                !name.starts_with('.') && !matches!(name, "target" | "node_modules")
            })
            .collect();
        dirs.sort();
        for sub in dirs {
            walk(&sub, depth - 1, out);
        }
    }
    let mut out = Vec::new();
    walk(root, 3, &mut out);
    out
}

/// What the server is told at the start.
pub fn options(root: &Path) -> Value {
    let mut options = json!({
        "checkOnSave": true,
        "cargo": {"buildScripts": {"enable": true}},
        "procMacro": {"enable": true},
    });
    if !root.join("Cargo.toml").is_file() {
        let linked: Vec<String> = linked_projects(root)
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        if !linked.is_empty() {
            options["linkedProjects"] = json!(linked);
        }
    }
    options
}

pub fn launch(program: PathBuf, root: &Path) -> Launch {
    Launch {
        program,
        args: Vec::new(),
        options: options(root),
    }
}

/// Adds the component to the rustup toolchain, in the background.
pub fn install() {
    let started = control(|c| !std::mem::replace(&mut c.installing, true));
    if !started {
        return;
    }
    thread::spawn(|| {
        let result = quiet("rustup")
            .args(["component", "add", "rust-analyzer"])
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    tr!("rust.rustup_not_found").to_string()
                } else {
                    e.to_string()
                }
            })
            .and_then(|output| {
                if output.status.success() {
                    Ok(())
                } else {
                    let error = String::from_utf8_lossy(&output.stderr);
                    let last: Vec<&str> = error.lines().rev().take(4).collect();
                    Err(last.into_iter().rev().collect::<Vec<_>>().join(" "))
                }
            });
        control(|c| {
            c.installing = false;
            match result {
                Ok(()) => {
                    c.restart_requested = true;
                    c.messages
                        .push((tr!("rust.server_installed").into(), false));
                }
                Err(e) => c
                    .messages
                    .push((trf!("rust.cannot_install_server", e = e), true)),
            }
        });
    });
}

#[cfg(test)]
#[path = "../../../test/languages/rust/server/tests.rs"]
mod tests;
