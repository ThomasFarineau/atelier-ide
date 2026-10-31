//! CSS, SCSS, Less and Sass: colors, syntax errors, structure, completion, navigation,
//! occurrences, hover, rename and Reformat Code.
//!
//! `vscode-css-language-server` (`server`), the server of VS Code, knows CSS, SCSS and Less:
//! the properties and their documentation, the browsers that support them, the errors, the
//! colors, the formatter. Until it runs, or without it (and for Sass, which it does not read),
//! the analysis of the IDE (`analysis`) answers with what the file says: its variables,
//! mixins, functions, selectors and keyframes, the properties and values of CSS, and a
//! formatter that reindents.

pub mod analysis;
pub mod colors;
pub mod convert;
pub mod data;
pub mod format;
pub mod lexer;
mod server;

use crate::languages::{
    Completion, Diagnostic, Location, Symbol,
    lsp::{
        Launch,
        provider::{Control, Language, LspProvider, with_control},
    },
};
use crate::plugins::builtin::{BuiltinPlugin, Command, Host};
use lexer::Dialect;
use std::{
    ops::Range,
    path::Path,
    sync::{Arc, Mutex},
};

/// The languages of style sheets, as `project::language` names them.
pub fn is_style(language: &str) -> bool {
    matches!(language, "css" | "scss" | "sass" | "less")
}

pub struct CssLanguage;

impl Language for CssLanguage {
    fn handles(&self, language: &str) -> bool {
        is_style(language)
    }
    fn name(&self) -> &'static str {
        "CSS"
    }
    fn language_id(&self) -> &'static str {
        "css"
    }
    fn language_id_for(&self, language: &str) -> &'static str {
        match language {
            "scss" => "scss",
            "less" => "less",
            // The server does not read the indented syntax: it is left to the IDE.
            "sass" => "sass",
            _ => "css",
        }
    }
    fn server_reads(&self, language: &str) -> bool {
        language != "sass"
    }
    fn server_name(&self) -> &'static str {
        "vscode-css-language-server"
    }
    fn find_server(&self, root: &Path) -> Option<Launch> {
        server::launch(root)
    }
    fn control(&self) -> &'static Mutex<Control> {
        &server::CONTROL
    }
    fn needs_server(&self) -> String {
        tr!("css.needs_server").into()
    }
    fn installing(&self) -> &'static str {
        tr!("css.installing_server")
    }
    fn syntax_errors(&self, text: &str) -> Vec<Diagnostic> {
        analysis::syntax_errors(text, Dialect::guess(text))
    }
    fn symbols(&self, text: &str) -> Vec<Symbol> {
        analysis::symbols(text, Dialect::guess(text))
    }
    fn highlights(&self, text: &str, offset: usize) -> Vec<Range<usize>> {
        analysis::highlights(text, offset, Dialect::guess(text))
    }
    fn definition(&self, text: &str, offset: usize) -> Vec<Location> {
        analysis::definition(text, offset, Dialect::guess(text))
    }
    fn complete(&self, text: &str, offset: usize, explicit: bool) -> Option<Completion> {
        analysis::complete(text, offset, explicit, Dialect::guess(text))
    }
    fn word_start(&self, text: &str, offset: usize) -> usize {
        analysis::word_start(text, offset)
    }
    fn triggers(&self) -> &'static [char] {
        &[':', '@', '$', '-']
    }
    fn format_local(&self, text: &str, path: Option<&Path>) -> Result<String, String> {
        let dialect = match path {
            Some(_) => Dialect::of_path(path),
            None => Dialect::guess(text),
        };
        format::reformat(text, dialect, &analysis::indent_of(text))
    }
    /// The formatter of the IDE prints a minified sheet out again, which the server does not.
    fn formats_itself(&self) -> bool {
        true
    }
    fn tab_size(&self) -> usize {
        2
    }
}

pub type Css = LspProvider<CssLanguage>;

pub fn provider(root: &Path, repaint: Arc<dyn Fn() + Send + Sync>) -> Css {
    LspProvider::new(CssLanguage, root, repaint)
}

/// The commands of CSS support in Search Everywhere.
#[derive(Default)]
pub struct CssPlugin;

