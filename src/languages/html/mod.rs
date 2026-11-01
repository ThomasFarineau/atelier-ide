//! HTML and Pug: colors, syntax errors, structure, completion of elements, attributes, values
//! and entities, documentation, occurrences and navigation of tag pairs, ids, classes and
//! mixins, and Reformat Code.
//!
//! The code a page holds belongs to its own language (`embedded`): `<script>` to JavaScript or
//! TypeScript, `<style>` (and the `style` attributes) to CSS, SCSS, Less or Sass, the event
//! handlers (`onclick`) to JavaScript, and in Pug the `script.` and `style.` blocks. They take
//! the colors of their language, and its provider answers for them: completion, errors,
//! documentation, navigation, rename, formatting.
//!
//! `vscode-html-language-server` (`server`), the one of VS Code, completes and documents HTML
//! once installed; the analysis of the IDE (`analysis`) answers before it runs, without it,
//! and for Pug, which it does not read.

pub mod analysis;
pub mod data;
pub mod embedded;
pub mod emmet;
pub mod format;
pub mod lexer;
pub mod pug;
mod server;
pub mod tree;

use crate::languages::{
    Completion, Diagnostic, Hover, Location, Symbol,
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

pub use embedded::is_markup;

pub struct HtmlLanguage;

impl Language for HtmlLanguage {
    fn handles(&self, language: &str) -> bool {
        language == "html"
    }
    fn name(&self) -> &'static str {
        "HTML"
    }
    fn language_id(&self) -> &'static str {
        "html"
    }
    fn server_name(&self) -> &'static str {
        "vscode-html-language-server"
    }
    fn find_server(&self, root: &Path) -> Option<Launch> {
        server::launch(root)
    }
    fn control(&self) -> &'static Mutex<Control> {
        &server::CONTROL
    }
    fn needs_server(&self) -> String {
        tr!("html.needs_server").into()
    }
    fn installing(&self) -> &'static str {
        tr!("html.installing_server")
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
    fn hover_local(&self, text: &str, offset: usize) -> Option<Hover> {
        analysis::hover(text, offset)
    }
    fn word_start(&self, text: &str, offset: usize) -> usize {
        analysis::word_start(text, offset)
    }
    fn triggers(&self) -> &'static [char] {
        &['<', '/', '&', '"', '=']
    }
    fn format_local(&self, text: &str, _path: Option<&Path>) -> Result<String, String> {
        Ok(format::reformat(text))
    }
    /// The server's formatter would format the scripts and styles too.
    fn formats_itself(&self) -> bool {
        true
    }
}

/// Pug has no language server: the analysis of the IDE answers.
pub struct PugLanguage;

static PUG_CONTROL: Mutex<Control> = Mutex::new(Control::new());

impl Language for PugLanguage {
    fn handles(&self, language: &str) -> bool {
        language == "pug"
    }
    fn name(&self) -> &'static str {
        "Pug"
    }
    fn language_id(&self) -> &'static str {
        "jade"
    }
    fn server_reads(&self, _language: &str) -> bool {
        false
    }
    fn server_name(&self) -> &'static str {
        "Pug"
    }
    fn find_server(&self, _root: &Path) -> Option<Launch> {
        None
    }
    fn control(&self) -> &'static Mutex<Control> {
        &PUG_CONTROL
    }
    fn needs_server(&self) -> String {
        tr!("html.pug_no_server").into()
    }
    fn installing(&self) -> &'static str {
        ""
    }
    fn syntax_errors(&self, text: &str) -> Vec<Diagnostic> {
        analysis::pug_errors(text)
    }
    fn symbols(&self, text: &str) -> Vec<Symbol> {
        analysis::pug_symbols(text)
    }
    fn highlights(&self, text: &str, offset: usize) -> Vec<Range<usize>> {
        analysis::pug_highlights(text, offset)
    }
    fn definition(&self, text: &str, offset: usize) -> Vec<Location> {
        analysis::pug_definition(text, offset)
    }
    fn complete(&self, text: &str, offset: usize, explicit: bool) -> Option<Completion> {
        analysis::pug_complete(text, offset, explicit)
    }
    fn hover_local(&self, text: &str, offset: usize) -> Option<Hover> {
        analysis::pug_hover(text, offset)
    }
    fn word_start(&self, text: &str, offset: usize) -> usize {
        analysis::word_start(text, offset)
    }
    fn format_local(&self, text: &str, _path: Option<&Path>) -> Result<String, String> {
        Ok(format::reformat_pug(text))
    }
    fn formats_itself(&self) -> bool {
        true
    }
    fn tab_size(&self) -> usize {
        2
    }
}

pub type Html = LspProvider<HtmlLanguage>;
pub type Pug = LspProvider<PugLanguage>;

pub fn provider(root: &Path, repaint: Arc<dyn Fn() + Send + Sync>) -> Html {
    LspProvider::new(HtmlLanguage, root, repaint)
}

pub fn pug_provider(root: &Path, repaint: Arc<dyn Fn() + Send + Sync>) -> Pug {
    LspProvider::new(PugLanguage, root, repaint)
}

/// The commands of HTML support, in the menu and Search Everywhere.
#[derive(Default)]
pub struct HtmlPlugin;

impl BuiltinPlugin for HtmlPlugin {
    fn language(&self) -> bool {
        true
    }
    fn id(&self) -> &'static str {
        "html"
    }
    fn name(&self) -> &'static str {
        "HTML / Pug"
    }
    fn description(&self) -> &'static str {
        tr!("html.description")
    }
    fn icon(&self) -> &'static str {
        "languages/html/extensions/html+htm+xhtml"
    }

    fn update(&mut self, _ctx: &eframe::egui::Context, host: &mut dyn Host) {
        for (text, error) in with_control(&server::CONTROL, |c| std::mem::take(&mut c.messages)) {
            host.balloon("HTML", text, error);
        }
    }

    fn file_saved(&mut self, path: &Path) {
        if crate::project::language(path) == "html" {
            with_control(&server::CONTROL, |c| c.saved.push(path.to_path_buf()));
        }
    }

    fn commands(&self) -> Vec<Command> {
        [
            ("html.install", tr!("html.command_install")),
            ("html.restart", tr!("html.command_restart")),
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
            "html.install" => {
                host.notify(tr!("html.installing_server").into());
                server::install();
            }
            "html.restart" => with_control(&server::CONTROL, |c| c.restart_requested = true),
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../../../test/languages/html/tests.rs"]
mod tests;
