//! Rust support: syntax errors, structure, completion, navigation, hover, rename and Reformat
//! Code.
//!
//! `rust-analyzer` (`server`) answers with the knowledge of the compiler: types, traits, macros,
//! the crates of the project and their dependencies, `cargo check` on save. Until it runs, or
//! without it, the analysis of the file with tree-sitter (`analysis`) gives the syntax errors,
//! the structure, the names of the file and where they are declared, and rustfmt formats.

pub(crate) mod analysis;
mod format;
mod server;

use crate::languages::{
    Completion, Diagnostic, Location, Symbol,
    lsp::{
        Launch,
        provider::{Control, Language, LspProvider, with_control},
    },
};
use crate::plugins::builtin::{BuiltinPlugin, Command, Host};
use std::{
    ops::Range,
    path::Path,
    sync::{Arc, Mutex},
};

pub struct RustLanguage;

impl Language for RustLanguage {
    fn handles(&self, language: &str) -> bool {
        language == "rs"
    }
    fn name(&self) -> &'static str {
        "Rust"
    }
    fn language_id(&self) -> &'static str {
        "rust"
    }
    fn server_name(&self) -> &'static str {
        "rust-analyzer"
    }
    fn find_server(&self, root: &Path) -> Option<Launch> {
        server::find_server().map(|program| server::launch(program, root))
    }
    fn control(&self) -> &'static Mutex<Control> {
        &server::CONTROL
    }
    fn needs_server(&self) -> String {
        tr!("rust.rename_needs_server").into()
    }
    fn installing(&self) -> &'static str {
        tr!("rust.installing_server")
    }
    fn syntax_errors(&self, text: &str) -> Vec<Diagnostic> {
        analysis::syntax_errors(text)
    }
    fn symbols(&self, text: &str) -> Vec<Symbol> {
        analysis::symbols(text)
    }
    fn highlights(&self, text: &str, offset: usize) -> Vec<Range<usize>> {
        analysis::highlights(text, offset)
    }
    fn definition(&self, text: &str, offset: usize) -> Vec<Location> {
        analysis::definition(text, offset)
    }
    fn complete(&self, text: &str, offset: usize, explicit: bool) -> Option<Completion> {
        analysis::complete(text, offset, explicit)
    }
    fn triggers(&self) -> &'static [char] {
        &['.', ':', '\'']
    }
    /// rustfmt directly, with the edition of the crate.
    fn format_local(&self, text: &str, path: Option<&Path>) -> Result<String, String> {
        format::rustfmt(text, path)
    }
}

pub type Rust = LspProvider<RustLanguage>;

pub fn provider(root: &Path, repaint: Arc<dyn Fn() + Send + Sync>) -> Rust {
    LspProvider::new(RustLanguage, root, repaint)
}

/// The commands of Rust support in Search Everywhere, and `cargo check` on save.
#[derive(Default)]
pub struct RustPlugin;

impl BuiltinPlugin for RustPlugin {
    fn id(&self) -> &'static str {
        "rust"
    }
    fn name(&self) -> &'static str {
        "Rust"
    }
    fn description(&self) -> &'static str {
        tr!("rust.description")
    }
    fn icon(&self) -> &'static str {
        "settings"
    }

    fn update(&mut self, _ctx: &eframe::egui::Context, host: &mut dyn Host) {
        for (text, error) in with_control(&server::CONTROL, |c| std::mem::take(&mut c.messages)) {
            host.balloon("Rust", text, error);
        }
    }

    fn file_saved(&mut self, path: &Path) {
        if path.extension().is_some_and(|e| e == "rs") {
            with_control(&server::CONTROL, |c| c.saved.push(path.to_path_buf()));
        }
    }

    fn commands(&self) -> Vec<Command> {
        [
            ("rust.install", tr!("rust.command_install")),
            ("rust.restart", tr!("rust.command_restart")),
        ]
        .into_iter()
        .map(|(id, label)| Command {
            id,
            label: label.into(),
        })
        .collect()
    }

    fn run_command(&mut self, id: &str, host: &mut dyn Host) {
        match id {
            "rust.install" => {
                host.notify(tr!("rust.installing_server").into());
                server::install();
            }
            "rust.restart" => with_control(&server::CONTROL, |c| c.restart_requested = true),
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../../../test/languages/rust/mod/tests.rs"]
mod tests;
