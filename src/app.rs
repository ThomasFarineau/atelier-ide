use crate::{
    icons,
    plugins::{self, Plugin},
    project::{self, Document},
    terminal::Terminal,
};
use eframe::egui::{self, Color32, CornerRadius, FontId, Margin, RichText, Stroke};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const ACCENT: Color32 = Color32::from_rgb(150, 120, 255);
const ISLAND_RADIUS: u8 = 10;
const CLOSE_HOVER: Color32 = Color32::from_rgb(196, 43, 28);
/// Modified-file color, as in IntelliJ's project view.
const MODIFIED: Color32 = Color32::from_rgb(108, 153, 235);

/// Islands layout colors: the window background shows through the gaps between panels.
#[derive(Clone, Copy)]
struct Palette {
    background: Color32,
    island: Color32,
    border: Color32,
    hover: Color32,
}

fn palette(theme: usize) -> Palette {
    match theme {
        1 => Palette {
            background: Color32::from_rgb(13, 17, 27),
            island: Color32::from_rgb(24, 30, 44),
            border: Color32::from_rgb(37, 45, 63),
            hover: Color32::from_rgb(38, 46, 66),
        },
        2 => Palette {
            background: Color32::from_rgb(233, 235, 240),
            island: Color32::WHITE,
            border: Color32::from_rgb(218, 221, 228),
            hover: Color32::from_rgb(226, 229, 237),
        },
        _ => Palette {
            background: Color32::from_rgb(19, 20, 23),
            island: Color32::from_rgb(31, 32, 36),
            border: Color32::from_rgb(43, 45, 51),
            hover: Color32::from_rgb(46, 48, 55),
        },
    }
}

/// Frameless buttons with a rounded hover highlight, as in IntelliJ toolbars and tool window headers.
fn flat(ui: &mut egui::Ui) {
    let visuals = &mut ui.style_mut().visuals;
    visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
    ] {
        widget.bg_stroke = Stroke::NONE;
    }
    visuals.selection.bg_fill = visuals.widgets.hovered.weak_bg_fill;
    visuals.selection.stroke.color = visuals.widgets.active.fg_stroke.color;
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct Settings {
    theme: usize,
    font_size: f32,
    recent: Vec<PathBuf>,
    enabled_plugins: BTreeSet<String>,
}

#[derive(Clone)]
enum Pending {
    Close(usize),
    Project(PathBuf),
    Quit,
}

pub struct Ide {
    settings: Settings,
    root: PathBuf,
    documents: Vec<Document>,
    active: usize,
    /// Directory listings, cached until refresh: (path, is_dir).
    tree: BTreeMap<PathBuf, Vec<(PathBuf, bool)>>,
    expanded: BTreeSet<PathBuf>,
    terminal: Option<Terminal>,
    terminal_error: String,
    show_terminal: bool,
    show_project: bool,
    show_settings: bool,
    show_plugins: bool,
    show_palette: bool,
    show_new_file: bool,
    show_find: bool,
    palette_query: String,
    find_query: String,
    new_name: String,
    plugins: Vec<Plugin>,
    plugin_errors: Vec<String>,
    status: String,
    pending: Option<Pending>,
    can_quit: bool,
    cursor: (usize, usize),
    /// Raised by the dev supervisor (`cargo dev`) to swap this process for a new build.
    reload: Option<Arc<AtomicBool>>,
}

/// What survives a dev-mode reload. Unsaved text is kept, so a reload never loses work.
#[derive(Deserialize, Serialize)]
struct Session {
    root: PathBuf,
    documents: Vec<(Option<PathBuf>, String)>,
    active: usize,
    show_terminal: bool,
    show_project: bool,
    #[serde(default)]
    expanded: BTreeSet<PathBuf>,
}

