//! Finding and installing pyright, the language server of Python, and the formatters ruff and
//! black. pyright needs Node.js; the IDE installs it in its own folder (`~/.atelier/lsp`), so
//! nothing global is touched. A `basedpyright` or `pyright` already on the PATH, or in the
//! virtual environment of the project, is used first. The server is told the Python of the
//! project (its `.venv`, else its runtime), to find the packages installed there.

use crate::languages::{
    javascript::server::{find_in, managed_dir, npm},
    lsp::{
        Launch,
        provider::{Control, with_control},
    },
};
use crate::plugins::builtins::runtimes::install;
use serde_json::json;
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    thread,
};

pub static CONTROL: Mutex<Control> = Mutex::new(Control::new());

/// The servers, the better first.
const NAMES: [&str; 2] = ["basedpyright-langserver", "pyright-langserver"];

fn control<R>(f: impl FnOnce(&mut Control) -> R) -> R {
    with_control(&CONTROL, f)
}

/// The folder of the programs of the virtual environment of the project.
fn venv_bin(project: &Path) -> Option<PathBuf> {
    install::venv(project).map(|(dir, _)| dir.join(if cfg!(windows) { "Scripts" } else { "bin" }))
}

fn on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .and_then(|paths| std::env::split_paths(&paths).find_map(|dir| find_in(&dir, name)))
}

pub fn find_server(project: &Path) -> Option<PathBuf> {
    NAMES.iter().find_map(|name| {
        venv_bin(project)
            .and_then(|dir| find_in(&dir, name))
            .or_else(|| find_in(&managed_dir().join("node_modules").join(".bin"), name))
            .or_else(|| on_path(name))
    })
}

pub fn launch(project: &Path) -> Option<Launch> {
    let program = find_server(project)?;
    let python = install::python(project).map(|p| p.to_string_lossy().into_owned());
    // pyright and basedpyright read the same settings, under their own names too.
    let analysis = json!({
        "autoSearchPaths": true,
        "useLibraryCodeForTypes": true,
        "diagnosticMode": "openFilesOnly",
    });
    Some(Launch {
        program,
        args: vec!["--stdio".into()],
        options: json!({}),
        settings: json!({
            "python": {"pythonPath": python},
            "python.analysis": analysis,
            "basedpyright.analysis": analysis,
        }),
    })
}

/// Installs pyright in the IDE's folder, in the background.
pub fn install() {
    let started = control(|c| !std::mem::replace(&mut c.installing, true));
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
                    .arg("pyright")
                    .output()
                    .map_err(|e| {
                        if e.kind() == std::io::ErrorKind::NotFound {
                            tr!("python.npm_not_found").to_string()
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
                    c.messages
                        .push((tr!("python.server_installed").into(), false));
                }
                Err(e) => c
                    .messages
                    .push((trf!("python.cannot_install_server", e = e), true)),
            }
        });
    });
}

fn quiet(program: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .stdin(Stdio::piped())
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

/// The text through ruff, else black: from the virtual environment of the project, else the
/// PATH. Runs the formatter in the folder of the file, for its settings (`pyproject.toml`).
pub fn format(text: &str, path: Option<&Path>) -> Result<String, String> {
    let dir = path.and_then(Path::parent);
    let project = dir.unwrap_or(Path::new("."));
    let find = |name: &str| {
        project
            .ancestors()
            .find_map(|d| venv_bin(d).and_then(|bin| find_in(&bin, name)))
            .or_else(|| on_path(name))
    };
    let file_name = path
        .and_then(|p| p.file_name())
        .map_or("stdin.py".into(), |n| n.to_string_lossy().into_owned());
    let (mut command, name) = if let Some(ruff) = find("ruff") {
        let mut c = quiet(&ruff);
        c.args(["format", "--stdin-filename", &file_name, "-"]);
        (c, "ruff")
    } else if let Some(black) = find("black") {
        let mut c = quiet(&black);
        c.args(["-q", "-"]);
        (c, "black")
    } else {
        return Err(tr!("python.no_formatter").into());
    };
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        let input = text.to_string();
        // Written from another thread: a big file would fill the pipe both ways.
        thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        });
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if output.status.success() {
        String::from_utf8(output.stdout).map_err(|e| e.to_string())
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        Err(trf!(
            "python.formatter_failed",
            name = name,
            error = error.lines().next().unwrap_or("").trim()
        ))
    }
}
