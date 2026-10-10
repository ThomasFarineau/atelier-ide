//! Translations built-in plugin: the i18n files of a project, from one file chosen as the source
//! (`.atelier/i18n.json`). The Translations window shows every key in every language, with what
//! is missing, empty, extra or has other placeholders than the source; a cell is edited in place
//! and written to its file, a language or a key is added in one step. The MCP tool `i18n` lets an
//! AI check the languages or add one ([`mcp_tool`]).

pub mod catalog;

use crate::i18n::Text;
use crate::plugins::builtin::{BuiltinPlugin, Command, Dock, Host, ToolWindow};
use crate::ui;
use catalog::{Catalog, Fill, Problem, Report};
use eframe::egui::{self, RichText};
use egui_extras::{Column, TableBuilder};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

pub static TOOL_WINDOWS: [ToolWindow; 1] = [ToolWindow {
    id: "translations.table",
    title: Text("translations.title"),
    icon: "translations/icon",
    dock: Dock::Bottom,
    shortcut: "",
}];

const CHECK_EVERY: Duration = Duration::from_secs(2);

/// The cell being edited: (key, language, text).
type Editing = (String, String, String);

#[derive(Default)]
pub struct TranslationsPlugin {
    root: PathBuf,
    catalog: Option<Catalog>,
    error: Option<String>,
    /// The keys, those of the source first, then those only other languages have.
    keys: Vec<String>,
    /// language → key → text.
    values: HashMap<String, HashMap<String, String>>,
    reports: Vec<Report>,
    stamps: Vec<(PathBuf, Option<SystemTime>)>,
    checked: Option<Instant>,
    candidates: Vec<PathBuf>,
    scanning: Option<JoinHandle<Vec<PathBuf>>>,
    filter: String,
    problems_only: bool,
    editing: Option<Editing>,
    new_language: String,
    new_key: String,
}

