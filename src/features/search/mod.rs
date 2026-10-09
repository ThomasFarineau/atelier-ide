//! Find in Files: the search of the whole project, with its results grouped by file, and the
//! replacement across the project (Ctrl+Shift+F).

use crate::{
    plugins::builtin::{BuiltinPlugin, Command, Dock, Host, ToolWindow},
    search::{self, FileHits, Options},
    ui,
};
use eframe::egui::{self, Align, Color32, FontId, Layout, RichText, TextFormat, text::LayoutJob};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

pub static TOOL_WINDOWS: [ToolWindow; 1] = [ToolWindow {
    id: "search.results",
    title: "Rechercher",
    icon: "search",
    dock: Dock::Bottom,
    shortcut: "Ctrl+Maj+F",
}];

const MAX_MATCHES: usize = 5000;
const ROW_HEIGHT: f32 = 22.0;

#[derive(Default)]
struct Results {
    files: Vec<FileHits>,
    matches: usize,
    looked: usize,
    done: bool,
    /// Files changed and replacements made, once a replacement is over.
    replaced: Option<(usize, usize)>,
}

#[derive(Default)]
pub struct SearchPlugin {
    query: String,
    replace: String,
    mask: String,
    options: Options,
    show_replace: bool,
    results: Arc<Mutex<Results>>,
    cancel: Option<Arc<AtomicBool>>,
    collapsed: HashSet<PathBuf>,
    selected: Option<(PathBuf, usize)>,
    focus: bool,
    error: Option<String>,
    /// The first click on "Tout remplacer" asks to confirm.
    confirm: bool,
    searched: bool,
}

enum Row {
    File(usize),
    Hit(usize, usize),
}

impl SearchPlugin {
    fn start(&mut self, ctx: &egui::Context, root: PathBuf) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.confirm = false;
        self.error = None;
        self.selected = None;
        self.collapsed.clear();
        if self.query.is_empty() {
            *self.results.lock().unwrap_or_else(|e| e.into_inner()) = Results::default();
            self.searched = false;
            return;
        }
        let re = match search::build(&self.query, self.options) {
            Ok(re) => re,
            Err(message) => {
                self.error = Some(message);
                return;
            }
        };
        let results = Arc::new(Mutex::new(Results::default()));
        self.results = results.clone();
        self.searched = true;
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(cancel.clone());
        let (mask, ctx) = (self.mask.clone(), ctx.clone());
        thread::spawn(move || {
            let sink = results.clone();
            let looked = search::search_project(&root, &re, &mask, &cancel, MAX_MATCHES, |hits| {
                if let Ok(mut r) = sink.lock() {
                    r.matches += hits.hits.len();
                    r.files.push(hits);
                }
                ctx.request_repaint();
            });
            if let Ok(mut r) = results.lock() {
                r.looked = looked;
                r.files.sort_by(|a, b| a.path.cmp(&b.path));
                r.done = true;
            }
            ctx.request_repaint();
        });
    }

    fn replace_everywhere(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        let re = match search::build(&self.query, self.options) {
            Ok(re) => re,
            Err(message) => {
                self.error = Some(message);
                return;
            }
        };
        // Open documents are written first, so that nothing is lost when files are rewritten.
        if !host.save_files() {
            return;
        }
        self.confirm = false;
        let (root, replacement, options, mask) = (host.root().to_path_buf(), self.replace.clone(), self.options, self.mask.clone());
        let results = self.results.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let done = search::replace_in_project(&root, &re, &replacement, options, &mask);
            if let Ok(mut r) = results.lock() {
                r.replaced = Some(done);
            }
            ctx.request_repaint();
        });
    }

    fn rows(&self, results: &Results) -> Vec<Row> {
        let mut rows = Vec::new();
        for (i, file) in results.files.iter().enumerate() {
            rows.push(Row::File(i));
            if !self.collapsed.contains(&file.path) {
                rows.extend((0..file.hits.len()).map(|h| Row::Hit(i, h)));
            }
        }
        rows
    }
}