impl Ide {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, "settings"))
            .unwrap_or_default();
        if settings.font_size < 10.0 {
            settings.font_size = 15.0;
        }
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let (plugins, plugin_errors) = plugins::load(&root);
        let mut ide = Self {
            settings,
            root,
            documents: Vec::new(),
            active: 0,
            tree: BTreeMap::new(),
            expanded: BTreeSet::new(),
            terminal: None,
            terminal_error: String::new(),
            show_terminal: false,
            show_project: true,
            show_settings: false,
            show_plugins: false,
            show_palette: false,
            show_new_file: false,
            show_find: false,
            palette_query: String::new(),
            find_query: String::new(),
            new_name: String::new(),
            plugins,
            plugin_errors,
            status: "Prêt".into(),
            pending: None,
            can_quit: false,
            cursor: (1, 1),
            reload: crate::dev::listen(&cc.egui_ctx),
        };
        // Optional project argument: cargo run -- C:\path\to\project
        ide.expanded.insert(ide.root.clone());
        egui_extras::install_image_loaders(&cc.egui_ctx);
        if let Some(path) = std::env::args_os()
            .skip(1)
            .map(PathBuf::from)
            .find(|p| p.is_dir())
        {
            ide.switch_project(path);
        }
        if ide.reload.is_some()
            && let Some(path) = crate::dev::session_path()
        {
            ide.restore_session(&path);
        }
        ide.apply_theme(&cc.egui_ctx);
        ide
    }

    fn apply_theme(&self, ctx: &egui::Context) {
        let p = palette(self.settings.theme);
        let mut visuals = if self.settings.theme == 2 {
            egui::Visuals::light()
        } else {
            egui::Visuals::dark()
        };
        visuals.panel_fill = p.island;
        visuals.window_fill = p.island;
        visuals.window_stroke = Stroke::new(1.0_f32, p.border);
        visuals.window_corner_radius = CornerRadius::same(12);
        visuals.menu_corner_radius = CornerRadius::same(10);
        visuals.extreme_bg_color = p.background;
        visuals.faint_bg_color = p.hover;
        visuals.widgets.noninteractive.bg_stroke.color = p.border;
        visuals.widgets.inactive.weak_bg_fill = p.hover;
        visuals.widgets.inactive.bg_fill = p.hover;
        visuals.widgets.hovered.weak_bg_fill = p.border;
        visuals.widgets.hovered.bg_fill = p.border;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.border);
        for widget in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.corner_radius = CornerRadius::same(6);
        }
        visuals.selection.bg_fill = ACCENT.gamma_multiply(0.45);
        visuals.selection.stroke.color = ACCENT;
        ctx.set_visuals(visuals);
        ctx.style_mut(|style| {
            style.spacing.item_spacing = egui::vec2(9.0, 7.0);
            style.spacing.button_padding = egui::vec2(10.0, 6.0);
            style.text_styles.insert(
                egui::TextStyle::Monospace,
                FontId::monospace(self.settings.font_size),
            );
        });
    }

    fn save_session(&self, path: &Path) -> anyhow::Result<()> {
        let session = Session {
            root: self.root.clone(),
            documents: self
                .documents
                .iter()
                .map(|d| (d.path.clone(), d.text.clone()))
                .collect(),
            active: self.active,
            show_terminal: self.show_terminal,
            show_project: self.show_project,
            expanded: self.expanded.clone(),
        };
        fs::write(path, serde_json::to_vec(&session)?)?;
        Ok(())
    }
    fn restore_session(&mut self, path: &Path) {
        let Ok(bytes) = fs::read(path) else { return };
        let _ = fs::remove_file(path);
        let session: Session = match serde_json::from_slice(&bytes) {
            Ok(session) => session,
            Err(e) => {
                self.status = format!("Erreur : session de rechargement illisible : {e}");
                return;
            }
        };
        if session.root.is_dir() {
            self.switch_project(session.root);
        }
        for (path, text) in session.documents {
            let doc = match path {
                Some(path) => Document::open(path),
                None => Ok(Document::untitled()),
            };
            // A file deleted since the reload is dropped; its unsaved text is gone with it.
            if let Ok(mut doc) = doc {
                doc.text = text;
                self.documents.push(doc);
            }
        }
        self.active = session.active.min(self.documents.len().saturating_sub(1));
        self.show_terminal = session.show_terminal;
        self.show_project = session.show_project;
        if !session.expanded.is_empty() {
            self.expanded = session.expanded;
        }
        self.status = "Rechargé à chaud".into();
    }

    /// Rounded, bordered panel floating on the window background.
    fn island(&self) -> egui::Frame {
        let p = palette(self.settings.theme);
        egui::Frame::new()
            .fill(p.island)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(ISLAND_RADIUS))
            .inner_margin(Margin::same(8))
            .outer_margin(Margin::same(3))
    }

    fn report(&mut self, result: anyhow::Result<()>) -> bool {
        match result {
            Ok(()) => true,
            Err(e) => {
                self.status = format!("Erreur : {e:#}");
                false
            }
        }
    }
    fn open_file(&mut self, path: PathBuf) {
        let path = path.canonicalize().unwrap_or(path);
        if let Some(index) = self
            .documents
            .iter()
            .position(|d| d.path.as_ref() == Some(&path))
        {
            self.active = index;
            return;
        }
        match Document::open(path) {
            Ok(doc) => {
                self.documents.push(doc);
                self.active = self.documents.len() - 1;
                self.status = "Fichier ouvert".into();
            }
            Err(e) => self.status = format!("Erreur : {e:#}"),
        }
    }
    fn choose_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new().set_directory(&self.root).pick_file() {
            self.open_file(path);
        }
    }
    fn choose_project(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_directory(&self.root)
            .pick_folder()
        {
            self.request(Pending::Project(path));
        }
    }
    fn switch_project(&mut self, root: PathBuf) {
        self.root = root.canonicalize().unwrap_or(root);
        self.documents.clear();
        self.active = 0;
        self.tree.clear();
        self.expanded = BTreeSet::from([self.root.clone()]);
        self.terminal = None;
        self.terminal_error.clear();
        self.settings.recent.retain(|p| p != &self.root);
        self.settings.recent.insert(0, self.root.clone());
        self.settings.recent.truncate(6);
        (self.plugins, self.plugin_errors) = plugins::load(&self.root);
        self.status = format!("Projet : {}", self.root.display());
    }
    fn save_document(&mut self, index: usize, save_as: bool) -> bool {
        let Some(doc) = self.documents.get(index) else {
            return true;
        };
        let path = if save_as || doc.path.is_none() {
            rfd::FileDialog::new()
                .set_directory(&self.root)
                .set_file_name(doc.title())
                .save_file()
        } else {
            doc.path.clone()
        };
        let Some(path) = path else { return false };
        let path = path.canonicalize().unwrap_or(path);
        if self
            .documents
            .iter()
            .enumerate()
            .any(|(i, d)| i != index && d.path.as_ref() == Some(&path))
        {
            self.status = "Ce fichier est déjà ouvert dans un autre onglet".into();
            return false;
        }
        let result = self.documents[index].save(path, save_as);
        if self.report(result) {
            self.tree.clear();
            self.status = "Enregistré".into();
            true
        } else {
            false
        }
    }
    fn save_all(&mut self) -> bool {
        for index in 0..self.documents.len() {
            if self.documents[index].dirty() && !self.save_document(index, false) {
                return false;
            }
        }
        true
    }
    fn request(&mut self, action: Pending) {
        let dirty = match action {
            Pending::Close(i) => self.documents.get(i).is_some_and(Document::dirty),
            _ => self.documents.iter().any(Document::dirty),
        };
        if dirty {
            self.pending = Some(action);
        } else {
            self.perform(action);
        }
    }
    fn perform(&mut self, action: Pending) {
        match action {
            Pending::Close(index) => {
                if index < self.documents.len() {
                    self.documents.remove(index);
                    self.active = self
                        .active
                        .saturating_sub(usize::from(index <= self.active))
                        .min(self.documents.len().saturating_sub(1));
                }
            }
            Pending::Project(root) => self.switch_project(root),
            Pending::Quit => self.can_quit = true,
        }
    }
    fn start_terminal(&mut self) {
        match Terminal::start(&self.root) {
            Ok(t) => {
                self.terminal = Some(t);
                self.terminal_error.clear();
            }
            Err(e) => self.terminal_error = format!("Impossible de démarrer le shell : {e:#}"),
        }
    }
    fn execute(&mut self, command: &str) {
        self.show_terminal = true;
        if self.terminal.as_ref().is_none_or(|t| t.exited) {
            self.start_terminal();
        }
        if let Some(t) = &mut self.terminal {
            let result = t.command(command);
            self.report(result);
        }
    }
    fn run_file(&mut self) {
        let Some(doc) = self.documents.get(self.active) else {
            self.status = "Ouvrez un fichier à exécuter".into();
            return;
        };
        if (doc.dirty() || doc.path.is_none()) && !self.save_document(self.active, false) {
            return;
        }
        let doc = &self.documents[self.active];
        let Some(path) = &doc.path else { return };
        let path = path.to_string_lossy().into_owned();
        let command = match doc.language.as_str() {
            "rs" => plugins::shell_command("cargo", &["run".into()]),
            "py" => plugins::shell_command("python", &["-u".into(), path]),
            "java" => plugins::shell_command("java", &[path]),
            "js" => plugins::shell_command("node", &[path]),
            "ts" => plugins::shell_command("tsx", &[path]),
            "html" => {
                #[cfg(windows)]
                let cmd = format!("Start-Process -FilePath '{}'", path.replace('\'', "''"));
                #[cfg(target_os = "macos")]
                let cmd = plugins::shell_command("open", &[path]);
                #[cfg(all(not(windows), not(target_os = "macos")))]
                let cmd = plugins::shell_command("xdg-open", &[path]);
                cmd
            }
            _ => {
                self.status =
                    "CSS : ouvrez la page HTML associée. Ce type de fichier n'est pas exécutable."
                        .into();
                return;
            }
        };
        self.execute(&command);
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let command = egui::Modifiers::COMMAND;
        // egui ignores extra Shift when matching logical shortcuts: most specific first.
        if ctx.input_mut(|i| i.consume_key(command | egui::Modifiers::SHIFT, egui::Key::S)) {
            self.save_document(self.active, true);
        }
        if ctx.input_mut(|i| i.consume_key(command, egui::Key::S)) {
            self.save_document(self.active, false);
        }
        if ctx.input_mut(|i| i.consume_key(command, egui::Key::O)) {
            self.choose_file();
        }
        if ctx.input_mut(|i| i.consume_key(command, egui::Key::N)) {
            self.documents.push(Document::untitled());
            self.active = self.documents.len() - 1;
        }
        if ctx.input_mut(|i| i.consume_key(command, egui::Key::W)) && !self.documents.is_empty() {
            self.request(Pending::Close(self.active));
        }
        if ctx.input_mut(|i| i.consume_key(command, egui::Key::F)) {
            self.show_find = !self.show_find;
        }
        if ctx.input_mut(|i| i.consume_key(command, egui::Key::P)) {
            self.show_palette = !self.show_palette;
            self.palette_query.clear();
        }
        if ctx.input_mut(|i| i.consume_key(command, egui::Key::Backtick)) {
            self.show_terminal = !self.show_terminal;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F5)) {
            self.run_file();
        }
    }

    fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("toolbar")
            .exact_height(44.0)
            .frame(egui::Frame::new().inner_margin(Margin {
                left: 10,
                right: 0,
                top: 0,
                bottom: 0,
            }))
            .show_separator_line(false)
            .show(ctx, |ui| {
                // Empty areas of the bar move the window; widgets added afterwards take priority.
                let bar = ui.interact(
                    ui.max_rect(),
                    egui::Id::new("title_bar"),
                    egui::Sense::click_and_drag(),
                );
                let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                if bar.double_clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                } else if bar.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                let title = match self.documents.get(self.active) {
                    Some(doc) => format!("{}{}", doc.title(), if doc.dirty() { " ●" } else { "" }),
                    None => "Forge IDE".into(),
                };
                ui.painter().text(
                    ui.max_rect().center(),
                    egui::Align2::CENTER_CENTER,
                    title,
                    FontId::proportional(13.0),
                    ui.visuals().weak_text_color(),
                );
                ui.horizontal_centered(|ui| {
                    flat(ui);
                    let project_name = self.root.file_name().unwrap_or_default().to_string_lossy();
                    let initial = project_name
                        .chars()
                        .next()
                        .map_or_else(|| "F".into(), |c| c.to_uppercase().to_string());
                    let (badge, _) =
                        ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
                    ui.painter().rect_filled(badge, 6.0, ACCENT);
                    ui.painter().text(
                        badge.center(),
                        egui::Align2::CENTER_CENTER,
                        initial,
                        FontId::proportional(13.0),
                        Color32::WHITE,
                    );
                    ui.menu_button(
                        RichText::new(format!("{project_name}  ⌄")).strong(),
                        |ui| {
                            if ui.button("Ouvrir un projet…").clicked() {
                                self.choose_project();
                                ui.close();
                            }
                            if ui.button("Ouvrir un fichier…       Ctrl+O").clicked() {
                                self.choose_file();
                                ui.close();
                            }
                            if ui.button("Nouveau fichier        Ctrl+N").clicked() {
                                self.documents.push(Document::untitled());
                                self.active = self.documents.len() - 1;
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("Enregistrer tout").clicked() {
                                self.save_all();
                                ui.close();
                            }
                            if ui.button("Enregistrer sous…").clicked() {
                                self.save_document(self.active, true);
                                ui.close();
                            }
                            ui.separator();
                            for path in self.settings.recent.clone() {
                                if ui.button(path.display().to_string()).clicked() {
                                    self.request(Pending::Project(path));
                                    ui.close();
                                }
                            }
                        },
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.window_controls(ui, maximized);
                        ui.add_space(8.0);
                        if ui
                            .button(RichText::new("⚙").size(17.0))
                            .on_hover_text("Paramètres")
                            .clicked()
                        {
                            self.show_settings = true;
                        }
                        if ui
                            .button(RichText::new("⌕").size(17.0))
                            .on_hover_text("Rechercher partout · Ctrl+P")
                            .clicked()
                        {
                            self.show_palette = true;
                        }
                        ui.add_space(6.0);
                        if ui
                            .button(
                                RichText::new("▶")
                                    .size(16.0)
                                    .color(Color32::from_rgb(95, 200, 130)),
                            )
                            .on_hover_text("Exécuter le fichier actif · F5")
                            .clicked()
                        {
                            self.run_file();
                        }
                    });
                });
            });
    }

    /// Minimize / maximize / close, laid out right to left. Closing goes through the
    /// regular close request, so unsaved changes are still confirmed.
    fn window_controls(&self, ui: &mut egui::Ui, maximized: bool) {
        let ctx = ui.ctx().clone();
        let p = palette(self.settings.theme);
        let size = egui::vec2(46.0, 44.0);
        ui.spacing_mut().item_spacing.x = 0.0;
        let control = |ui: &mut egui::Ui, icon: &str, hover: Color32, tooltip: &str| {
            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
            let hovered = response.hovered();
            if hovered {
                ui.painter().rect_filled(rect, 0.0, hover);
            }
            let color = if hovered && hover == CLOSE_HOVER {
                Color32::WHITE
            } else {
                ui.visuals().text_color()
            };
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                icon,
                FontId::proportional(14.0),
                color,
            );
            response.on_hover_text(tooltip).clicked()
        };
        if control(ui, "✕", CLOSE_HOVER, "Fermer") {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let (icon, tooltip) = if maximized {
            ("❐", "Restaurer")
        } else {
            ("☐", "Agrandir")
        };
        if control(ui, icon, p.hover, tooltip) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }
        if control(ui, "—", p.hover, "Réduire") {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
    }

    /// Without native decorations the OS no longer offers resize borders: emulate them.
    fn resize_edges(ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
            return;
        }
        let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        let rect = ctx.viewport_rect();
        const EDGE: f32 = 5.0;
        let (west, east) = (pos.x - rect.left() < EDGE, rect.right() - pos.x < EDGE);
        let (north, south) = (pos.y - rect.top() < EDGE, rect.bottom() - pos.y < EDGE);
        use egui::{CursorIcon, ResizeDirection as Dir};
        let (direction, cursor) = match (north, south, west, east) {
            (true, _, true, _) => (Dir::NorthWest, CursorIcon::ResizeNorthWest),
            (true, _, _, true) => (Dir::NorthEast, CursorIcon::ResizeNorthEast),
            (_, true, true, _) => (Dir::SouthWest, CursorIcon::ResizeSouthWest),
            (_, true, _, true) => (Dir::SouthEast, CursorIcon::ResizeSouthEast),
            (true, ..) => (Dir::North, CursorIcon::ResizeNorth),
            (_, true, ..) => (Dir::South, CursorIcon::ResizeSouth),
            (_, _, true, _) => (Dir::West, CursorIcon::ResizeWest),
            (_, _, _, true) => (Dir::East, CursorIcon::ResizeEast),
            _ => return,
        };
        ctx.set_cursor_icon(cursor);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }
    }

    fn explorer(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("rail")
            .exact_width(44.0)
            .resizable(false)
            .frame(egui::Frame::new().inner_margin(Margin::symmetric(4, 3)))
            .show_separator_line(false)
            .show(ctx, |ui| {
                flat(ui);
                ui.add_space(4.0);
                if ui
                    .selectable_label(self.show_project, RichText::new("▤").size(22.0))
                    .on_hover_text("Projet")
                    .clicked()
                {
                    self.show_project = !self.show_project;
                }
                if ui
                    .button(RichText::new("⌕").size(22.0))
                    .on_hover_text("Palette · Ctrl+P")
                    .clicked()
                {
                    self.show_palette = true;
                }
                if ui
                    .button(RichText::new("◇").size(22.0))
                    .on_hover_text("Plugins")
                    .clicked()
                {
                    self.show_plugins = true;
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    if ui
                        .selectable_label(self.show_terminal, RichText::new(">_").monospace())
                        .on_hover_text("Terminal · Ctrl+`")
                        .clicked()
                    {
                        self.show_terminal = !self.show_terminal;
                    }
                });
            });
        if !self.show_project {
            return;
        }
        egui::SidePanel::left("project")
            .default_width(255.0)
            .width_range(180.0..=500.0)
            .frame(self.island())
            .show_separator_line(false)
            .show(ctx, |ui| {
                flat(ui);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Projet").strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button("↻")
                            .on_hover_text("Actualiser fichiers et plugins")
                            .clicked()
                        {
                            self.tree.clear();
                            (self.plugins, self.plugin_errors) = plugins::load(&self.root);
                        }
                        if ui
                            .small_button("+")
                            .on_hover_text("Créer un fichier dans le projet")
                            .clicked()
                        {
                            self.show_new_file = true;
                        }
                    });
                });
                ui.add_space(4.0);
                let root = self.root.clone();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if self.tree_row(ui, &root, true, 0).clicked() {
                            self.toggle_folder(&root);
                        }
                        if self.expanded.contains(&root) {
                            self.tree_ui(ui, &root, 1);
                        }
                    });
            });
    }
    fn toggle_folder(&mut self, path: &Path) {
        if !self.expanded.remove(path) {
            self.expanded.insert(path.to_path_buf());
        }
    }
    fn tree_ui(&mut self, ui: &mut egui::Ui, path: &Path, depth: usize) {
        if depth > 16 {
            ui.weak("Profondeur maximale atteinte");
            return;
        }
        if !self.tree.contains_key(path) {
            match project::children(path) {
                Ok(entries) => {
                    let entries = entries
                        .into_iter()
                        .filter_map(|p| {
                            let is_dir = p.is_dir() && !p.is_symlink();
                            (is_dir || p.is_file()).then_some((p, is_dir))
                        })
                        .collect();
                    self.tree.insert(path.into(), entries);
                }
                Err(e) => {
                    ui.colored_label(Color32::LIGHT_RED, format!("Lecture impossible : {e}"));
                    return;
                }
            }
        }
        for (entry, is_dir) in self.tree.get(path).cloned().unwrap_or_default() {
            let response = self.tree_row(ui, &entry, is_dir, depth);
            if is_dir {
                if response.clicked() {
                    self.toggle_folder(&entry);
                }
                if self.expanded.contains(&entry) {
                    self.tree_ui(ui, &entry, depth + 1);
                }
            } else if response
                .on_hover_text(entry.display().to_string())
                .clicked()
            {
                self.open_file(entry);
            }
        }
    }
    /// One explorer line: indent, chevron (folders), SVG icon, name.
    fn tree_row(
        &self,
        ui: &mut egui::Ui,
        path: &Path,
        is_dir: bool,
        depth: usize,
    ) -> egui::Response {
        const ROW_HEIGHT: f32 = 24.0;
        const INDENT: f32 = 16.0;
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), ROW_HEIGHT),
            egui::Sense::click(),
        );
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let open = is_dir && self.expanded.contains(path);
        let document = self
            .documents
            .iter()
            .position(|d| d.path.as_deref() == Some(path));
        let selected = document.is_some_and(|i| i == self.active);
        let dirty = document.is_some_and(|i| self.documents[i].dirty());
        let visuals = ui.visuals();
        let painter = ui.painter_at(rect);
        if selected {
            painter.rect_filled(rect, 6.0, ACCENT.gamma_multiply(0.28));
        } else if response.hovered() {
            painter.rect_filled(rect, 6.0, visuals.widgets.hovered.weak_bg_fill);
        }
        let center_y = rect.center().y;
        let mut x = rect.left() + 4.0 + depth as f32 * INDENT;
        if is_dir {
            let c = egui::pos2(x + 6.0, center_y);
            let points = if open {
                vec![
                    c + egui::vec2(-3.5, -1.75),
                    c + egui::vec2(0.0, 1.75),
                    c + egui::vec2(3.5, -1.75),
                ]
            } else {
                vec![
                    c + egui::vec2(-1.75, -3.5),
                    c + egui::vec2(1.75, 0.0),
                    c + egui::vec2(-1.75, 3.5),
                ]
            };
            painter.add(egui::Shape::line(
                points,
                Stroke::new(1.4_f32, visuals.weak_text_color()),
            ));
        }
        x += 16.0;
        let icon = if is_dir {
            icons::folder(open)
        } else {
            icons::file(path)
        };
        egui::Image::new(icon).paint_at(
            ui,
            egui::Rect::from_min_size(egui::pos2(x, center_y - 8.0), egui::vec2(16.0, 16.0)),
        );
        x += 23.0;
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let is_root = path == self.root;
        let color = if dirty {
            MODIFIED
        } else if is_root || selected {
            visuals.strong_text_color()
        } else {
            visuals.text_color()
        };
        let font = FontId::proportional(13.5);
        if is_root {
            let galley = painter.layout_no_wrap(name.into_owned(), font, color);
            let width = galley.size().x;
            painter.galley(
                egui::pos2(x, center_y - galley.size().y / 2.0),
                galley,
                color,
            );
            // Project path next to its name, as in IntelliJ.
            painter.text(
                egui::pos2(x + width + 8.0, center_y),
                egui::Align2::LEFT_CENTER,
                self.root.display().to_string(),
                FontId::proportional(12.0),
                visuals.weak_text_color(),
            );
        } else {
            painter.text(
                egui::pos2(x, center_y),
                egui::Align2::LEFT_CENTER,
                name,
                font,
                color,
            );
        }
        response
    }

    fn terminal_panel(&mut self, ctx: &egui::Context) {
        if !self.show_terminal {
            return;
        }
        if self.terminal.is_none() && self.terminal_error.is_empty() {
            self.start_terminal();
        }
        egui::TopBottomPanel::bottom("terminal").default_height(260.0).height_range(150.0..=600.0).resizable(true).frame(self.island()).show_separator_line(false).show(ctx, |ui| {
            let mut restart = false;
            ui.horizontal(|ui| {
                flat(ui);
                ui.label(RichText::new("Terminal").strong());
                ui.weak(if cfg!(windows) { "PowerShell" } else { "sh" });
                if ui.small_button("Redémarrer").clicked() { restart = true; }
                if ui.small_button("Ctrl+C").clicked() && let Some(t) = &mut self.terminal {
                    let result = t.write(&[3]); self.report(result);
                }
                if let Some(t) = &mut self.terminal { ui.checkbox(&mut t.raw, "Clavier direct").on_hover_text("Cliquez sur le terminal pour envoyer les touches à un programme interactif. Échap rend le clavier à l'IDE."); }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { if ui.small_button("×").clicked() { self.show_terminal = false; } });
            });
            ui.separator();
            if restart { self.start_terminal(); }
            if !self.terminal_error.is_empty() { ui.colored_label(Color32::LIGHT_RED, &self.terminal_error); }
            let Some(t) = &mut self.terminal else { return };
            let available = ui.available_size();
            t.resize(((available.y - 42.0) / 16.0) as u16, (available.x / 8.0) as u16);
            let screen = t.screen();
            let response = egui::ScrollArea::both().max_height((available.y - 42.0).max(50.0)).show(ui, |ui| {
                ui.add_sized([ui.available_width(), (available.y - 48.0).max(40.0)],
                    egui::Label::new(RichText::new(if screen.is_empty() { "Démarrage du shell…" } else { &screen }).font(FontId::monospace(13.0))).sense(egui::Sense::click()))
            }).inner;
            if t.raw {
                if response.clicked() { response.request_focus(); }
                if response.has_focus() {
                    let events = ctx.input(|i| i.events.clone());
                    for event in events {
                        let bytes = match event {
                            egui::Event::Text(text) | egui::Event::Paste(text) => text.into_bytes(),
                            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                                match key {
                                    egui::Key::Escape => { response.surrender_focus(); Vec::new() },
                                    egui::Key::C if modifiers.ctrl => vec![3],
                                    egui::Key::D if modifiers.ctrl => vec![4],
                                    egui::Key::Enter => vec![13], egui::Key::Backspace => vec![127], egui::Key::Tab => vec![9],
                                    egui::Key::ArrowUp => b"\x1b[A".to_vec(), egui::Key::ArrowDown => b"\x1b[B".to_vec(),
                                    egui::Key::ArrowRight => b"\x1b[C".to_vec(), egui::Key::ArrowLeft => b"\x1b[D".to_vec(),
                                    egui::Key::Home => b"\x1b[H".to_vec(), egui::Key::End => b"\x1b[F".to_vec(),
                                    egui::Key::Delete => b"\x1b[3~".to_vec(), _ => Vec::new(),
                                }
                            }, _ => Vec::new(),
                        };
                        if !bytes.is_empty() && let Err(e) = t.write(&bytes) { self.status = format!("Terminal : {e}"); }
                    }
                }
                ui.weak(if response.has_focus() { "Clavier actif · Échap pour revenir à l'IDE" } else { "Cliquez sur la sortie pour activer le clavier direct" });
            } else {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("❯").color(ACCENT));
                    let input = ui.add_sized([ui.available_width() - 100.0, 26.0], egui::TextEdit::singleline(&mut t.input).font(egui::TextStyle::Monospace).hint_text("Saisir une commande…"));
                    let submit = ui.button("Envoyer").clicked() || (input.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter)));
                    if submit && !t.input.is_empty() {
                        let command = std::mem::take(&mut t.input);
                        if let Err(e) = t.command(&command) { self.status = format!("Terminal : {e}"); }
                        input.request_focus();
                    }
                });
            }
            if t.exited { ui.colored_label(ACCENT, "Session terminée · Redémarrer pour ouvrir un nouveau shell"); }
        });
    }

    fn editor(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(self.island().inner_margin(Margin::symmetric(4, 6)))
            .show(ctx, |ui| {
                if self.documents.is_empty() {
                    self.welcome(ui);
                    return;
                }
                let mut close = None;
                egui::ScrollArea::horizontal()
                    .id_salt("tabs")
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            flat(ui);
                            ui.spacing_mut().item_spacing.x = 2.0;
                            ui.add_space(4.0);
                            for (i, doc) in self.documents.iter().enumerate() {
                                let icon =
                                    icons::file(doc.path.as_deref().unwrap_or(Path::new("")));
                                ui.add(
                                    egui::Image::new(icon)
                                        .fit_to_exact_size(egui::vec2(14.0, 14.0)),
                                );
                                let mut label = RichText::new(format!(
                                    "{}{}",
                                    doc.title(),
                                    if doc.dirty() { " ●" } else { "" }
                                ));
                                if doc.dirty() {
                                    label = label.color(MODIFIED);
                                }
                                if ui.selectable_label(i == self.active, label).clicked() {
                                    self.active = i;
                                }
                                if ui
                                    .small_button("×")
                                    .on_hover_text("Fermer l'onglet")
                                    .clicked()
                                {
                                    close = Some(i);
                                }
                                ui.add_space(6.0);
                            }
                        });
                    });
                if let Some(i) = close {
                    self.request(Pending::Close(i));
                }
                ui.separator();
                let Some(doc) = self.documents.get_mut(self.active) else {
                    return;
                };
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    let breadcrumb = doc
                        .path
                        .as_ref()
                        .map(|p| {
                            p.strip_prefix(&self.root)
                                .unwrap_or(p)
                                .display()
                                .to_string()
                        })
                        .unwrap_or_else(|| "Sans titre".into());
                    ui.weak(breadcrumb.replace('\\', "  /  "));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(&doc.language).color(language_color(&doc.language)));
                    });
                });
                let mut find_next = false;
                if self.show_find {
                    ui.horizontal(|ui| {
                        ui.label("Rechercher");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.find_query)
                                .hint_text("Texte exact…"),
                        );
                        let count = if self.find_query.is_empty() {
                            0
                        } else {
                            doc.text.matches(&self.find_query).count()
                        };
                        ui.weak(format!("{count} résultat(s)"));
                        find_next = ui.button("Suivant ↓").clicked();
                        if ui.small_button("×").clicked() {
                            self.show_find = false;
                        }
                    });
                }
                ui.separator();
                let theme =
                    egui_extras::syntax_highlighting::CodeTheme::from_memory(ctx, ui.style());
                let language = if doc.language == "ts" {
                    "js"
                } else {
                    &doc.language
                };
                let mut layouter =
                    |ui: &egui::Ui, buffer: &dyn egui::TextBuffer, _wrap_width: f32| {
                        let mut job = egui_extras::syntax_highlighting::highlight(
                            ui.ctx(),
                            ui.style(),
                            &theme,
                            buffer.as_str(),
                            language,
                        );
                        for section in &mut job.sections {
                            section.format.font_id = FontId::monospace(self.settings.font_size);
                        }
                        job.wrap.max_width = f32::INFINITY;
                        ui.fonts_mut(|fonts| fonts.layout_job(job))
                    };
                let editor_id = egui::Id::new(("source", doc.path.clone(), self.active));
                egui::ScrollArea::both()
                    .id_salt(("source_scroll", self.active))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.horizontal_top(|ui| {
                            ui.add_space(8.0);
                            let line_count = doc.text.lines().count().max(1)
                                + usize::from(doc.text.ends_with('\n'));
                            let numbers = (1..=line_count)
                                .map(|n| format!("{n:>4}"))
                                .collect::<Vec<_>>()
                                .join("\n");
                            ui.label(
                                RichText::new(numbers)
                                    .font(FontId::monospace(self.settings.font_size))
                                    .color(ui.visuals().weak_text_color()),
                            );
                            ui.separator();
                            let mut output = egui::TextEdit::multiline(&mut doc.text)
                                .id(editor_id)
                                .font(egui::TextStyle::Monospace)
                                .code_editor()
                                .desired_width(ui.available_width().max(800.0))
                                .desired_rows(40)
                                .lock_focus(true)
                                .layouter(&mut layouter)
                                .frame(false)
                                .show(ui);
                            if let Some(range) = output.cursor_range {
                                let index = range.primary.index;
                                let before: String = doc.text.chars().take(index).collect();
                                self.cursor = (
                                    before.chars().filter(|&c| c == '\n').count() + 1,
                                    before.rsplit('\n').next().unwrap_or("").chars().count() + 1,
                                );
                            }
                            if find_next && !self.find_query.is_empty() {
                                let current =
                                    output.cursor_range.map(|r| r.primary.index).unwrap_or(0);
                                let offset = doc
                                    .text
                                    .char_indices()
                                    .nth(current)
                                    .map(|(i, _)| i)
                                    .unwrap_or(doc.text.len());
                                let found = doc.text[offset..]
                                    .find(&self.find_query)
                                    .map(|i| i + offset)
                                    .or_else(|| doc.text.find(&self.find_query));
                                if let Some(byte) = found {
                                    let start = doc.text[..byte].chars().count();
                                    let end = start + self.find_query.chars().count();
                                    output.state.cursor.set_char_range(Some(
                                        egui::text::CCursorRange::two(
                                            egui::text::CCursor::new(start),
                                            egui::text::CCursor::new(end),
                                        ),
                                    ));
                                    output.state.store(ctx, editor_id);
                                    output.response.request_focus();
                                }
                            }
                        });
                    });
            });
    }

    fn welcome(&mut self, ui: &mut egui::Ui) {
        ui.add_space(80.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("F").size(76.0).strong().color(ACCENT));
            ui.add_space(8.0);
            ui.label(
                RichText::new("Votre prochain projet commence ici.")
                    .size(27.0)
                    .strong(),
            );
            ui.add_space(10.0);
            ui.weak("Un espace de travail natif. Sept langages. Toutes vos idées.");
            ui.add_space(30.0);
            if ui
                .add_sized(
                    [240.0, 42.0],
                    egui::Button::new("Ouvrir un projet…").fill(ACCENT.gamma_multiply(0.35)),
                )
                .clicked()
            {
                self.choose_project();
            }
            if ui
                .add_sized(
                    [240.0, 36.0],
                    egui::Button::new("Ouvrir un fichier     Ctrl+O"),
                )
                .clicked()
            {
                self.choose_file();
            }
            ui.add_space(30.0);
            ui.label(
                RichText::new("HTML   CSS   JavaScript   TypeScript   Java   Python   Rust")
                    .monospace()
                    .color(ACCENT),
            );
            ui.add_space(30.0);
            for path in self.settings.recent.clone() {
                if ui.link(path.display().to_string()).clicked() {
                    self.request(Pending::Project(path));
                }
            }
            ui.add_space(30.0);
            ui.weak(
                "Ctrl+P  Palette de commandes     ·     F5  Exécuter     ·     Ctrl+`  Terminal",
            );
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        egui::Window::new("Paramètres").open(&mut open).default_width(440.0).show(ctx, |ui| {
            ui.heading("Apparence");
            ui.add_space(10.0);
            let mut changed = false;
            for (index, name) in ["Forge Dark", "Midnight", "IntelliJ Light"].iter().enumerate() { changed |= ui.radio_value(&mut self.settings.theme, index, *name).changed(); }
            ui.separator();
            changed |= ui.add(egui::Slider::new(&mut self.settings.font_size, 11.0..=24.0).text("Taille de l'éditeur")).changed();
            if changed { self.apply_theme(ctx); }
            ui.separator();
            ui.label("Raccourcis");
            egui::Grid::new("shortcut_list").striped(true).show(ui, |ui| {
                for (key, action) in [("Ctrl+O", "Ouvrir un fichier"), ("Ctrl+N", "Nouveau fichier"), ("Ctrl+S", "Enregistrer"), ("Ctrl+Shift+S", "Enregistrer sous"), ("Ctrl+W", "Fermer l'onglet"), ("Ctrl+F", "Rechercher dans le fichier"), ("Ctrl+P", "Palette de commandes"), ("Ctrl+`", "Afficher le terminal"), ("F5", "Exécuter le fichier actif")] { ui.monospace(key); ui.label(action); ui.end_row(); }
            });
            ui.separator();
            ui.weak("Les outils d'exécution doivent être installés : cargo, python, java, node, tsx.");
        });
        self.show_settings = open;
        let mut open = self.show_plugins;
        let mut command = None;
        egui::Window::new("Plugins locaux").open(&mut open).default_width(540.0).show(ctx, |ui| {
            ui.label("Extensions du projet");
            ui.weak("Manifeste JSON dans .forge/plugins/. Activez uniquement des plugins de confiance : leurs commandes utilisent vos droits locaux.");
            if ui.button("Recharger").clicked() { (self.plugins, self.plugin_errors) = plugins::load(&self.root); }
            ui.separator();
            egui::ScrollArea::vertical().max_height(450.0).show(ui, |ui| {
                if self.plugins.is_empty() { ui.weak("Aucun plugin. Voir examples/plugins/rust-tools.json pour un exemple."); }
                for plugin in &self.plugins {
                    let key = self.plugin_key(&plugin.id);
                    ui.horizontal(|ui| {
                        let mut enabled = self.settings.enabled_plugins.contains(&key);
                        if ui.checkbox(&mut enabled, &plugin.name).changed() {
                            if enabled { self.settings.enabled_plugins.insert(key.clone()); } else { self.settings.enabled_plugins.remove(&key); }
                        }
                        ui.weak(&plugin.version);
                    });
                    ui.label(&plugin.description);
                    if self.settings.enabled_plugins.contains(&self.plugin_key(&plugin.id)) {
                        for cmd in &plugin.commands {
                            let shell = plugins::shell_command(&cmd.program, &cmd.args);
                            if ui.button(&cmd.name).on_hover_text(&shell).clicked() { command = Some(shell); }
                        }
                    }
                    ui.separator();
                }
                for error in &self.plugin_errors { ui.colored_label(Color32::LIGHT_RED, error); }
            });
        });
        self.show_plugins = open;
        if let Some(cmd) = command {
            self.execute(&cmd);
        }

        if self.show_new_file {
            egui::Window::new("Créer un fichier")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label("Chemin relatif au projet");
                    ui.text_edit_singleline(&mut self.new_name);
                    ui.weak("Exemple : src/main.rs · Les dossiers manquants seront créés.");
                    ui.horizontal(|ui| {
                        if ui.button("Créer").clicked() {
                            let result = (|| -> anyhow::Result<PathBuf> {
                                let path = project::new_file_path(&self.root, &self.new_name)?;
                                if let Some(parent) = path.parent() {
                                    fs::create_dir_all(parent)?;
                                }
                                fs::OpenOptions::new()
                                    .write(true)
                                    .create_new(true)
                                    .open(&path)?;
                                Ok(path)
                            })();
                            match result {
                                Ok(path) => {
                                    self.open_file(path);
                                    self.tree.clear();
                                    self.show_new_file = false;
                                    self.new_name.clear();
                                }
                                Err(e) => self.status = format!("Erreur : {e:#}"),
                            }
                        }
                        if ui.button("Annuler").clicked() {
                            self.show_new_file = false;
                        }
                    });
                });
        }
        self.palette(ctx);
        if let Some(action) = self.pending.clone() {
            egui::Modal::new(egui::Id::new("unsaved")).show(ctx, |ui| {
                ui.set_width(440.0);
                ui.heading("Modifications non enregistrées");
                ui.label("Enregistrer vos fichiers avant de continuer ?");
                for (i, doc) in self.documents.iter().enumerate() {
                    if doc.dirty() && (!matches!(action, Pending::Close(index) if index != i)) {
                        ui.label(format!("● {}", doc.title()));
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Enregistrer").clicked() {
                        let saved = match action {
                            Pending::Close(i) => self.save_document(i, false),
                            _ => self.save_all(),
                        };
                        if saved {
                            self.pending = None;
                            self.perform(action.clone());
                        }
                    }
                    if ui.button("Abandonner").clicked() {
                        self.pending = None;
                        self.perform(action.clone());
                    }
                    if ui.button("Annuler").clicked() {
                        self.pending = None;
                    }
                });
            });
        }
    }
    fn plugin_key(&self, id: &str) -> String {
        format!("{}::{id}", self.root.display())
    }

    fn palette(&mut self, ctx: &egui::Context) {
        if !self.show_palette {
            return;
        }
        let mut selected = None;
        egui::Modal::new(egui::Id::new("palette")).show(ctx, |ui| {
            ui.set_width(580.0);
            ui.heading("Palette de commandes");
            let input = ui.add(
                egui::TextEdit::singleline(&mut self.palette_query)
                    .hint_text("Rechercher une action ou un fichier ouvert…")
                    .desired_width(f32::INFINITY),
            );
            if !input.has_focus() {
                input.request_focus();
            }
            let query = self.palette_query.to_lowercase();
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .show(ui, |ui| {
                    for (id, label) in [
                        (0, "Ouvrir un projet"),
                        (1, "Ouvrir un fichier"),
                        (2, "Enregistrer tout"),
                        (3, "Terminal : afficher / masquer"),
                        (4, "Paramètres : thème et police"),
                        (5, "Plugins locaux"),
                        (6, "Exécuter le fichier actif"),
                        (7, "Actualiser le projet"),
                    ] {
                        if label.to_lowercase().contains(&query)
                            && ui.selectable_label(false, label).clicked()
                        {
                            selected = Some(id);
                        }
                    }
                    for (i, doc) in self.documents.iter().enumerate() {
                        if doc.title().to_lowercase().contains(&query)
                            && ui
                                .selectable_label(false, format!("Fichier : {}", doc.title()))
                                .clicked()
                        {
                            selected = Some(100 + i);
                        }
                    }
                });
            if ui.button("Fermer · Échap").clicked()
                || ctx.input(|i| i.key_pressed(egui::Key::Escape))
            {
                self.show_palette = false;
            }
        });
        if let Some(id) = selected {
            self.show_palette = false;
            match id {
                0 => self.choose_project(),
                1 => self.choose_file(),
                2 => {
                    self.save_all();
                }
                3 => self.show_terminal = !self.show_terminal,
                4 => self.show_settings = true,
                5 => self.show_plugins = true,
                6 => self.run_file(),
                7 => self.tree.clear(),
                index => self.active = index - 100,
            }
        }
    }
}