fn stamp(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl TranslationsPlugin {
    /// Reads the configuration and every file of the catalog again.
    fn reload(&mut self) {
        self.error = None;
        self.catalog = None;
        self.keys.clear();
        self.values.clear();
        self.reports.clear();
        self.stamps.clear();
        let Some(source) = catalog::source_of(&self.root) else {
            return;
        };
        let catalog = match catalog::discover(&source) {
            Ok(catalog) => catalog,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        for file in catalog.files() {
            self.stamps.push((file.path.clone(), stamp(&file.path)));
            match catalog::read(&file.path, catalog.format) {
                Ok(entries) => {
                    for (key, _) in &entries {
                        if !self.keys.contains(key) {
                            self.keys.push(key.clone());
                        }
                    }
                    self.values
                        .insert(file.language.clone(), entries.into_iter().collect());
                }
                Err(e) => self.error = Some(e),
            }
        }
        // The folder of a language added outside the IDE is a new file to watch.
        if let Some(dir) = catalog.source.path.parent() {
            self.stamps.push((dir.to_path_buf(), stamp(dir)));
        }
        self.reports = catalog::check_all(&catalog)
            .into_iter()
            .filter_map(Result::ok)
            .collect();
        self.catalog = Some(catalog);
    }

    fn changed_on_disk(&self) -> bool {
        self.stamps.iter().any(|(path, at)| stamp(path) != *at)
            || self
                .catalog
                .is_none()
                .then(|| catalog::source_of(&self.root))
                .flatten()
                .is_some()
    }

    fn use_source(&mut self, path: &Path, host: &mut dyn Host) {
        match catalog::set_source(&self.root, path) {
            Ok(()) => {
                self.reload();
                if let Some(catalog) = &self.catalog {
                    host.notify(trf!(
                        "translations.source_set",
                        language = catalog.source.language,
                        count = catalog.others.len() + 1
                    ));
                }
            }
            Err(e) => host.notify(trf!("translations.cannot_use_source", e = e)),
        }
    }

    fn report(&self, language: &str) -> Option<&Report> {
        self.reports.iter().find(|r| r.language == language)
    }

    /// The problem of a key in a language, if any.
    fn problem(&self, language: &str, key: &str) -> Option<&Problem> {
        self.report(language)?
            .problems
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, p)| p)
    }

    fn write(&mut self, language: &str, key: &str, text: &str, host: &mut dyn Host) {
        let Some(catalog) = &self.catalog else { return };
        let path = catalog.path_for(language);
        let path = catalog.file(language).map_or(path, |f| f.path.clone());
        if let Err(e) = catalog::set_value(&path, catalog.format, key, text) {
            host.notify(trf!("translations.cannot_write", e = e));
        }
        self.reload();
        host.reload_files();
    }

    /// The line of a key in a file, to open it there: the line of its last segment.
    fn key_line(path: &Path, key: &str) -> usize {
        let Ok(text) = std::fs::read_to_string(path) else {
            return 0;
        };
        let last = key.rsplit('.').next().unwrap_or(key);
        let quoted = format!("\"{key}\"");
        let segment = format!("\"{last}\"");
        let at = text
            .find(&quoted)
            .or_else(|| text.find(&segment))
            .or_else(|| {
                text.lines()
                    .position(|l| l.trim_start().starts_with(key))
                    .map(|i| text.lines().take(i).map(|l| l.len() + 1).sum())
            })
            .unwrap_or(0);
        text[..at.min(text.len())].matches('\n').count()
    }

    fn setup(&mut self, ui: &mut egui::Ui, host: &mut dyn Host) {
        let t = ui::theme(ui.ctx());
        ui.add(
            egui::Label::new(
                RichText::new(tr!("translations.setup_intro")).color(t.text_secondary),
            )
            .wrap(),
        );
        ui.add_space(6.0);
        if let Some(error) = &self.error {
            ui.colored_label(t.error, error);
        }
        if let Some(active) = host.active_file()
            && ui::primary_button(ui, tr!("translations.use_active_file")).clicked()
        {
            self.use_source(&active, host);
        }
        ui.add_space(8.0);
        if self.scanning.is_some() {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(12.0));
                ui.label(RichText::new(tr!("translations.scanning")).color(t.text_secondary));
            });
            return;
        }
        if self.candidates.is_empty() {
            ui.label(RichText::new(tr!("translations.no_candidates")).color(t.text_secondary));
            return;
        }
        ui::section(ui, tr!("translations.candidates"));
        let mut chosen = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for path in &self.candidates {
                    let shown = path
                        .strip_prefix(&self.root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    ui.horizontal(|ui| {
                        if ui::button(ui, tr!("translations.use_as_source")).clicked() {
                            chosen = Some(path.clone());
                        }
                        ui.label(shown);
                    });
                }
            });
        if let Some(path) = chosen {
            self.use_source(&path, host);
        }
    }

    fn table(&mut self, ui: &mut egui::Ui, host: &mut dyn Host) {
        let t = ui::theme(ui.ctx());
        let Some(catalog) = self.catalog.clone() else {
            return;
        };
        let languages: Vec<String> = catalog.files().map(|f| f.language.clone()).collect();
        let source_language = catalog.source.language.clone();
        // Toolbar.
        let mut add_language = None;
        let mut add_key = None;
        ui.horizontal(|ui| {
            if ui::icon_button(ui, "refresh", tr!("translations.reload")).clicked() {
                self.reload();
            }
            let source = catalog
                .source
                .path
                .strip_prefix(&self.root)
                .unwrap_or(&catalog.source.path)
                .to_string_lossy()
                .replace('\\', "/");
            ui.label(
                RichText::new(trf!(
                    "translations.source_label",
                    language = source_language,
                    path = source
                ))
                .color(t.text_secondary),
            );
            ui.add_space(8.0);
            ui::checkbox(
                ui,
                &mut self.problems_only,
                tr!("translations.problems_only"),
            );
            ui::search_field(ui, &mut self.filter, tr!("translations.filter"), 200.0);
            ui.add_space(8.0);
            let field = ui::search_field(
                ui,
                &mut self.new_language,
                tr!("translations.new_language_hint"),
                130.0,
            );
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui::button(ui, tr!("translations.add_language")).clicked() || enter)
                && !self.new_language.trim().is_empty()
            {
                add_language = Some(self.new_language.trim().to_string());
            }
            let field = ui::search_field(
                ui,
                &mut self.new_key,
                tr!("translations.new_key_hint"),
                160.0,
            );
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui::button(ui, tr!("translations.add_key")).clicked() || enter)
                && !self.new_key.trim().is_empty()
            {
                add_key = Some(self.new_key.trim().to_string());
            }
        });
        // One chip per language: how much is translated, how many problems.
        ui.horizontal_wrapped(|ui| {
            for language in &languages {
                let text = match self.report(language) {
                    Some(report) => {
                        trf!(
                            "translations.chip",
                            language = language,
                            percent = report.percent(),
                            errors = report.errors()
                        )
                    }
                    None => trf!("translations.chip_source", language = language),
                };
                let color = match self.report(language) {
                    Some(report) if report.errors() > 0 => t.warning,
                    Some(_) => t.success,
                    None => t.link,
                };
                let x = ui.cursor().left();
                let y = ui.cursor().top() + 10.0;
                let right = ui::paint_badge(ui, ui.painter(), x, y, &text, color);
                ui.allocate_exact_size(egui::vec2(right - x + 6.0, 20.0), egui::Sense::hover());
            }
        });
        if let Some(error) = &self.error {
            ui.colored_label(t.error, error);
        }
        if let Some(language) = add_language {
            match catalog::add_language(&catalog, &language, Fill::Empty) {
                Ok(path) => {
                    self.new_language.clear();
                    self.reload();
                    host.notify(trf!(
                        "translations.language_added",
                        language = language,
                        path = path.display()
                    ));
                }
                Err(e) => host.notify(trf!("translations.cannot_add_language", e = e)),
            }
        }
        if let Some(key) = add_key {
            let path = catalog.source.path.clone();
            if let Err(e) = catalog::set_value(&path, catalog.format, &key, "") {
                host.notify(trf!("translations.cannot_write", e = e));
            }
            for other in &catalog.others {
                let _ = catalog::set_value(&other.path, catalog.format, &key, "");
            }
            self.new_key.clear();
            self.reload();
            self.editing = Some((key, source_language.clone(), String::new()));
        }
        let filter = self.filter.trim().to_lowercase();
        let keys: Vec<String> = self
            .keys
            .iter()
            .filter(|key| {
                filter.is_empty()
                    || key.to_lowercase().contains(&filter)
                    || languages.iter().any(|l| {
                        self.values
                            .get(l)
                            .and_then(|v| v.get(*key))
                            .is_some_and(|v| v.to_lowercase().contains(&filter))
                    })
            })
            .filter(|key| {
                !self.problems_only
                    || languages
                        .iter()
                        .any(|l| self.problem(l, key).is_some_and(Problem::is_error))
            })
            .cloned()
            .collect();
        ui.add_space(4.0);
        let mut commit: Option<Editing> = None;
        let mut start: Option<Editing> = None;
        let mut cancel = false;
        let mut open: Option<usize> = None;
        let mut delete: Option<String> = None;
        let available = ui.available_width();
        let width = ((available - 220.0) / languages.len().max(1) as f32).max(140.0);
        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .auto_shrink([false, false])
            .column(Column::initial(220.0).at_least(120.0).clip(true));
        for _ in &languages {
            table = table.column(Column::initial(width).at_least(100.0).clip(true));
        }
        table
            .header(24.0, |mut header| {
                header.col(|ui| {
                    ui.label(RichText::new(trf!("translations.keys", count = keys.len())).strong());
                });
                for language in &languages {
                    header.col(|ui| {
                        let label = if *language == source_language {
                            trf!("translations.source_column", language = language)
                        } else {
                            language.clone()
                        };
                        ui.label(RichText::new(label).strong());
                    });
                }
            })
            .body(|body| {
                body.rows(24.0, keys.len(), |mut row| {
                    let index = row.index();
                    let key = &keys[index];
                    row.col(|ui| {
                        let response = ui
                            .add(
                                egui::Label::new(RichText::new(key).monospace().size(12.0))
                                    .truncate()
                                    .sense(egui::Sense::click()),
                            )
                            .on_hover_text(key);
                        if response.double_clicked() {
                            open = Some(index);
                        }
                        response.context_menu(|ui| {
                            ui::menu_begin(ui);
                            if ui::menu_item(
                                ui,
                                Some("openNewTab"),
                                tr!("translations.jump_to_key"),
                                None,
                                false,
                            )
                            .clicked()
                            {
                                open = Some(index);
                                ui.close();
                            }
                            if ui::menu_item(
                                ui,
                                Some("copy"),
                                tr!("translations.copy_key"),
                                None,
                                false,
                            )
                            .clicked()
                            {
                                ui.ctx().copy_text(key.clone());
                                ui.close();
                            }
                            if ui::menu_item(
                                ui,
                                Some("delete"),
                                tr!("translations.delete_key"),
                                None,
                                true,
                            )
                            .clicked()
                            {
                                delete = Some(key.clone());
                                ui.close();
                            }
                        });
                    });
                    for language in &languages {
                        row.col(|ui| {
                            let editing_here = self
                                .editing
                                .as_ref()
                                .is_some_and(|(k, l, _)| k == key && l == language);
                            if editing_here {
                                let Some((_, _, text)) = &mut self.editing else {
                                    return;
                                };
                                let edit = ui.add(
                                    egui::TextEdit::singleline(text).desired_width(f32::INFINITY),
                                );
                                edit.request_focus();
                                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                    cancel = true;
                                } else if edit.lost_focus() {
                                    commit = Some((key.clone(), language.clone(), text.clone()));
                                }
                                return;
                            }
                            let value = self.values.get(language).and_then(|v| v.get(key));
                            let problem = self.problem(language, key);
                            // A key the source does not have is missing nowhere: only extra
                            // where it is.
                            let in_source = self
                                .values
                                .get(&source_language)
                                .is_some_and(|v| v.contains_key(key));
                            let (shown, color) = match (value, problem) {
                                (None, _) if !in_source => ("—".to_string(), t.text_disabled),
                                (None, _) => {
                                    (tr!("translations.cell_missing").to_string(), t.error)
                                }
                                (Some(v), Some(Problem::Extra)) => (v.clone(), t.warning),
                                (Some(v), _) if v.is_empty() => {
                                    (tr!("translations.cell_empty").to_string(), t.warning)
                                }
                                (Some(v), Some(Problem::Placeholders { .. })) => {
                                    (v.clone(), t.warning)
                                }
                                (Some(v), Some(Problem::SameAsSource)) => {
                                    (v.clone(), t.text_secondary)
                                }
                                (Some(v), _) => (v.clone(), t.text),
                            };
                            let mut response = ui.add(
                                egui::Label::new(RichText::new(shown).color(color).size(12.5))
                                    .truncate()
                                    .sense(egui::Sense::click()),
                            );
                            if let Some(problem) = problem {
                                response = response.on_hover_text(problem.label());
                            } else if let Some(value) = value {
                                response = response.on_hover_text(value);
                            }
                            if response.clicked() {
                                start = Some((
                                    key.clone(),
                                    language.clone(),
                                    value.cloned().unwrap_or_default(),
                                ));
                            }
                        });
                    }
                });
            });
        if cancel {
            self.editing = None;
        }
        if let Some((key, language, text)) = commit {
            self.editing = None;
            let before = self
                .values
                .get(&language)
                .and_then(|v| v.get(&key))
                .cloned();
            if before.as_deref() != Some(text.as_str()) {
                self.write(&language, &key, &text, host);
            }
        }
        if let Some(edit) = start {
            self.editing = Some(edit);
        }
        if let Some(index) = open {
            let path = catalog.source.path.clone();
            let line = Self::key_line(&path, &keys[index]);
            host.open_at(path, line, 0, 0);
        }
        if let Some(key) = delete {
            for file in catalog.files() {
                if let Err(e) = catalog::remove_key(&file.path, catalog.format, &key) {
                    host.notify(trf!("translations.cannot_write", e = e));
                }
            }
            self.reload();
            host.reload_files();
        }
    }
}