impl BuiltinPlugin for CssPlugin {
    fn id(&self) -> &'static str {
        "css"
    }
    fn name(&self) -> &'static str {
        "CSS / SCSS / Sass / Less"
    }
    fn description(&self) -> &'static str {
        tr!("css.description")
    }
    fn icon(&self) -> &'static str {
        "settings"
    }

    fn update(&mut self, _ctx: &eframe::egui::Context, host: &mut dyn Host) {
        for (text, error) in with_control(&server::CONTROL, |c| std::mem::take(&mut c.messages)) {
            host.balloon("CSS", text, error);
        }
    }

    fn file_saved(&mut self, path: &Path) {
        let style = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| is_style(&e.to_ascii_lowercase()));
        if style {
            with_control(&server::CONTROL, |c| c.saved.push(path.to_path_buf()));
        }
    }

    fn gutter_commands(&self, path: &Path) -> Vec<Command> {
        let mut commands = Vec::new();
        if minifiable(path) {
            commands.push(Command {
                id: "css.minify",
                label: tr!("css.command_minify_file").into(),
            });
        }
        if let Some(from) = style_dialect(path) {
            for (id, to, label) in conversions() {
                if to != from {
                    commands.push(Command {
                        id,
                        label: label.into(),
                    });
                }
            }
        }
        commands
    }

    fn commands(&self) -> Vec<Command> {
        [
            ("css.minify", tr!("css.command_minify")),
            ("css.to_scss", tr!("css.command_convert_scss")),
            ("css.to_less", tr!("css.command_convert_less")),
            ("css.to_sass", tr!("css.command_convert_sass")),
            ("css.install", tr!("css.command_install")),
            ("css.restart", tr!("css.command_restart")),
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
            "css.install" => {
                host.notify(tr!("css.installing_server").into());
                server::install();
            }
            "css.restart" => with_control(&server::CONTROL, |c| c.restart_requested = true),
            "css.minify" => match host.active_file().filter(|p| minifiable(p)) {
                Some(path) => {
                    // What the editor has, not an older text on the disk.
                    host.save_files();
                    match minify_file(&path) {
                        Ok(message) => host.balloon("CSS", message, false),
                        Err(e) => host.balloon("CSS", e, true),
                    }
                }
                None => host.notify(tr!("css.minify_needs_css").into()),
            },
            _ => {
                let Some((_, to, _)) = conversions().into_iter().find(|(c, ..)| *c == id) else {
                    return;
                };
                let Some(path) = host.active_file().filter(|p| style_dialect(p).is_some()) else {
                    host.notify(tr!("css.convert_needs_style").into());
                    return;
                };
                host.save_files();
                match convert_file(&path, to) {
                    Ok(target) => {
                        host.balloon(
                            "CSS",
                            trf!(
                                "css.converted",
                                name = target.file_name().unwrap_or_default().to_string_lossy()
                            ),
                            false,
                        );
                        host.open_file(target);
                    }
                    Err(e) => host.balloon("CSS", e, true),
                }
            }
        }
    }
}

/// The conversions offered: (command, dialect, label).
fn conversions() -> [(&'static str, Dialect, &'static str); 3] {
    [
        (
            "css.to_scss",
            Dialect::Scss,
            tr!("css.command_convert_scss_file"),
        ),
        (
            "css.to_less",
            Dialect::Less,
            tr!("css.command_convert_less_file"),
        ),
        (
            "css.to_sass",
            Dialect::Sass,
            tr!("css.command_convert_sass_file"),
        ),
    ]
}

/// The dialect of a style sheet, None for another file.
fn style_dialect(path: &Path) -> Option<Dialect> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    is_style(&extension).then(|| Dialect::of(&extension))
}

/// Writes `path` in dialect `to` next to it: `name.scss`, or `name-2.scss` when that one
/// already exists, never over a file. The file written.
pub fn convert_file(path: &Path, to: Dialect) -> Result<std::path::PathBuf, String> {
    let from = style_dialect(path).ok_or_else(|| tr!("css.convert_needs_style").to_string())?;
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let converted = convert::convert(&text, from, to)?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = convert::extension(to);
    let target = (1..)
        .map(|n| {
            let name = if n == 1 {
                format!("{stem}.{extension}")
            } else {
                format!("{stem}-{n}.{extension}")
            };
            path.with_file_name(name)
        })
        .find(|p| !p.exists())
        .expect("a free name");
    std::fs::write(&target, converted).map_err(|e| e.to_string())?;
    Ok(target)
}

/// A sheet worth minifying: CSS, not already minified.
fn minifiable(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    name.ends_with(".css") && !name.ends_with(".min.css")
}

/// Writes `name.min.css` next to `name.css`. What to tell the user.
pub fn minify_file(path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let small = format::minify(&text, Dialect::Css)?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let target = path.with_file_name(format!("{stem}.min.css"));
    std::fs::write(&target, &small).map_err(|e| e.to_string())?;
    let kib = |n: usize| format!("{:.1}", n as f64 / 1024.0);
    Ok(trf!(
        "css.minified",
        name = target.file_name().unwrap_or_default().to_string_lossy(),
        from = kib(text.len()),
        to = kib(small.len())
    ))
}

#[cfg(test)]
#[path = "../../../test/languages/css/tests.rs"]
mod tests;