impl eframe::App for Ide {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self
            .reload
            .as_ref()
            .is_some_and(|r| r.swap(false, Ordering::SeqCst))
        {
            // Unsaved work goes into the session, so quitting without the dialog is safe.
            let result = crate::dev::session_path().map_or(Ok(()), |p| self.save_session(&p));
            if self.report(result) {
                self.pending = None;
                self.can_quit = true;
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.can_quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request(Pending::Quit);
        }
        if self.can_quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.pending.is_none()
            && !self.show_palette
            && !self
                .terminal
                .as_ref()
                .is_some_and(|t| t.raw && self.show_terminal)
        {
            self.shortcuts(ctx);
        }
        if let Some(terminal) = &mut self.terminal {
            terminal.poll();
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        Self::resize_edges(ctx);
        self.toolbar(ctx);
        egui::TopBottomPanel::bottom("status")
            .exact_height(26.0)
            .frame(egui::Frame::new().inner_margin(Margin::symmetric(6, 0)))
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(8.0);
                    ui.label(RichText::new(&self.status).small().color(
                        if self.status.starts_with("Erreur") {
                            Color32::LIGHT_RED
                        } else {
                            ui.visuals().weak_text_color()
                        },
                    ));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("UTF-8").small());
                        ui.label(
                            RichText::new(format!("Ln {}, Col {}", self.cursor.0, self.cursor.1))
                                .small(),
                        );
                        if let Some(doc) = self.documents.get(self.active) {
                            ui.label(
                                RichText::new(doc.language.to_uppercase())
                                    .small()
                                    .color(language_color(&doc.language)),
                            );
                        }
                    });
                });
            });
        self.explorer(ctx);
        // Keeps the same background gap on the right edge as between islands.
        egui::SidePanel::right("gutter")
            .exact_width(5.0)
            .resizable(false)
            .frame(egui::Frame::NONE)
            .show_separator_line(false)
            .show(ctx, |_| {});
        self.terminal_panel(ctx);
        self.editor(ctx);
        self.dialogs(ctx);
    }
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        palette(self.settings.theme)
            .background
            .to_normalized_gamma_f32()
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "settings", &self.settings);
    }
}