impl BuiltinPlugin for TranslationsPlugin {
    fn id(&self) -> &'static str {
        "translations"
    }
    fn name(&self) -> &'static str {
        tr!("translations.title")
    }
    fn description(&self) -> &'static str {
        tr!("translations.description")
    }
    fn icon(&self) -> &'static str {
        "translations/icon"
    }
    fn tool_windows(&self) -> &'static [ToolWindow] {
        &TOOL_WINDOWS
    }
    /// In a project that has a source, or files that look like translations.
    fn applies(&self) -> bool {
        self.catalog.is_some() || self.error.is_some() || !self.candidates.is_empty()
    }

    fn update(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        if host.root() != self.root {
            *self = Self {
                root: host.root().to_path_buf(),
                ..Self::default()
            };
            self.reload();
            let root = self.root.clone();
            self.scanning = Some(thread::spawn(move || catalog::candidates(&root)));
        }
        if let Some(handle) = self.scanning.take_if(|h| h.is_finished()) {
            self.candidates = handle.join().unwrap_or_default();
            ctx.request_repaint();
        } else if self.scanning.is_some() {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
        // Files changed by hand, by git or by an AI: the table follows, unless a cell is edited.
        if self.editing.is_none() && self.checked.is_none_or(|at| at.elapsed() > CHECK_EVERY) {
            self.checked = Some(Instant::now());
            if self.changed_on_disk() {
                self.reload();
            }
        }
    }

    fn file_saved(&mut self, path: &Path) {
        let ours = self
            .catalog
            .as_ref()
            .is_some_and(|c| c.files().any(|f| f.path == path));
        if ours {
            self.reload();
        }
    }

    fn commands(&self) -> Vec<Command> {
        vec![
            Command {
                id: "translations.use_active",
                label: tr!("translations.command_use_active").into(),
            },
            Command {
                id: "translations.check",
                label: tr!("translations.command_check").into(),
            },
            Command {
                id: "translations.window",
                label: tr!("translations.command_window").into(),
            },
        ]
    }

    fn run_command(&mut self, id: &str, host: &mut dyn Host) {
        match id {
            "translations.use_active" => match host.active_file() {
                Some(path) => self.use_source(&path, host),
                None => host.notify(tr!("translations.no_active_file").into()),
            },
            "translations.check" => {
                self.reload();
                let Some(catalog) = &self.catalog else {
                    host.show_tool_window(&TOOL_WINDOWS[0]);
                    return;
                };
                let source = catalog.source.language.clone();
                for report in &self.reports {
                    let error = report.errors() > 0;
                    host.balloon(
                        tr!("translations.title"),
                        catalog::describe(report, &source),
                        error,
                    );
                }
                if self.reports.is_empty() {
                    host.notify(tr!("translations.no_other_language").into());
                }
            }
            "translations.window" => host.show_tool_window(&TOOL_WINDOWS[0]),
            _ => {}
        }
    }

    fn tool_window(&mut self, _id: &str, ui: &mut egui::Ui, host: &mut dyn Host) {
        if self.catalog.is_none() {
            self.setup(ui, host);
        } else {
            self.table(ui, host);
        }
    }
}

