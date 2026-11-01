//! Python support: syntax errors, structure, completion, navigation, hover, rename, quick fixes
//! and Reformat Code.
//!
//! pyright (`server`) knows the project as the type checker does: the types, the packages of its
//! virtual environment, the errors. Until it runs, or without it, the analysis of the file with
//! tree-sitter (`analysis`) gives the syntax errors, the structure, the names of the file and
//! of `self`, where they are declared and IntelliJ's templates. ruff or black format.

pub(crate) mod analysis;
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

pub struct PythonLanguage;

impl Language for PythonLanguage {
    fn handles(&self, language: &str) -> bool {
        language == "py"
    }
    fn name(&self) -> &'static str {
        "Python"
    }
    fn language_id(&self) -> &'static str {
        "python"
    }
    fn server_name(&self) -> &'static str {
        "pyright"
    }
    fn find_server(&self, root: &Path) -> Option<Launch> {
        server::launch(root)
    }
    fn control(&self) -> &'static Mutex<Control> {
        &server::CONTROL
    }
    fn needs_server(&self) -> String {
        tr!("python.needs_server").into()
    }
    fn installing(&self) -> &'static str {
        tr!("python.installing_server")
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
    fn word_start(&self, text: &str, offset: usize) -> usize {
        analysis::word_start(text, offset)
    }
    fn triggers(&self) -> &'static [char] {
        &['.']
    }
    /// pyright does not format: ruff or black do.
    fn format_local(&self, text: &str, path: Option<&Path>) -> Result<String, String> {
        server::format(text, path)
    }
    fn formats_itself(&self) -> bool {
        true
    }
}

pub type Python = LspProvider<PythonLanguage>;

pub fn provider(root: &Path, repaint: Arc<dyn Fn() + Send + Sync>) -> Python {
    LspProvider::new(PythonLanguage, root, repaint)
}

/// The commands of Python support in Search Everywhere.
#[derive(Default)]
pub struct PythonPlugin;

impl BuiltinPlugin for PythonPlugin {
    fn language(&self) -> bool {
        true
    }
    fn id(&self) -> &'static str {
        "python"
    }
    fn name(&self) -> &'static str {
        "Python"
    }
    fn description(&self) -> &'static str {
        tr!("python.description")
    }
    fn icon(&self) -> &'static str {
        "settings"
    }

    fn update(&mut self, _ctx: &eframe::egui::Context, host: &mut dyn Host) {
        for (text, error) in with_control(&server::CONTROL, |c| std::mem::take(&mut c.messages)) {
            host.balloon("Python", text, error);
        }
    }

    fn file_saved(&mut self, path: &Path) {
        if path.extension().is_some_and(|e| e == "py" || e == "pyi") {
            with_control(&server::CONTROL, |c| c.saved.push(path.to_path_buf()));
        }
    }

    fn commands(&self) -> Vec<Command> {
        [
            ("python.install", tr!("python.command_install")),
            ("python.restart", tr!("python.command_restart")),
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
            "python.install" => {
                host.notify(tr!("python.installing_server").into());
                server::install();
            }
            "python.restart" => with_control(&server::CONTROL, |c| c.restart_requested = true),
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../../../test/languages/python/tests.rs"]
mod tests;