fn language_color(language: &str) -> Color32 {
    match language {
        "html" => Color32::from_rgb(230, 142, 99),
        "css" => Color32::from_rgb(164, 139, 240),
        "js" => Color32::from_rgb(226, 199, 109),
        "ts" => Color32::from_rgb(103, 169, 231),
        "java" => Color32::from_rgb(219, 155, 109),
        "py" => Color32::from_rgb(117, 192, 173),
        "rs" => Color32::from_rgb(221, 144, 120),
        _ => Color32::from_rgb(157, 164, 180),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ide() -> Ide {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .unwrap();
        Ide {
            settings: Settings {
                font_size: 15.0,
                ..Default::default()
            },
            root,
            documents: Vec::new(),
            active: 0,
            tree: BTreeMap::new(),
            expanded: BTreeSet::new(),
            terminal: None,
            terminal_error: String::new(),
            show_terminal: false,
            show_project: true,
            show_settings: false,
            show_plugins: false,
            show_palette: false,
            show_new_file: false,
            show_find: false,
            palette_query: String::new(),
            find_query: String::new(),
            new_name: String::new(),
            plugins: Vec::new(),
            plugin_errors: Vec::new(),
            status: "Prêt".into(),
            pending: None,
            can_quit: false,
            cursor: (1, 1),
            reload: None,
        }
    }

    #[test]
    fn dirty_documents_block_close_project_switch_and_exit() {
        let mut ide = ide();
        let mut doc = Document::untitled();
        doc.text = "unsaved".into();
        ide.documents.push(doc);
        ide.request(Pending::Close(0));
        assert!(matches!(ide.pending, Some(Pending::Close(0))));
        assert_eq!(ide.documents.len(), 1);
        let original = ide.root.clone();
        ide.request(Pending::Project(original.join("examples/demo")));
        assert!(matches!(ide.pending, Some(Pending::Project(_))));
        assert_eq!(ide.root, original);
        ide.request(Pending::Quit);
        assert!(matches!(ide.pending, Some(Pending::Quit)));
        assert!(!ide.can_quit);
    }

    #[test]
    fn dev_reload_keeps_project_tabs_and_unsaved_text() {
        let mut ide = ide();
        ide.open_file(ide.root.join("examples/demo/hello.py"));
        ide.open_file(ide.root.join("examples/demo/app.js"));
        ide.documents[0].text.push_str(
            "# pas encore enregistré
",
        );
        ide.documents.push(Document::untitled());
        ide.documents[2].text = "brouillon".into();
        ide.active = 1;
        ide.show_terminal = true;
        let path = std::env::temp_dir().join(format!("forge-session-{}.json", std::process::id()));
        ide.save_session(&path).unwrap();

        let mut restored = super::tests::ide();
        restored.root = std::env::temp_dir();
        restored.restore_session(&path);
        assert_eq!(restored.root, ide.root);
        assert_eq!(restored.documents.len(), 3);
        assert!(restored.documents[0].dirty());
        assert!(restored.documents[0].text.ends_with(
            "# pas encore enregistré
"
        ));
        assert!(!restored.documents[1].dirty());
        assert_eq!(restored.documents[2].text, "brouillon");
        assert_eq!(restored.active, 1);
        assert!(restored.show_terminal);
        assert!(!path.exists(), "the session file is consumed on restore");
    }

    #[test]
    fn opening_the_same_file_reuses_its_tab() {
        let mut ide = ide();
        let path = ide.root.join("examples/demo/src/main.rs");
        ide.open_file(path.clone());
        ide.open_file(path);
        assert_eq!(ide.documents.len(), 1);
        assert_eq!(ide.documents[0].language, "rs");
        ide.request(Pending::Close(0));
        assert!(ide.documents.is_empty());
        assert_eq!(ide.active, 0);
    }

    #[test]
    fn all_languages_and_themes_render_without_panicking() {
        let mut ide = ide();
        let ctx = egui::Context::default();
        for file in [
            "index.html",
            "style.css",
            "app.js",
            "hello.ts",
            "Main.java",
            "hello.py",
            "src/main.rs",
        ] {
            ide.open_file(ide.root.join("examples/demo").join(file));
        }
        assert_eq!(ide.documents.len(), 7);
        for theme in 0..3 {
            ide.settings.theme = theme;
            ide.apply_theme(&ctx);
            for active in 0..7 {
                ide.active = active;
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1440.0, 920.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        ide.toolbar(ctx);
                        ide.explorer(ctx);
                        ide.editor(ctx);
                        ide.dialogs(ctx);
                    },
                );
                let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
                assert!(!primitives.is_empty());
            }
        }
    }
}