// ---- MCP -----------------------------------------------------------------------------------

/// The `i18n` tool of the MCP server.
pub fn mcp_spec() -> Value {
    json!({
        "name": "i18n",
        "description": "The translations of the project, from the source file chosen in Atelier (.atelier/i18n.json); the other languages are the files next to it (locales/en.json, locales/en/common.json, messages_en.properties). action \"languages\" lists them; \"check\" checks every language against the source; \"check_language\" checks one language in detail; \"add_language\" creates the file of a new language with every key of the source. A check lists the missing keys, the keys the source does not have, the empty values, the values whose placeholders ({name}, {{count}}, %s) differ from the source, and the values left as in the source. Pass \"source\" (a path) to choose the source file when none is set, or to change it.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["languages", "check", "check_language", "add_language"]},
                "language": {"type": "string", "description": "The language, as in the file names: en, fr_FR, pt-BR… Required by check_language and add_language."},
                "fill": {"type": "string", "enum": ["empty", "source"], "description": "add_language: the new values start empty (default) or as the text of the source, to translate in place."},
                "source": {"type": "string", "description": "Path of the file to use as the source of the translations, absolute or relative to the project."},
            },
            "required": ["action"],
        },
    })
}

fn report_json(report: &Report) -> Value {
    let keys = |kind: fn(&Problem) -> bool| -> Vec<&str> {
        report
            .problems
            .iter()
            .filter(|(_, p)| kind(p))
            .map(|(k, _)| k.as_str())
            .collect()
    };
    let placeholders: Vec<Value> = report
        .problems
        .iter()
        .filter_map(|(k, p)| match p {
            Problem::Placeholders { source, found } => {
                Some(json!({"key": k, "source": source, "found": found}))
            }
            _ => None,
        })
        .collect();
    json!({
        "language": report.language,
        "file": report.path,
        "translated": report.translated,
        "total": report.total,
        "percent": report.percent(),
        "missing": keys(|p| *p == Problem::Missing),
        "extra": keys(|p| *p == Problem::Extra),
        "empty": keys(|p| *p == Problem::Empty),
        "placeholders": placeholders,
        "sameAsSource": keys(|p| *p == Problem::SameAsSource),
    })
}