impl BuiltinPlugin for SearchPlugin {
    fn core(&self) -> bool {
        true
    }
    fn id(&self) -> &'static str {
        "search"
    }
    fn name(&self) -> &'static str {
        "Recherche dans le projet"
    }
    fn description(&self) -> &'static str {
        "Recherche et remplacement dans tous les fichiers du projet : texte, mots entiers, casse, expressions régulières et masque de fichiers. Les résultats sont groupés par fichier."
    }
    fn icon(&self) -> &'static str {
        "search"
    }
    fn tool_windows(&self) -> &'static [ToolWindow] {
        &TOOL_WINDOWS
    }

    fn update(&mut self, _ctx: &egui::Context, host: &mut dyn Host) {
        let replaced = self.results.lock().ok().and_then(|mut r| r.replaced.take());
        if let Some((files, total)) = replaced {
            host.reload_files();
            host.balloon("Remplacer dans le projet", format!("{total} remplacement(s) dans {files} fichier(s)"), false);
            *self.results.lock().unwrap_or_else(|e| e.into_inner()) = Results::default();
            self.searched = false;
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        use egui::{Key, Modifiers};
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::F)) {
            self.open(host);
        }
    }

    fn commands(&self) -> Vec<Command> {
        vec![Command {
            id: "search.project",
            label: "Rechercher dans le projet…  Ctrl+Maj+F".into(),
        }]
    }

    fn run_command(&mut self, id: &str, host: &mut dyn Host) {
        if id == "search.project" {
            self.open(host);
        }
    }

    fn tool_window(&mut self, _id: &str, ui: &mut egui::Ui, host: &mut dyn Host) {
        let t = ui::theme(ui.ctx());
        let ctx = ui.ctx().clone();
        let mut run = false;
        #[cfg(debug_assertions)]
        if !self.searched
            && let Ok(query) = std::env::var("ATELIER_SHOT_SEARCH")
        {
            self.query = query;
            run = true;
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let chevron = if self.show_replace { "chevronDown" } else { "chevronRight" };
            if ui::icon_toggle(ui, chevron, self.show_replace, "Remplacer").clicked() {
                self.show_replace = !self.show_replace;
            }
            let field = ui::search_field(ui, &mut self.query, "Rechercher dans le projet", 320.0);
            if self.focus {
                field.request_focus();
                self.focus = false;
            }
            if (field.lost_focus() || field.has_focus()) && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                run = true;
                field.request_focus();
            }
            for (label, flag, tip) in [
                ("Aa", &mut self.options.case, "Respecter la casse"),
                ("W", &mut self.options.word, "Mots entiers"),
                (".*", &mut self.options.regex, "Expression régulière"),
            ] {
                let galley = ui.painter().layout_no_wrap(label.into(), ui::semibold(12.0), if *flag { Color32::WHITE } else { t.text_secondary });
                let (rect, response) = ui.allocate_exact_size(egui::vec2((galley.size().x + 12.0).max(26.0), 24.0), egui::Sense::click());
                if *flag {
                    ui.painter().rect_filled(rect, 5.0, t.accent);
                } else if response.hovered() {
                    ui.painter().rect_filled(rect, 5.0, t.hover);
                }
                ui.painter().galley(rect.center() - galley.size() / 2.0, galley, t.text);
                if response.on_hover_text(tip).clicked() {
                    *flag = !*flag;
                    run = true;
                }
            }
            ui.add(egui::TextEdit::singleline(&mut self.mask).hint_text("Masque : *.js, src/*").desired_width(170.0));
            if ui::primary_button(ui, "Rechercher").clicked() {
                run = true;
            }
            if self.cancel.is_some() && !self.results.lock().is_ok_and(|r| r.done) && ui::button(ui, "Arrêter").clicked()
                && let Some(cancel) = &self.cancel
            {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        if self.show_replace {
            ui.horizontal(|ui| {
                ui.add_space(ui::BUTTON + 4.0);
                ui::search_field(ui, &mut self.replace, "Remplacer par", 320.0);
                let found = self.results.lock().map_or(0, |r| r.matches);
                let label = if self.confirm { format!("Confirmer : {found} remplacement(s)") } else { "Tout remplacer".to_string() };
                ui.add_enabled_ui(found > 0, |ui| {
                    let clicked = if self.confirm { ui::primary_button(ui, &label).clicked() } else { ui::button(ui, &label).clicked() };
                    if clicked {
                        if self.confirm {
                            self.replace_everywhere(&ctx, host);
                        } else {
                            self.confirm = true;
                        }
                    }
                });
                if self.confirm {
                    ui.label(RichText::new("Les fichiers du projet seront modifiés.").small().color(t.text_secondary));
                }
            });
        }
        if run {
            self.start(&ctx, host.root().to_path_buf());
        }
        if let Some(error) = &self.error {
            ui.label(RichText::new(error).color(t.error));
        }
        let (rows, summary) = {
            let results = self.results.lock().unwrap_or_else(|e| e.into_inner());
            let summary = if !self.searched {
                String::new()
            } else if results.done {
                format!("{} résultat(s) dans {} fichier(s) · {} fichier(s) examiné(s)", results.matches, results.files.len(), results.looked)
            } else {
                format!("Recherche… {} résultat(s) dans {} fichier(s)", results.matches, results.files.len())
            };
            (self.rows(&results), summary)
        };
        if !summary.is_empty() {
            ui.label(RichText::new(summary).small().color(t.text_secondary));
        }
        let root = host.root().to_path_buf();
        let mut open: Option<(PathBuf, usize, usize, usize)> = None;
        let mut toggle: Option<PathBuf> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_HEIGHT, rows.len(), |ui, range| {
            let results = self.results.lock().unwrap_or_else(|e| e.into_inner());
            ui.spacing_mut().item_spacing.y = 0.0;
            for index in range {
                match &rows[index] {
                    Row::File(f) => {
                        let file = &results.files[*f];
                        let (rect, response) = ui::row(ui, false);
                        let collapsed = self.collapsed.contains(&file.path);
                        ui::paint_chevron(ui, egui::pos2(rect.left() + 12.0, rect.center().y), !collapsed);
                        let relative = file.path.strip_prefix(&root).unwrap_or(&file.path);
                        let name = file.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        let end = ui::paint_text(ui, rect, rect.left() + 26.0, &name, t.text, ui::semibold(12.5));
                        let folder = relative.parent().map(|p| p.display().to_string()).unwrap_or_default();
                        let end = ui::paint_text(ui, rect, end + 8.0, &folder, t.text_secondary, FontId::proportional(12.0));
                        ui::paint_text(ui, rect, end + 8.0, &format!("{}", file.hits.len()), t.text_secondary, FontId::proportional(12.0));
                        if response.clicked() {
                            toggle = Some(file.path.clone());
                        }
                    }
                    Row::Hit(f, h) => {
                        let file = &results.files[*f];
                        let hit = &file.hits[*h];
                        let selected = self.selected.as_ref() == Some(&(file.path.clone(), *h));
                        let (rect, response) = ui::row(ui, selected);
                        let number = format!("{}", hit.line + 1);
                        ui::paint_text(ui, rect, rect.left() + 34.0, &number, t.line_number, FontId::monospace(12.0));
                        // The line, with the match highlighted.
                        let chars: Vec<char> = hit.text.chars().collect();
                        let start = hit.shown_column.min(chars.len());
                        let end = (start + hit.length).min(chars.len());
                        let part = |from: usize, to: usize| chars[from..to].iter().collect::<String>();
                        let mut job = LayoutJob::default();
                        let plain = TextFormat::simple(FontId::monospace(12.5), t.text);
                        job.append(&part(0, start), 0.0, plain.clone());
                        job.append(
                            &part(start, end),
                            0.0,
                            TextFormat {
                                background: Color32::from_rgba_unmultiplied(255, 200, 40, 90),
                                ..plain.clone()
                            },
                        );
                        job.append(&part(end, chars.len()), 0.0, plain);
                        let galley = ui.fonts_mut(|f| f.layout_job(job));
                        let at = egui::pos2(rect.left() + 74.0, rect.center().y - galley.size().y / 2.0);
                        ui.painter().with_clip_rect(rect).galley(at, galley, t.text);
                        if response.clicked() {
                            open = Some((file.path.clone(), hit.line, hit.column, hit.length));
                            self.selected = Some((file.path.clone(), *h));
                        }
                    }
                }
            }
            let _ = Layout::top_down(Align::Min);
        });
        if let Some(path) = toggle
            && !self.collapsed.remove(&path)
        {
            self.collapsed.insert(path);
        }
        if let Some((path, line, column, length)) = open {
            host.open_at(path, line, column, length);
        }
    }
}

impl SearchPlugin {
    fn open(&mut self, host: &mut dyn Host) {
        if let Some(text) = host.selected_text().filter(|t| !t.contains('\n') && t.len() < 200) {
            self.query = text;
        }
        self.focus = true;
        host.show_tool_window(&TOOL_WINDOWS[0]);
    }
}
