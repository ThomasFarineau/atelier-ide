//! Finding, installing and driving `typescript-language-server`, the language server of
//! JavaScript and TypeScript. It needs Node.js; the IDE installs it in its own folder
//! (`~/.atelier/lsp`) so that nothing global is touched.

use crate::languages::lsp::Launch;
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    thread,
};

/// What the commands of the plugin ask of the provider, and what the provider tells back.
#[derive(Default)]
pub struct Control {
    pub install_requested: bool,
    pub restart_requested: bool,
    pub installing: bool,
    /// Messages for the user: (text, is an error).
    pub messages: Vec<(String, bool)>,
}

static CONTROL: Mutex<Control> = Mutex::new(Control {
    install_requested: false,
    restart_requested: false,
    installing: false,
    messages: Vec::new(),
});

pub fn control<R>(f: impl FnOnce(&mut Control) -> R) -> R {
    let mut guard = CONTROL.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut guard)
}

/// The folder the IDE installs language servers in (`ATELIER_LSP_DIR` overrides it, for tests).
pub fn managed_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("ATELIER_LSP_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map_or_else(std::env::temp_dir, PathBuf::from);
    home.join(".atelier").join("lsp")
}

fn executable_names(name: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![
            format!("{name}.cmd"),
            format!("{name}.exe"),
            format!("{name}.bat"),
        ]
    } else {
        vec![name.to_string()]
    }
}

pub(crate) fn find_in(dir: &Path, name: &str) -> Option<PathBuf> {
    executable_names(name)
        .into_iter()
        .map(|n| dir.join(n))
        .find(|p| p.is_file())
}

/// `typescript-language-server`: the project's own, then the IDE's, then the one on the PATH.
pub fn find_server(project: &Path) -> Option<PathBuf> {
    const NAME: &str = "typescript-language-server";
    find_in(&project.join("node_modules").join(".bin"), NAME)
        .or_else(|| find_in(&managed_dir().join("node_modules").join(".bin"), NAME))
        .or_else(|| {
            std::env::var_os("PATH")
                .and_then(|paths| std::env::split_paths(&paths).find_map(|dir| find_in(&dir, NAME)))
        })
}

/// The TypeScript the server should use: the project's, or the one installed with the server.
pub fn find_tsserver(project: &Path) -> Option<PathBuf> {
    [project.to_path_buf(), managed_dir()]
        .into_iter()
        .map(|dir| {
            dir.join("node_modules")
                .join("typescript")
                .join("lib")
                .join("tsserver.js")
        })
        .find(|p| p.is_file())
}

/// How to start the server for a project, if it is installed.
pub fn launch(project: &Path) -> Option<Launch> {
    let program = find_server(project)?;
    // The hints the editor shows in the code: the names of the arguments that are literals, the
    // types of the variables and the results of functions.
    let preferences = json!({
        "includeInlayParameterNameHints": "literals",
        "includeInlayVariableTypeHints": true,
        "includeInlayVariableTypeHintsWhenTypeMatchesName": false,
        "includeInlayFunctionLikeReturnTypeHints": true,
    });
    let options = match find_tsserver(project) {
        Some(path) => json!({
            "tsserver": {"path": path.to_string_lossy()},
            "hostInfo": "atelier",
            "preferences": preferences,
        }),
        None => json!({"hostInfo": "atelier", "preferences": preferences}),
    };
    Some(Launch {
        program,
        args: vec!["--stdio".into()],
        options,
    })
}

pub(crate) fn npm() -> Command {
    let mut command = Command::new(if cfg!(windows) { "npm.cmd" } else { "npm" });
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

/// Installs the server and TypeScript in the IDE's folder, in the background.
pub fn install() {
    let started = control(|c| {
        if c.installing {
            false
        } else {
            c.installing = true;
            true
        }
    });
    if !started {
        return;
    }
    thread::spawn(|| {
        let dir = managed_dir();
        let result = std::fs::create_dir_all(&dir)
            .map_err(|e| e.to_string())
            .and_then(|()| {
                npm()
                    .args(["install", "--no-audit", "--no-fund", "--prefix"])
                    .arg(&dir)
                    .args(["typescript@5", "typescript-language-server"])
                    .output()
                    .map_err(|e| {
                        if e.kind() == std::io::ErrorKind::NotFound {
                            tr!("javascript.npm_cannot_found_install_node").to_string()
                        } else {
                            e.to_string()
                        }
                    })
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
                    c.messages.push((
                        tr!("javascript.javascript_language_server_installed").into(),
                        false,
                    ));
                }
                Err(e) => c.messages.push((
                    trf!("javascript.cannot_install_language_server", e = e),
                    true,
                )),
            }
        });
    });
}

/// Whether Node.js is there to run the server.
pub fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(test)]
#[path = "../../../test/languages/javascript/server/tests.rs"]
mod tests;