/// Runs the `i18n` tool for a project; None for another tool.
pub fn mcp_tool(name: &str, args: &Value, root: &Path) -> Option<Result<String, String>> {
    (name == "i18n").then(|| run_tool(args, root))
}

fn run_tool(args: &Value, root: &Path) -> Result<String, String> {
    if let Some(source) = args["source"].as_str().filter(|s| !s.is_empty()) {
        let path = Path::new(source);
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            root.join(path)
        };
        catalog::set_source(root, &path)?;
    }
    let source = catalog::source_of(root).ok_or(
        "No translation source is set for this project. Pass \"source\" with the path of the file of the reference language (for example locales/en.json), or choose it in Atelier's Translations window.",
    )?;
    let catalog = catalog::discover(&source)?;
    let language = args["language"]
        .as_str()
        .map(str::trim)
        .filter(|l| !l.is_empty());
    let pretty = |value: Value| serde_json::to_string_pretty(&value).map_err(|e| e.to_string());
    match args["action"].as_str().unwrap_or("") {
        "languages" => pretty(json!({
            "source": {"language": catalog.source.language, "file": catalog.source.path},
            "languages": catalog.others.iter().map(|f| json!({"language": f.language, "file": f.path})).collect::<Vec<_>>(),
        })),
        "check" => {
            let reports: Vec<Value> = catalog::check_all(&catalog)
                .into_iter()
                .map(|r| r.map_or_else(|e| json!({"error": e}), |r| report_json(&r)))
                .collect();
            pretty(json!({"source": catalog.source.language, "languages": reports}))
        }
        "check_language" => {
            let language = language.ok_or("\"language\" is required by check_language")?;
            pretty(report_json(&catalog::check(&catalog, language)?))
        }
        "add_language" => {
            let language = language.ok_or("\"language\" is required by add_language")?;
            let fill = if args["fill"].as_str() == Some("source") {
                Fill::Source
            } else {
                Fill::Empty
            };
            let path = catalog::add_language(&catalog, language, fill)?;
            let catalog = catalog::discover(&source)?;
            pretty(
                json!({"created": path, "report": report_json(&catalog::check(&catalog, language)?)}),
            )
        }
        other => Err(format!(
            "unknown action \"{other}\": languages, check, check_language or add_language"
        )),
    }
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/translations/mod/tests.rs"]
mod tests;
