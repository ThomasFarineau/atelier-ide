//! Finding and installing `vscode-css-language-server` (from `vscode-langservers-extracted`),
//! the language server of CSS, SCSS and Less: the one of VS Code. It needs Node.js; the IDE
//! installs it next to the server of JavaScript (`~/.atelier/lsp`), so nothing global is touched.

use crate::languages::{
    javascript::server::{find_in, managed_dir, npm},
    lsp::{
        Launch,
        provider::{Control, with_control},
    },
};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    thread,
};

pub static CONTROL: Mutex<Control> = Mutex::new(Control::new());

const NAME: &str = "vscode-css-language-server";
const PACKAGE: &str = "vscode-langservers-extracted";

/// The server: the project's own, then the IDE's, then the one on the PATH.
pub fn find_server(project: &Path) -> Option<PathBuf> {
    find_in(&project.join("node_modules").join(".bin"), NAME)
        .or_else(|| find_in(&managed_dir().join("node_modules").join(".bin"), NAME))
        .or_else(|| {
            std::env::var_os("PATH")
                .and_then(|paths| std::env::split_paths(&paths).find_map(|dir| find_in(&dir, NAME)))
        })
}

pub fn launch(project: &Path) -> Option<Launch> {
    Some(Launch {
        program: find_server(project)?,
        args: vec!["--stdio".into()],
        // Without it the server does not format.
        options: json!({"provideFormatter": true}),
        settings: serde_json::Value::Null,
    })
}

/// Installs the server in the IDE's folder, in the background.
pub fn install() {
    install_into(&CONTROL, tr!("css.server_installed"), |e| {
        trf!("css.cannot_install_server", e = e)
    });
}

/// Installs `vscode-langservers-extracted` for the language of `language_control` (CSS,
/// HTML: the package has the servers of both), telling it `installed` or the error.
pub fn install_into(
    language_control: &'static Mutex<Control>,
    installed: &'static str,
    failed: fn(String) -> String,
) {
    let started = with_control(language_control, |c| {
        !std::mem::replace(&mut c.installing, true)
    });
    if !started {
        return;
    }
    thread::spawn(move || {
        let dir = managed_dir();
        let result = std::fs::create_dir_all(&dir)
            .map_err(|e| e.to_string())
            .and_then(|()| {
                npm()
                    .args(["install", "--no-audit", "--no-fund", "--prefix"])
                    .arg(&dir)
                    .arg(PACKAGE)
                    .output()
                    .map_err(|e| {
                        if e.kind() == std::io::ErrorKind::NotFound {
                            tr!("css.npm_not_found").to_string()
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
        with_control(language_control, |c| {
            c.installing = false;
            match result {
                Ok(()) => {
                    c.restart_requested = true;
                    c.messages.push((installed.into(), false));
                }
                Err(e) => c.messages.push((failed(e), true)),
            }
        });
    });
}
