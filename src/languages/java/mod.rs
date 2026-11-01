//! Java support: syntax errors, structure, completion, navigation, hover, rename, quick fixes,
//! Reformat Code and Optimize Imports.
//!
//! jdtls (`server`) knows the project as the compiler does, with its Maven or Gradle build and
//! its dependencies. Until it runs, or without it, the analysis of the file with tree-sitter
//! (`analysis`) gives the syntax errors, the structure, the names of the file, where they are
//! declared, and IntelliJ's live templates (`psvm`, `sout`, `fori`…).

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

pub struct JavaLanguage;

impl Language for JavaLanguage {
    fn handles(&self, language: &str) -> bool {
        language == "java"
    }
    fn name(&self) -> &'static str {
        "Java"
    }
    fn language_id(&self) -> &'static str {
        "java"
    }
    fn server_name(&self) -> &'static str {
        "jdtls"
    }
    fn find_server(&self, root: &Path) -> Option<Launch> {
        server::find(root)
    }
    fn control(&self) -> &'static Mutex<Control> {
        &server::CONTROL
    }
    fn needs_server(&self) -> String {
        tr!("java.needs_server").into()
    }
    fn installing(&self) -> &'static str {
        tr!("java.installing_server")
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
        &['.', '@']
    }
}

pub type Java = LspProvider<JavaLanguage>;

pub fn provider(root: &Path, repaint: Arc<dyn Fn() + Send + Sync>) -> Java {
    LspProvider::new(JavaLanguage, root, repaint)
}

/// The commands of Java support in Search Everywhere, and the check of the code on save.
#[derive(Default)]
pub struct JavaPlugin;

impl BuiltinPlugin for JavaPlugin {
    fn language(&self) -> bool {
        true
    }
    fn id(&self) -> &'static str {
        "java"
    }
    fn name(&self) -> &'static str {
        "Java"
    }
    fn description(&self) -> &'static str {
        tr!("java.description")
    }
    fn icon(&self) -> &'static str {
        "settings"
    }

    fn update(&mut self, _ctx: &eframe::egui::Context, host: &mut dyn Host) {
        for (text, error) in with_control(&server::CONTROL, |c| std::mem::take(&mut c.messages)) {
            host.balloon("Java", text, error);
        }
    }

    fn file_saved(&mut self, path: &Path) {
        if path.extension().is_some_and(|e| e == "java") {
            with_control(&server::CONTROL, |c| c.saved.push(path.to_path_buf()));
        }
    }

    fn commands(&self) -> Vec<Command> {
        [
            ("java.install", tr!("java.command_install")),
            ("java.restart", tr!("java.command_restart")),
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
            "java.install" => {
                host.notify(tr!("java.installing_server").into());
                server::install();
            }
            "java.restart" => with_control(&server::CONTROL, |c| c.restart_requested = true),
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../../../test/languages/java/mod/tests.rs"]
mod tests;
