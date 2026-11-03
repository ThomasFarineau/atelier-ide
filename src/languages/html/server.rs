//! Finding and installing `vscode-html-language-server`, the language server of HTML of VS Code.
//! It comes in the same npm package as the one of CSS (`vscode-langservers-extracted`).
//!
//! It is told to leave the code of `<script>` and `<style>` alone: the IDE gives that code to
//! the providers of JavaScript and CSS.

use crate::languages::{
    javascript::server::{find_in, managed_dir},
    lsp::{Launch, provider::Control},
};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

pub static CONTROL: Mutex<Control> = Mutex::new(Control::new());

const NAME: &str = "vscode-html-language-server";

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
        // The IDE formats HTML itself, and the other languages answer for their code.
        options: json!({
            "provideFormatter": false,
            "embeddedLanguages": {"css": false, "javascript": false},
        }),
        settings: json!({
            "html": {
                "validate": {"scripts": false, "styles": false},
                "suggest": {"html5": true},
                "hover": {"documentation": true, "references": true},
                "autoClosingTags": true,
            },
        }),
    })
}

/// Installs the server in the IDE's folder, in the background.
pub fn install() {
    crate::languages::css::server::install_into(&CONTROL, tr!("html.server_installed"), |e| {
        trf!("html.cannot_install_server", e = e)
    });
}
