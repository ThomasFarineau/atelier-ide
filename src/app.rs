use crate::{
    icons,
    plugins::{
        self, Plugin,
        builtin::{self, BuiltinPlugin, Dock, EditorLayout, Host, ToolWindow},
    },
    project::{self, Document},
    terminal::{Shell, Terminal},
    terminal_layout::Node,
    ui::{self, Theme},
};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Margin, Rect, RichText, Sense, Stroke, StrokeKind,
    containers::menu::{MenuButton, MenuConfig},
    pos2, vec2,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

mod plugins_page;

const TITLE_HEIGHT: f32 = 40.0;
const STATUS_HEIGHT: f32 = 28.0;
const STRIPE_WIDTH: f32 = 40.0;
const HEADER_HEIGHT: f32 = 38.0;
const ISLAND_RADIUS: u8 = 10;
const CLOSE_HOVER: Color32 = Color32::from_rgb(0xE8, 0x11, 0x23);

/// The IDE's own tool windows, next to those of the built-in plugins.
static PROJECT_TOOL: ToolWindow = ToolWindow {
    id: "project",
    title: "Projet",
    icon: "project",
    dock: Dock::Left,
    shortcut: "Alt+1",
};
static TERMINAL_TOOL: ToolWindow = ToolWindow {
    id: "terminal",
    title: "Terminal",
    icon: "terminal",
    dock: Dock::Bottom,
    shortcut: "Ctrl+`",
};
static NOTIFICATIONS_TOOL: ToolWindow = ToolWindow {
    id: "notifications",
    title: "Notifications",
    icon: "notifications",
    dock: Dock::Right,
    shortcut: "",
};

/// Actions of Search Everywhere and of the main menu: (label, shortcut, icon).
const ACTIONS: [(&str, &str, &str); 11] = [
    ("Ouvrir un projet…", "", "project"),
    ("Ouvrir un fichier…", "Ctrl+O", "openNewTab"),
    ("Tout enregistrer", "Ctrl+S", "save"),
    ("Terminal", "Ctrl+`", "terminal"),
    ("Paramètres…", "Ctrl+Alt+S", "settings"),
    ("Plugins…", "", "settings"),
    ("Exécuter le fichier actif", "F5", "run"),
    ("Actualiser le projet", "", "refresh"),
    (
        "Ouvrir un projet dans une nouvelle fenêtre…",
        "",
        "openNewTab",
    ),
    ("Nouveau fichier dans le projet…", "", "add"),
    ("Notifications", "", "notifications"),
];

const SHORTCUTS: [(&str, &str); 36] = [
    ("Ctrl+O", "Ouvrir un fichier"),
    ("Ctrl+N", "Nouveau fichier"),
    ("Ctrl+S", "Enregistrer"),
    ("Ctrl+Shift+S", "Enregistrer sous"),
    ("Ctrl+W", "Fermer l'onglet"),
    ("Ctrl+F", "Rechercher dans le fichier"),
    ("Double Maj / Ctrl+P", "Rechercher partout"),
    ("Ctrl+Shift+N", "Aller au fichier"),
    ("Ctrl+Alt+S", "Paramètres"),
    ("Alt+1", "Fenêtre Projet"),
    ("Ctrl+`", "Terminal"),
    ("F5", "Exécuter le fichier actif"),
    ("Ctrl+Espace", "Complétion du code"),
    ("Maj+F6", "Renommer"),
    ("Ctrl+F / Ctrl+R / F3", "Rechercher / Remplacer / occurrence suivante"),
    ("Ctrl+Maj+F", "Rechercher dans le projet"),
    ("Ctrl+clic / Ctrl+B", "Aller à la déclaration (ou aux utilisations)"),
    ("Ctrl+Maj+B / Ctrl+Alt+B", "Déclaration du type / implémentations"),
    ("Alt+F7", "Utilisations"),
    ("Ctrl+F12", "Structure du fichier"),
    ("Alt+Entrée", "Actions rapides (importer…)"),
    ("Ctrl+Maj+M", "Parenthèse ou accolade correspondante"),
    ("Ctrl+E", "Fichiers récents"),
    ("Ctrl+G", "Aller à la ligne"),
    ("Ctrl+- / Ctrl+=", "Replier / déplier le code"),
    ("Ctrl+Maj+- / Ctrl+Maj+=", "Tout replier / tout déplier"),
    ("Ctrl+Alt+Gauche", "Retour à l'emplacement précédent"),
    ("Ctrl+Alt+Droite", "Emplacement suivant"),
    ("Ctrl+K", "Commit"),
    ("Ctrl+Shift+K", "Pousser"),
    ("Ctrl+T", "Mettre à jour le projet"),
    ("Alt+0", "Fenêtre Commit"),
    ("Alt+9", "Fenêtre Git"),
    ("Alt+8", "Fenêtre Kubernetes"),
    ("F7 / Maj+F7", "Différence suivante / précédente"),
    ("Échap", "Rendre le clavier à l'IDE (terminal)"),
];

/// What to select once a document is open.
#[derive(Clone, Debug)]
enum NavTarget {
    Place(crate::languages::Location),
    Offset(usize),
}

/// A place in a file: where the caret was.
#[derive(Clone, Debug, PartialEq)]
struct NavPoint {
    path: PathBuf,
    offset: usize,
}

/// The place typed in "go to line": `12` or `12:5`, 1-based.
fn line_hit(query: &str) -> Option<Hit> {
    let (line, column) = query.split_once(':').unwrap_or((query, ""));
    let line: usize = line.trim().parse().ok().filter(|&l| l > 0)?;
    let column: usize = column.trim().parse().ok().filter(|&c| c > 0).unwrap_or(1);
    Some(Hit {
        label: format!("Ligne {line}, colonne {column}"),
        hint: "Entrée pour y aller".into(),
        icon: Some("search"),
        file: None,
        target: Target::Line(line - 1, column - 1),
    })
}

/// Identifies a document for the language service: by its path, or by its tab when untitled.
fn doc_key(path: &Option<PathBuf>, index: usize) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    match path {
        Some(path) => path.hash(&mut hasher),
        None => ("untitled", index).hash(&mut hasher),
    }
    hasher.finish()
}

/// Colors of the project badge, picked from the project name as IntelliJ does.
const PROJECT_COLORS: [Color32; 8] = [
    Color32::from_rgb(0x25, 0x9F, 0xB3),
    Color32::from_rgb(0x8F, 0x6C, 0xE0),
    Color32::from_rgb(0xD9, 0x7A, 0x3F),
    Color32::from_rgb(0x4C, 0x9F, 0x58),
    Color32::from_rgb(0xD0, 0x5C, 0x7C),
    Color32::from_rgb(0x3F, 0x7F, 0xE0),
    Color32::from_rgb(0xB8, 0x9B, 0x2E),
    Color32::from_rgb(0x5E, 0x8C, 0x9E),
];

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct Settings {
    theme: usize,
    font_size: f32,
    recent: Vec<PathBuf>,
    enabled_plugins: BTreeSet<String>,
    /// Built-in plugins are enabled unless listed here.
    disabled_builtins: BTreeSet<String>,
    /// Ctrl + mouse wheel changes the editor font size unless this is set.
    wheel_zoom_off: bool,
    /// What the user answered when offered to connect an AI to the MCP server of a project,
    /// by `project|ai`.
    mcp_choices: BTreeMap<String, bool>,
}

#[derive(Clone)]
enum Pending {
    Close(usize),
    Project(PathBuf),
    Quit,
}

struct Note {
    title: String,
    text: String,
    error: bool,
    at: Instant,
}

#[derive(Clone)]
enum Target {
    /// A 0-based line and column of the active document.
    Line(usize, usize),
    Action(usize),
    Builtin(&'static str, &'static str),
    Tab(usize),
    File(PathBuf),
}

/// What the Search Everywhere popup is asked.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PaletteMode {
    Everywhere,
    Recent,
    Line,
}

struct Hit {
    label: String,
    hint: String,
    icon: Option<&'static str>,
    file: Option<PathBuf>,
    target: Target,
}

pub struct Ide {
    settings: Settings,
    root: PathBuf,
    documents: Vec<Document>,
    active: usize,
    /// Directory listings, cached until refresh: (path, is_dir).
    tree: BTreeMap<PathBuf, Vec<(PathBuf, bool)>>,
    /// Modification time of each listed folder when it was read: a change means the folder
    /// gained or lost entries.
    tree_stamps: BTreeMap<PathBuf, Option<std::time::SystemTime>>,
    expanded: BTreeSet<PathBuf>,
    tree_selected: Option<PathBuf>,
    /// Every selected entry when several are (Ctrl or Shift click); empty otherwise.
    tree_multi: BTreeSet<PathBuf>,
    /// Where a Shift selection starts.
    tree_anchor: Option<PathBuf>,
    /// What a drag in the tree carries: the selection, when the grabbed row is part of it.
    tree_drag_set: Vec<PathBuf>,
    terminals: Vec<TerminalTab>,
    active_terminal: usize,
    next_pane: u64,
    drag: Option<Drag>,
    /// Shells found on this machine, listed the first time the menu opens.
    shells: Option<Vec<Shell>>,
    terminal_error: String,
    last_watch: Instant,
    show_terminal: bool,
    show_project: bool,
    show_notifications: bool,
    show_settings: bool,
    settings_page: usize,
    plugin_filter: String,
    /// When the font size last changed with the wheel, to show it briefly.
    zoom_hint: Option<Instant>,
    plugin_selected: Option<String>,
    show_palette: bool,
    /// The title the window has now, to send it again only when it changes.
    window_title: String,
    /// An AI started in a terminal (pane id) that the user is asked to connect to the MCP server.
    mcp_offer: Option<(u64, crate::plugins::builtins::mcp::agents::Agent)>,
    /// What the command line (`atelier <path>`) reaches this IDE through.
    cli: Option<crate::cli::Server>,
    /// The context graph of the project: kept in `.atelier/graph` while the user leaves it on.
    graph: crate::graph::Service,
    palette_mode: PaletteMode,
    /// Files in the order they were last used, the most recent first.
    recent_files: Vec<PathBuf>,
    show_new_file: bool,
    find: crate::find::Find,
    /// A find action asked for outside the bar (F3).
    find_request: Option<crate::find::Action>,
    palette_query: String,
    palette_index: usize,
    /// Files of the project for Search Everywhere, listed when it opens.
    project_files: Option<Vec<PathBuf>>,
    new_name: String,
    tree_dialog: Option<TreeDialog>,
    /// File or folder dragged in the tree, and the folder it is over (this frame, last frame).
    tree_drag: Option<PathBuf>,
    tree_drop: Option<PathBuf>,
    tree_drop_next: Option<PathBuf>,
    /// Folder the dragged entry hovers and since when: it opens after a short delay.
    tree_hover: Option<(PathBuf, Instant)>,
    scroll_to_selected: bool,
    doc_drag: Option<usize>,
    /// Rectangle of each tree row drawn last, for the tests that drive the mouse.
    #[cfg(test)]
    row_rects: Vec<(PathBuf, Rect)>,
    /// Text to put in the clipboard at the next frame.
    pending_copy: Option<String>,
    plugins: Vec<Plugin>,
    plugin_errors: Vec<String>,
    status: String,
    logged_status: String,
    notes: Vec<Note>,
    /// Notifications shown as balloons, by index in `notes`.
    balloons: Vec<usize>,
    pending: Option<Pending>,
    /// Project waiting for the "this window / new window" choice.
    project_choice: Option<PathBuf>,
    can_quit: bool,
    cursor: (usize, usize),
    /// Language intelligence (completion, problems, tooltips) for the open project.
    languages: Arc<crate::languages::Service>,
    /// Wakes the interface when the language service has an answer.
    repaint: Arc<dyn Fn() + Send + Sync>,
    /// Errors and warnings of the active document.
    problems: (usize, usize),
    /// Places left by Ctrl+click and the like, to come back to (Ctrl+Alt+Left) and return to.
    nav_back: Vec<NavPoint>,
    nav_forward: Vec<NavPoint>,
    /// A place to select once its document is loaded.
    pending_nav: Option<(PathBuf, NavTarget)>,
    /// Raised by the dev supervisor (`cargo dev`) to swap this process for a new build.
    reload: Option<Arc<AtomicBool>>,
    builtins: Vec<Box<dyn BuiltinPlugin>>,
    /// Built-in tool window shown instead of the project view.
    left_tool: Option<&'static ToolWindow>,
    /// Built-in tool window shown instead of the terminal.
    bottom_tool: Option<&'static ToolWindow>,
    /// Tool window that has the focus (blue stripe button); None for the editor.
    focus: Option<&'static str>,
    panel_rects: Vec<(Option<&'static str>, Rect)>,
    shift_down: bool,
    last_shift: Option<Instant>,
    /// Source editor scroll position (0 to 1), and its last change for the preview to follow.
    editor_scroll: f32,
    scroll_sync: Option<f32>,
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
    /// Open tool windows, by id.
    #[serde(default)]
    tools: Vec<String>,
}

fn initials(name: &str) -> String {
    let words: Vec<&str> = name
        .split(['-', '_', ' ', '.'])
        .filter(|w| !w.is_empty())
        .collect();
    let letters: String = match words.as_slice() {
        [] => "F".into(),
        [one] => one.chars().take(2).collect(),
        [first, second, ..] => first
            .chars()
            .take(1)
            .chain(second.chars().take(1))
            .collect(),
    };
    letters.to_uppercase()
}

fn project_color(name: &str) -> Color32 {
    let hash = name
        .bytes()
        .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
    PROJECT_COLORS[hash as usize % PROJECT_COLORS.len()]
}

fn ago(at: Instant) -> String {
    match at.elapsed().as_secs() {
        0..60 => "à l'instant".into(),
        s @ 60..3600 => format!("il y a {} min", s / 60),
        s => format!("il y a {} h", s / 3600),
    }
}

/// Project files for Search Everywhere: hidden folders of the tree are skipped.
fn list_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(children) = project::searchable_children(&dir) else {
            continue;
        };
        for child in children {
            if child.is_dir() {
                if !child.is_symlink() {
                    stack.push(child);
                }
            } else {
                files.push(child);
            }
            if files.len() >= 20_000 {
                return files;
            }
        }
    }
    files
}

impl Ide {
    fn blank(root: PathBuf, settings: Settings, reload: Option<Arc<AtomicBool>>) -> Self {
        let (plugins, plugin_errors) = plugins::load(&root);
        let repaint: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
        Self {
            languages: Arc::new(crate::languages::Service::new(&root, repaint.clone())),
            repaint,
            problems: (0, 0),
            nav_back: Vec::new(),
            nav_forward: Vec::new(),
            pending_nav: None,
            settings,
            expanded: BTreeSet::from([root.clone()]),
            root,
            documents: Vec::new(),
            active: 0,
            tree: BTreeMap::new(),
            tree_stamps: BTreeMap::new(),
            tree_selected: None,
            tree_multi: BTreeSet::new(),
            tree_anchor: None,
            tree_drag_set: Vec::new(),
            terminals: Vec::new(),
            active_terminal: 0,
            next_pane: 0,
            drag: None,
            shells: None,
            terminal_error: String::new(),
            last_watch: Instant::now(),
            show_terminal: false,
            show_project: true,
            show_notifications: false,
            show_settings: false,
            settings_page: 0,
            plugin_filter: String::new(),
            zoom_hint: None,
            plugin_selected: None,
            show_palette: false,
            window_title: String::new(),
            mcp_offer: None,
            cli: None,
            graph: crate::graph::Service::default(),
            palette_mode: PaletteMode::Everywhere,
            recent_files: Vec::new(),
            show_new_file: false,
            find: crate::find::Find::default(),
            find_request: None,
            palette_query: String::new(),
            palette_index: 0,
            project_files: None,
            new_name: String::new(),
            tree_dialog: None,
            tree_drag: None,
            tree_drop: None,
            tree_drop_next: None,
            tree_hover: None,
            scroll_to_selected: false,
            doc_drag: None,
            #[cfg(test)]
            row_rects: Vec::new(),
            pending_copy: None,
            plugins,
            plugin_errors,
            status: "Prêt".into(),
            logged_status: "Prêt".into(),
            notes: Vec::new(),
            balloons: Vec::new(),
            pending: None,
            project_choice: None,
            can_quit: false,
            cursor: (1, 1),
            reload,
            builtins: builtin::all(),
            left_tool: None,
            bottom_tool: None,
            focus: None,
            panel_rects: Vec::new(),
            shift_down: false,
            last_shift: None,
            editor_scroll: 0.0,
            scroll_sync: None,
        }
    }

    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, "settings"))
            .unwrap_or_default();
        if settings.font_size < 10.0 {
            settings.font_size = 14.0;
        }
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut ide = Self::blank(root, settings, crate::dev::listen(&cc.egui_ctx));
        let ctx = cc.egui_ctx.clone();
        ide.repaint = Arc::new(move || ctx.request_repaint());
        ide.languages = Arc::new(crate::languages::Service::new(&ide.root, ide.repaint.clone()));
        egui_extras::install_image_loaders(&cc.egui_ctx);
        // `atelier <folder|file>`: a folder is the project, a file opens in the current one.
        let target = crate::cli::target();
        if let Some(path) = target.clone().filter(|p| p.is_dir()) {
            ide.switch_project(path);
        }
        if ide.reload.is_some()
            && let Some(path) = crate::dev::session_path()
        {
            ide.restore_session(&path);
        }
        if let Some(file) = target.filter(|p| p.is_file()) {
            ide.open_file(file);
        }
        ide.cli = crate::cli::serve(&ide.root, ide.repaint.clone());
        ide.graph.set_root(&ide.root, ide.repaint.clone());
        crate::stats::start();
        ide.apply_theme(&cc.egui_ctx);
        ide
    }

    /// Opens what another `atelier` process asked for, and brings the window forward.
    fn serve_command_line(&mut self, ctx: &egui::Context) {
        let in_front = ctx.input(|i| i.focused);
        let requests = match &mut self.cli {
            Some(server) => {
                server.update(&self.root, in_front);
                server.poll()
            }
            None => return,
        };
        for path in requests {
            if path.is_dir() {
                self.open_project(path);
            } else {
                self.open_file(path);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    fn apply_theme(&self, ctx: &egui::Context) {
        ui::install(
            ctx,
            ui::theme_by_index(self.settings.theme),
            self.settings.font_size,
        );
    }

    fn theme(&self, ctx: &egui::Context) -> Theme {
        ui::theme(ctx)
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
            tools: self
                .all_tools()
                .into_iter()
                .filter(|t| self.tool_visible(t))
                .map(|t| t.id.to_string())
                .collect(),
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
                Some(path) => Document::open(path.canonicalize().unwrap_or(path)),
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
            self.expanded = session
                .expanded
                .into_iter()
                .map(|p| p.canonicalize().unwrap_or(p))
                .collect();
        }
        for id in &session.tools {
            if let Some(tool) = self.all_tools().into_iter().find(|t| t.id == id) {
                self.open_tool(tool);
            }
        }
        self.status = "Rechargé à chaud".into();
    }

    /// Rounded panel floating on the window background, as in IntelliJ's Islands theme.
    fn island(&self, ctx: &egui::Context) -> egui::Frame {
        egui::Frame::new()
            .fill(self.theme(ctx).island)
            .corner_radius(CornerRadius::same(ISLAND_RADIUS))
            .outer_margin(Margin {
                left: 2,
                right: 3,
                top: 2,
                bottom: 3,
            })
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

    // ---- Notifications ------------------------------------------------------------------

    fn push_note(&mut self, title: &str, text: String, error: bool, balloon: bool) {
        self.notes.push(Note {
            title: title.into(),
            text,
            error,
            at: Instant::now(),
        });
        if self.notes.len() > 200 {
            self.notes.remove(0);
            self.balloons.retain(|&i| i > 0);
            for index in &mut self.balloons {
                *index -= 1;
            }
        }
        if balloon {
            self.balloons.push(self.notes.len() - 1);
        }
    }

    /// Status messages go to the notification log; errors also show a balloon.
    fn log_status(&mut self) {
        if self.status == self.logged_status {
            return;
        }
        self.logged_status = self.status.clone();
        let message = self.status.clone();
        match message.strip_prefix("Erreur : ") {
            Some(error) => self.push_note("Erreur", error.to_string(), true, true),
            None => self.push_note("Atelier IDE", message, false, false),
        }
    }

    // ---- Files and projects -------------------------------------------------------------

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
        let binary = self
            .builtins
            .iter()
            .filter(|p| !self.settings.disabled_builtins.contains(p.id()))
            .any(|p| p.opens_binary(&path));
        let opened = if binary {
            Ok(Document::binary(path))
        } else {
            Document::open_async(path)
        };
        match opened {
            Ok(doc) => {
                self.documents.push(doc);
                self.active = self.documents.len() - 1;
                self.status = "Fichier ouvert".into();
            }
            Err(e) => self.status = format!("Erreur : {e:#}"),
        }
    }
    /// Once a second, reloads the unmodified documents that another program changed.
    fn watch_files(&mut self, ctx: &egui::Context) {
        const EVERY: Duration = Duration::from_secs(1);
        if self.last_watch.elapsed() >= EVERY {
            self.last_watch = Instant::now();
            let mut errors = Vec::new();
            for doc in &mut self.documents {
                if !doc.dirty() && doc.changed_on_disk()
                    && let Err(e) = doc.reload()
                {
                    errors.push(format!("{} : {e:#}", doc.title()));
                }
            }
            if let Some(error) = errors.first() {
                self.status = format!("Erreur : rechargement de {error}");
            }
            self.refresh_tree();
        }
        ctx.request_repaint_after(EVERY);
    }
    /// Forgets the listing of the folders that changed on disk, so the tree reads them again:
    /// files created, deleted or renamed by the terminal, Git or any other program show up.
    fn refresh_tree(&mut self) {
        let stale: Vec<PathBuf> = self
            .tree
            .keys()
            .filter(|dir| self.tree_stamps.get(*dir) != Some(&project::modified(dir)))
            .cloned()
            .collect();
        if stale.is_empty() {
            return;
        }
        for dir in &stale {
            self.tree.remove(dir);
            self.tree_stamps.remove(dir);
        }
        // Quick-open lists the files of the project too.
        self.project_files = None;
        // What disappeared leaves the selection.
        self.tree_multi.retain(|p| p.exists());
        if self.tree_selected.as_deref().is_some_and(|p| !p.exists()) {
            self.tree_selected = None;
        }
    }
    /// Collects files read in the background; a file that cannot be read is closed.
    fn poll_loading(&mut self, ctx: &egui::Context) {
        let mut failed = Vec::new();
        for (index, doc) in self.documents.iter_mut().enumerate() {
            if let Some(Err(e)) = doc.poll() {
                failed.push((index, format!("Erreur : {e:#}")));
            }
        }
        for (index, message) in failed.into_iter().rev() {
            self.documents.remove(index);
            if self.active >= index {
                self.active = self.active.saturating_sub(1);
            }
            self.status = message;
        }
        if self.documents.iter().any(|d| d.loading.is_some()) {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }
    #[cfg(test)]
    fn wait_loaded(&mut self) {
        while self.documents.iter().any(|d| d.loading.is_some()) {
            for doc in &mut self.documents {
                let _ = doc.poll();
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn choose_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new().set_directory(&self.root).pick_file() {
            self.open_file(path);
        }
    }
    fn choose_project(&mut self, new_window: bool) {
        if let Some(path) = rfd::FileDialog::new()
            .set_directory(&self.root)
            .pick_folder()
        {
            if new_window {
                self.open_window(path);
            } else {
                self.open_project(path);
            }
        }
    }
    /// Opens a project, asking first whether to replace this one or use a new window.
    fn open_project(&mut self, path: PathBuf) {
        let path = path.canonicalize().unwrap_or(path);
        if path != self.root {
            self.project_choice = Some(path);
        }
    }
    /// Each project window is its own process, as IntelliJ on Windows.
    fn open_window(&mut self, path: PathBuf) {
        let result = std::env::current_exe().and_then(|exe| {
            std::process::Command::new(exe)
                .arg("--new-window")
                .arg(project::simplify(&path))
                .current_dir(project::simplify(&path))
                .env_remove(crate::dev::CHILD_ENV)
                .env_remove(crate::dev::SESSION_ENV)
                .spawn()
        });
        match result {
            Ok(_) => {
                self.remember(&path);
                self.status = format!(
                    "Projet ouvert dans une nouvelle fenêtre : {}",
                    path.display()
                );
            }
            Err(e) => self.status = format!("Erreur : nouvelle fenêtre impossible : {e}"),
        }
    }
    fn remember(&mut self, path: &Path) {
        self.settings.recent.retain(|p| p != path);
        self.settings.recent.insert(0, path.to_path_buf());
        self.settings.recent.truncate(10);
    }
    fn switch_project(&mut self, root: PathBuf) {
        self.root = root.canonicalize().unwrap_or(root);
        self.documents.clear();
        self.active = 0;
        self.tree.clear();
        self.tree_selected = None;
        self.tree_multi.clear();
        self.project_files = None;
        self.expanded = BTreeSet::from([self.root.clone()]);
        self.terminals.clear();
        self.terminal_error.clear();
        let root = self.root.clone();
        self.remember(&root);
        (self.plugins, self.plugin_errors) = plugins::load(&self.root);
        self.languages = Arc::new(crate::languages::Service::new(&self.root, self.repaint.clone()));
        self.problems = (0, 0);
        self.graph.set_root(&self.root, self.repaint.clone());
        self.status = format!("Projet : {}", self.root.display());
    }
    fn new_document(&mut self) {
        self.documents.push(Document::untitled());
        self.active = self.documents.len() - 1;
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
        let result = self.documents[index].save(path.clone(), save_as);
        if self.report(result) {
            for plugin in &mut self.builtins {
                plugin.file_saved(&path);
            }
            self.tree.clear();
            self.graph.file_changed();
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
                    self.languages.close(doc_key(&self.documents[index].path, index));
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
    fn new_pane(&mut self, shell: &Shell) -> Option<Pane> {
        match Terminal::start(&project::simplify(&self.root), shell) {
            Ok(mut term) => {
                term.want_focus = true;
                self.terminal_error.clear();
                self.next_pane += 1;
                Some(Pane {
                    id: self.next_pane,
                    term,
                    selection: Default::default(),
                })
            }
            Err(e) => {
                self.terminal_error = format!("Impossible de démarrer {} : {e:#}", shell.name);
                None
            }
        }
    }
    /// Opens a new terminal tab running `shell`, or the default shell.
    fn start_terminal(&mut self, shell: Option<Shell>) {
        // A project of a WSL distribution gets a shell inside that distribution.
        let shell = shell.unwrap_or_else(|| {
            crate::wsl::parse(&self.root).map_or_else(Shell::default_shell, |w| Shell::wsl(&w.distro))
        });
        if let Some(pane) = self.new_pane(&shell) {
            self.terminals.push(TerminalTab {
                active: pane.id,
                root: Node::Leaf(pane),
            });
            self.active_terminal = self.terminals.len() - 1;
        }
    }
    /// Starts the active pane's shell again, in the same place.
    fn restart_terminal(&mut self) {
        let Some(shell) = self
            .terminals
            .get_mut(self.active_terminal)
            .and_then(|tab| tab.pane())
            .map(|pane| pane.term.shell.clone())
        else {
            return self.start_terminal(None);
        };
        if let Some(mut pane) = self.new_pane(&shell)
            && let Some(tab) = self.terminals.get_mut(self.active_terminal)
            && let Some(slot) = tab.pane()
        {
            pane.id = slot.id;
            *slot = pane;
        }
    }
    /// Splits the active pane with a new one running the same shell.
    fn split_terminal(&mut self, stacked: bool) {
        let Some(shell) = self
            .terminals
            .get_mut(self.active_terminal)
            .and_then(|tab| tab.pane())
            .map(|pane| pane.term.shell.clone())
        else {
            return;
        };
        if let Some(pane) = self.new_pane(&shell) {
            let tab = &mut self.terminals[self.active_terminal];
            let (target, new) = (tab.active, pane.id);
            if tab.root.split(&|p: &Pane| p.id == target, stacked, pane) {
                tab.active = new;
            }
        }
    }
    /// Sends the Enter that was held back when an AI was typed in a pane.
    fn release_enter(&mut self, pane: u64) {
        let found = self.terminals.iter_mut().flat_map(|tab| tab.root.leaves_mut()).find(|p| p.id == pane);
        if let Some(pane) = found {
            let _ = pane.term.write(b"\r");
        }
    }

    /// The user started an AI in a terminal: connects it to the MCP server of the IDE if they
    /// said so before, asks if they were never asked, and lets it start in the other cases.
    fn offer_mcp(&mut self, pane: u64, agent: crate::plugins::builtins::mcp::agents::Agent) {
        use crate::plugins::builtins::mcp::agents;
        let key = format!("{}|{}", self.root.display(), agent.id());
        if agents::is_connected(agent, &self.root, crate::plugins::builtins::mcp::address().as_deref()) {
            return self.release_enter(pane);
        }
        match self.settings.mcp_choices.get(&key) {
            Some(true) => {
                self.connect_mcp(agent);
                self.release_enter(pane);
            }
            Some(false) => self.release_enter(pane),
            None => self.mcp_offer = Some((pane, agent)),
        }
    }

    fn connect_mcp(&mut self, agent: crate::plugins::builtins::mcp::agents::Agent) {
        match crate::plugins::builtins::mcp::agents::connect(agent, &self.root, crate::plugins::builtins::mcp::address().as_deref()) {
            Ok(_) => self.status = format!("{} connecté au MCP d'Atelier", agent.name()),
            Err(e) => self.push_note("MCP", e, true, true),
        }
    }

    /// Splits pane `target` on the side it was dropped on, with a new pane running its shell.
    fn split_terminal_at(&mut self, target: u64, stacked: bool, before: bool) {
        let Some(shell) = self
            .terminals
            .get(self.active_terminal)
            .and_then(|tab| tab.root.leaves().into_iter().find(|p| p.id == target))
            .map(|pane| pane.term.shell.clone())
        else {
            return;
        };
        if let Some(mut pane) = self.new_pane(&shell) {
            pane.term.want_focus = true;
            let new = pane.id;
            let tab = &mut self.terminals[self.active_terminal];
            if tab.root.split_with(&|p: &Pane| p.id == target, stacked, before, Node::Leaf(pane)) {
                tab.active = new;
            }
        }
    }

    /// Closes the active pane, or the tab when it is the last one.
    fn close_pane(&mut self) {
        let index = self.active_terminal;
        match self.terminals.get_mut(index) {
            Some(tab) if tab.count() > 1 => {
                let target = tab.active;
                tab.root.remove(&|p: &Pane| p.id == target);
                if let Some(first) = tab.root.leaves_mut().into_iter().next() {
                    first.term.want_focus = true;
                    tab.active = first.id;
                }
            }
            _ => self.close_terminal(index),
        }
    }
    fn close_terminal(&mut self, index: usize) {
        if index < self.terminals.len() {
            self.terminals.remove(index);
        }
        if self.terminals.is_empty() {
            self.show_terminal = false;
        }
        self.active_terminal = self.active_terminal.min(self.terminals.len().saturating_sub(1));
    }
    fn execute(&mut self, command: &str) {
        self.set_terminal(true);
        let alive = self
            .terminals
            .get_mut(self.active_terminal)
            .and_then(|tab| tab.pane())
            .is_some_and(|pane| !pane.term.exited);
        if !alive {
            self.start_terminal(None);
        }
        if let Some(pane) = self
            .terminals
            .get_mut(self.active_terminal)
            .and_then(|tab| tab.pane())
        {
            let result = pane.term.command(command);
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
        if let Some(wsl) = crate::wsl::parse(path) {
            let language = doc.language.clone();
            return self.run_in_wsl(&language, wsl);
        }
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
                    "Erreur : ce type de fichier n'est pas exécutable (pour le CSS, ouvrez la page HTML associée)"
                        .into();
                return;
            }
        };
        self.execute(&command);
    }
    /// Runs a file of a WSL distribution in a terminal of that distribution, with Linux paths.
    fn run_in_wsl(&mut self, language: &str, file: crate::wsl::WslPath) {
        let path = file.linux.clone();
        let command = match language {
            "rs" => crate::wsl::shell_command("cargo", &["run".into()]),
            "py" => crate::wsl::shell_command("python3", &["-u".into(), path]),
            "java" => crate::wsl::shell_command("java", &[path]),
            "js" => crate::wsl::shell_command("node", &[path]),
            "ts" => crate::wsl::shell_command("tsx", &[path]),
            "html" => format!(
                "explorer.exe \"$(wslpath -w {})\"",
                crate::wsl::quote(&path)
            ),
            _ => {
                self.status =
                    "Erreur : ce type de fichier n'est pas exécutable (pour le CSS, ouvrez la page HTML associée)"
                        .into();
                return;
            }
        };
        let in_wsl = self
            .terminals
            .get_mut(self.active_terminal)
            .and_then(|tab| tab.pane())
            .is_some_and(|pane| !pane.term.exited && crate::terminal::is_wsl(&pane.term.shell));
        if !in_wsl {
            self.start_terminal(Some(Shell::wsl(&file.distro)));
        }
        self.execute(&command);
    }
    /// Goes to a place chosen in the editor (Ctrl+click, Ctrl+B): `from` and `offset` are where
    /// the caret was, to come back to.
    fn go_to(&mut self, target: crate::languages::Location, from: Option<PathBuf>, offset: usize) {
        let Some(path) = target.path.clone().or_else(|| from.clone()) else {
            // An unsaved document: the place is in the active one.
            if let Some(doc) = self.documents.get_mut(self.active) {
                let range = target.range_in(&doc.text);
                doc.view.select(range.start, range.end);
            }
            return;
        };
        if let Some(from) = from {
            self.nav_back.push(NavPoint { path: from, offset });
            let excess = self.nav_back.len().saturating_sub(100);
            self.nav_back.drain(..excess);
            self.nav_forward.clear();
        }
        self.pending_nav = Some((path, NavTarget::Place(target)));
        self.apply_pending_nav();
    }

    /// Puts the caret of the active document on a 0-based line and column.
    fn go_to_line(&mut self, line: usize, column: usize) {
        let Some(doc) = self.documents.get_mut(self.active) else {
            return;
        };
        let from = doc.path.clone().map(|path| NavPoint { path, offset: doc.view.cursor });
        let offset = doc.view.offset_at(&doc.text, line, column);
        doc.view.select(offset, offset);
        if let Some(from) = from {
            self.nav_back.push(from);
            self.nav_forward.clear();
        }
    }

    /// Opens the file of a pending place and selects it once the document is read.
    fn apply_pending_nav(&mut self) {
        let Some((path, target)) = self.pending_nav.clone() else {
            return;
        };
        self.open_file(path.clone());
        let canonical = path.canonicalize().unwrap_or(path);
        let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| d.path.as_ref() == Some(&canonical))
        else {
            self.pending_nav = None;
            return;
        };
        if doc.loading.is_some() {
            return;
        }
        let range = match target {
            NavTarget::Place(place) => place.range_in(&doc.text),
            NavTarget::Offset(offset) => offset.min(doc.text.len())..offset.min(doc.text.len()),
        };
        doc.view.select(range.start, range.end);
        self.pending_nav = None;
    }

    /// Back or forward through the places left by jumps.
    fn navigate_history(&mut self, back: bool) {
        let here = self
            .documents
            .get(self.active)
            .and_then(|d| Some(NavPoint { path: d.path.clone()?, offset: d.view.cursor }));
        let (from, to) = if back {
            (&mut self.nav_back, &mut self.nav_forward)
        } else {
            (&mut self.nav_forward, &mut self.nav_back)
        };
        let Some(point) = from.pop() else {
            return;
        };
        if let Some(here) = here {
            to.push(here);
        }
        self.pending_nav = Some((point.path, NavTarget::Offset(point.offset)));
        self.apply_pending_nav();
    }

    /// Applies the changes of a rename to the other files: the open documents change (and
    /// stay to be saved), the others are written.
    fn apply_file_edits(&mut self, files: Vec<(PathBuf, Vec<crate::languages::TextEdit>)>) {
        let apply = |text: &str, edits: &[crate::languages::TextEdit]| {
            let mut new = text.to_string();
            let mut sorted: Vec<_> = edits.to_vec();
            sorted.sort_by_key(|e| std::cmp::Reverse(e.range.start));
            for edit in sorted {
                let end = edit.range.end.min(new.len());
                let start = edit.range.start.min(end);
                if new.is_char_boundary(start) && new.is_char_boundary(end) {
                    new.replace_range(start..end, &edit.new_text);
                }
            }
            new
        };
        let mut changed = 0;
        for (path, edits) in files {
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            if let Some(doc) = self.documents.iter_mut().find(|d| d.path.as_ref() == Some(&canonical)) {
                let new = apply(&doc.text, &edits);
                doc.view.replace_text(&mut doc.text, &new);
                changed += 1;
            } else if let Ok(text) = fs::read_to_string(&path)
                && fs::write(&path, apply(&text, &edits)).is_ok()
            {
                changed += 1;
            }
        }
        self.status = format!("Renommé, avec {changed} autre(s) fichier(s) modifié(s)");
    }

    /// The selection of the active document, for the find bar and the search window.
    fn selected_text(&self) -> Option<String> {
        let doc = self.documents.get(self.active)?;
        let range = doc.view.selection();
        (!range.is_empty() && range.end <= doc.text.len()).then(|| doc.text.get(range).map(str::to_string)).flatten()
    }

    fn open_palette(&mut self) {
        self.open_palette_as(PaletteMode::Everywhere);
    }

    fn open_palette_as(&mut self, mode: PaletteMode) {
        self.palette_mode = mode;
        self.show_palette = true;
        self.palette_query.clear();
        self.palette_index = 0;
        self.project_files = None;
    }

    fn perform_action(&mut self, index: usize) {
        match index {
            0 => self.choose_project(false),
            1 => self.choose_file(),
            2 => {
                self.save_all();
            }
            3 => self.set_terminal(!self.show_terminal),
            4 => {
                self.show_settings = true;
                self.settings_page = 0;
            }
            5 => {
                self.show_settings = true;
                self.settings_page = 3;
            }
            6 => self.run_file(),
            7 => {
                self.tree.clear();
                (self.plugins, self.plugin_errors) = plugins::load(&self.root);
            }
            8 => self.choose_project(true),
            9 => self.show_new_file = true,
            _ => self.show_notifications = !self.show_notifications,
        }
    }

    // ---- Shortcuts ----------------------------------------------------------------------

    fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers};
        let command = Modifiers::COMMAND;
        let pressed = |modifiers, key| ctx.input_mut(|i| i.consume_key(modifiers, key));
        // egui ignores extra Shift when matching logical shortcuts: most specific first.
        if pressed(command | Modifiers::SHIFT, Key::S) {
            self.save_document(self.active, true);
        }
        if pressed(command | Modifiers::ALT, Key::S) {
            self.show_settings = true;
        }
        if pressed(command, Key::S) {
            self.save_document(self.active, false);
        }
        if pressed(command, Key::O) {
            self.choose_file();
        }
        if pressed(command | Modifiers::ALT, Key::ArrowLeft) {
            self.navigate_history(true);
        }
        if pressed(command | Modifiers::ALT, Key::ArrowRight) {
            self.navigate_history(false);
        }
        if pressed(command | Modifiers::SHIFT, Key::N) {
            self.open_palette();
        }
        if pressed(command, Key::N) {
            self.new_document();
        }
        if pressed(command, Key::W) && !self.documents.is_empty() {
            self.request(Pending::Close(self.active));
        }
        if pressed(command, Key::F) {
            let selected = self.selected_text();
            self.find.show(selected, false);
        }
        if pressed(command, Key::R) {
            let selected = self.selected_text();
            self.find.show(selected, true);
        }
        if pressed(Modifiers::SHIFT, Key::F3) {
            self.find_request = Some(crate::find::Action::Previous);
        }
        if pressed(Modifiers::NONE, Key::F3) {
            self.find_request = Some(crate::find::Action::Next);
        }
        if pressed(command, Key::P) {
            self.open_palette();
        }
        if pressed(command, Key::E) {
            self.open_palette_as(PaletteMode::Recent);
        }
        if pressed(command, Key::G) && !self.documents.is_empty() {
            self.open_palette_as(PaletteMode::Line);
        }
        if pressed(command, Key::Backtick) {
            self.set_terminal(!self.show_terminal);
        }
        if pressed(Modifiers::ALT, Key::Num1) {
            self.toggle_tool(&PROJECT_TOOL);
        }
        if pressed(Modifiers::NONE, Key::F5) {
            self.run_file();
        }
        // Double Shift opens Search Everywhere.
        let (shift, other) = ctx.input(|i| {
            (
                i.modifiers.shift && !i.modifiers.ctrl && !i.modifiers.alt,
                i.events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Key { .. } | egui::Event::Text(_))),
            )
        });
        if other {
            self.last_shift = None;
        } else if shift && !self.shift_down {
            if self
                .last_shift
                .is_some_and(|t| t.elapsed() < Duration::from_millis(450))
            {
                self.open_palette();
                self.last_shift = None;
            } else {
                self.last_shift = Some(Instant::now());
            }
        }
        self.shift_down = shift;
        self.each_builtin(|plugin, ide| plugin.shortcuts(ctx, ide));
    }

    // ---- Title bar ----------------------------------------------------------------------

    fn toolbar(&mut self, ctx: &egui::Context) {
        let t = self.theme(ctx);
        egui::TopBottomPanel::top("toolbar")
            .exact_height(TITLE_HEIGHT)
            .frame(egui::Frame::new().fill(t.frame))
            .show_separator_line(false)
            .show(ctx, |ui| {
                let bar_rect = ui.max_rect();
                // Empty areas of the bar move the window; widgets added afterwards take priority.
                let bar = ui.interact(
                    bar_rect,
                    egui::Id::new("title_bar"),
                    Sense::click_and_drag(),
                );
                let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                if bar.double_clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                } else if bar.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                let name = self
                    .root
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Atelier".into());
                let color = project_color(&name);
                // Project color fading over the left of the bar, as in IntelliJ.
                let mut mesh = egui::Mesh::default();
                let fade = Rect::from_min_size(bar_rect.min, vec2(620.0, bar_rect.height()));
                let (from, to) = (color.gamma_multiply(0.24), Color32::TRANSPARENT);
                for (pos, color) in [
                    (fade.left_top(), from),
                    (fade.right_top(), to),
                    (fade.right_bottom(), to),
                    (fade.left_bottom(), from),
                ] {
                    mesh.colored_vertex(pos, color);
                }
                mesh.add_triangle(0, 1, 2);
                mesh.add_triangle(0, 2, 3);
                ui.painter().add(egui::Shape::mesh(mesh));
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.add_space(8.0);
                    self.main_menu(ui);
                    ui.add_space(2.0);
                    self.project_widget(ui, &name, color);
                    self.each_builtin(|plugin, ide| plugin.toolbar(ui, ide));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        self.window_controls(ui, maximized);
                        ui.add_space(6.0);
                        if ui::icon_button(ui, "settings", "Paramètres · Ctrl+Alt+S").clicked() {
                            self.show_settings = true;
                        }
                        if ui::icon_button(ui, "search", "Rechercher partout · Double Maj")
                            .clicked()
                        {
                            self.open_palette();
                        }
                        ui.add_space(10.0);
                        MenuButton::from_button(
                            egui::Button::image(ui::icon(ui, "moreVertical")).frame(false),
                        )
                        .ui(ui, |ui| {
                            for (index, (label, shortcut, _)) in ACTIONS.iter().enumerate() {
                                if ui
                                    .add(egui::Button::new(*label).shortcut_text(*shortcut))
                                    .clicked()
                                {
                                    self.perform_action(index);
                                    ui.close();
                                }
                            }
                        });
                        ui.add_enabled_ui(false, |ui| {
                            ui::icon_button(ui, "debug", "Débogueur : non disponible");
                        });
                        if ui::icon_button(ui, "run", "Exécuter le fichier actif · F5").clicked()
                        {
                            self.run_file();
                        }
                        self.run_widget(ui);
                    });
                });
            });
    }

    fn main_menu(&mut self, ui: &mut egui::Ui) {
        let config = MenuConfig::new();
        MenuButton::from_button(egui::Button::image(ui::icon(ui, "menu")).frame(false))
            .config(config)
            .ui(ui, |ui| {
                ui.set_min_width(220.0);
                let item = |ui: &mut egui::Ui, label: &str, shortcut: &str| {
                    ui.add(egui::Button::new(label).shortcut_text(shortcut))
                        .clicked()
                };
                ui.menu_button("Fichier", |ui| {
                    if item(ui, "Nouveau fichier", "Ctrl+N") {
                        self.new_document();
                        ui.close();
                    }
                    if item(ui, "Nouveau fichier dans le projet…", "") {
                        self.show_new_file = true;
                        ui.close();
                    }
                    ui.separator();
                    if item(ui, "Ouvrir un fichier…", "Ctrl+O") {
                        self.choose_file();
                        ui.close();
                    }
                    if item(ui, "Ouvrir un projet…", "") {
                        self.choose_project(false);
                        ui.close();
                    }
                    if item(ui, "Ouvrir un projet dans une nouvelle fenêtre…", "") {
                        self.choose_project(true);
                        ui.close();
                    }
                    ui.menu_button("Projets récents", |ui| {
                        for path in self.settings.recent.clone() {
                            if path != self.root && item(ui, &path.display().to_string(), "") {
                                self.open_project(path);
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    if item(ui, "Enregistrer", "Ctrl+S") {
                        self.save_document(self.active, false);
                        ui.close();
                    }
                    if item(ui, "Enregistrer sous…", "Ctrl+Shift+S") {
                        self.save_document(self.active, true);
                        ui.close();
                    }
                    if item(ui, "Tout enregistrer", "") {
                        self.save_all();
                        ui.close();
                    }
                    ui.separator();
                    if item(ui, "Paramètres…", "Ctrl+Alt+S") {
                        self.show_settings = true;
                        ui.close();
                    }
                    ui.separator();
                    if item(ui, "Quitter", "") {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        ui.close();
                    }
                });
                ui.menu_button("Édition", |ui| {
                    if item(ui, "Rechercher…", "Ctrl+F") {
                        let selected = self.selected_text();
                        self.find.show(selected, false);
                        ui.close();
                    }
                    if item(ui, "Renommer…", "Maj+F6") {
                        if let Some(doc) = self.documents.get_mut(self.active) {
                            doc.view.request_rename(&doc.text);
                        }
                        ui.close();
                    }
                    if item(ui, "Remplacer…", "Ctrl+R") {
                        let selected = self.selected_text();
                        self.find.show(selected, true);
                        ui.close();
                    }
                    if item(ui, "Rechercher partout…", "Double Maj") {
                        self.open_palette();
                        ui.close();
                    }
                });
                ui.menu_button("Affichage", |ui| {
                    ui.menu_button("Fenêtres d'outils", |ui| {
                        for tool in self.all_tools() {
                            if item(ui, tool.title, tool.shortcut) {
                                self.open_tool(tool);
                                ui.close();
                            }
                        }
                    });
                    ui.menu_button("Thème", |ui| {
                        for (index, name) in ui::THEMES.iter().enumerate() {
                            if ui.radio(self.settings.theme == index, *name).clicked() {
                                self.settings.theme = index;
                                self.apply_theme(ui.ctx());
                                ui.close();
                            }
                        }
                    });
                });
                ui.menu_button("Exécuter", |ui| {
                    if item(ui, "Exécuter le fichier actif", "F5") {
                        self.run_file();
                        ui.close();
                    }
                    self.external_commands_menu(ui);
                });
                let menus: Vec<_> = self
                    .builtins
                    .iter()
                    .filter(|p| self.builtin_enabled(p.id()))
                    .map(|p| (p.id(), p.name(), p.commands()))
                    .collect();
                for (plugin, name, commands) in menus {
                    ui.menu_button(name, |ui| {
                        for command in commands {
                            let label = command
                                .label
                                .split_once(" : ")
                                .map_or(command.label.as_str(), |(_, l)| l);
                            let (label, shortcut) = label.split_once("  ").unwrap_or((label, ""));
                            if item(ui, label, shortcut) {
                                self.run_builtin_command(plugin, command.id);
                                ui.close();
                            }
                        }
                    });
                }
                ui.menu_button("Fenêtre", |ui| {
                    if item(ui, "Nouvelle fenêtre de projet…", "") {
                        self.choose_project(true);
                        ui.close();
                    }
                    if item(ui, "Notifications", "") {
                        self.show_notifications = !self.show_notifications;
                        ui.close();
                    }
                });
                ui.menu_button("Aide", |ui| {
                    if item(ui, "Raccourcis clavier", "") {
                        self.show_settings = true;
                        self.settings_page = 2;
                        ui.close();
                    }
                    if item(ui, "À propos", "") {
                        self.push_note(
                            "Atelier IDE",
                            format!("Version {}", env!("CARGO_PKG_VERSION")),
                            false,
                            true,
                        );
                        ui.close();
                    }
                });
            });
    }

    fn external_commands_menu(&mut self, ui: &mut egui::Ui) {
        let mut command = None;
        let enabled: Vec<_> = self
            .plugins
            .iter()
            .filter(|p| {
                self.settings
                    .enabled_plugins
                    .contains(&self.plugin_key(&p.id))
            })
            .collect();
        if !enabled.is_empty() {
            ui.separator();
        }
        for plugin in enabled {
            for cmd in &plugin.commands {
                let shell = plugins::shell_command(&cmd.program, &cmd.args);
                if ui.button(&cmd.name).on_hover_text(&shell).clicked() {
                    command = Some(shell);
                    ui.close();
                }
            }
        }
        if let Some(command) = command {
            self.execute(&command);
        }
    }

    fn project_widget(&mut self, ui: &mut egui::Ui, name: &str, color: Color32) {
        let t = self.theme(ui.ctx());
        let galley = ui
            .painter()
            .layout_no_wrap(name.to_string(), ui::semibold(13.0), t.text);
        let size = vec2(8.0 + 20.0 + 8.0 + galley.size().x + 6.0 + 16.0 + 8.0, 28.0);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let fill = color.gamma_multiply(if response.hovered() { 0.42 } else { 0.3 });
        ui.painter().rect_filled(rect, 6.0, fill);
        let badge = Rect::from_min_size(
            pos2(rect.left() + 8.0, rect.center().y - 10.0),
            vec2(20.0, 20.0),
        );
        ui.painter().rect_filled(badge, 4.0, color);
        ui.painter().text(
            badge.center(),
            Align2::CENTER_CENTER,
            initials(name),
            ui::semibold(10.5),
            Color32::WHITE,
        );
        let x = badge.right() + 8.0;
        ui.painter().galley(
            pos2(x, rect.center().y - galley.size().y / 2.0),
            galley.clone(),
            t.text,
        );
        ui::paint_icon(
            ui,
            "chevronDown",
            Rect::from_min_size(
                pos2(x + galley.size().x + 4.0, rect.center().y - 8.0),
                vec2(16.0, 16.0),
            ),
        );
        let popup = egui::Popup::menu(&response).id(egui::Id::new("project_menu"));
        popup.show(|ui| {
            ui.set_min_width(300.0);
            ui::section(ui, "Projets récents");
            for path in self.settings.recent.clone() {
                let label = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let current = path == self.root;
                let button = egui::Button::new(RichText::new(&label).strong())
                    .right_text(
                        RichText::new(project::simplify(&path).display().to_string())
                            .weak()
                            .small(),
                    )
                    .selected(current);
                let response = ui.add(button);
                if response.clicked() && !current {
                    self.open_project(path.clone());
                    ui.close();
                }
                response.context_menu(|ui| {
                    if ui.button("Ouvrir dans cette fenêtre").clicked() {
                        self.request(Pending::Project(path.clone()));
                        ui.close();
                    }
                    if ui.button("Ouvrir dans une nouvelle fenêtre").clicked() {
                        self.open_window(path.clone());
                        ui.close();
                    }
                    if ui.button("Retirer de la liste").clicked() {
                        self.settings.recent.retain(|p| p != &path);
                        ui.close();
                    }
                });
            }
            ui.separator();
            if ui.button("Ouvrir un projet…").clicked() {
                self.choose_project(false);
                ui.close();
            }
            if ui.button("Ouvrir dans une nouvelle fenêtre…").clicked() {
                self.choose_project(true);
                ui.close();
            }
            let git: Vec<_> = self
                .builtins
                .iter()
                .filter(|p| self.builtin_enabled(p.id()))
                .flat_map(|p| p.commands().into_iter().map(move |c| (p.id(), c)))
                .filter(|(_, c)| c.id == "git.clone")
                .collect();
            for (plugin, command) in git {
                if ui.button("Cloner un dépôt Git…").clicked() {
                    self.run_builtin_command(plugin, command.id);
                    ui.close();
                }
            }
        });
    }

    /// Run configuration widget: IntelliJ's "Current File".
    fn run_widget(&mut self, ui: &mut egui::Ui) {
        let label = match self.documents.get(self.active) {
            Some(doc) => doc.title(),
            None => "Fichier actif".into(),
        };
        let response = ui::icon_text_button(ui, None, &label, true);
        egui::Popup::menu(&response)
            .id(egui::Id::new("run_menu"))
            .show(|ui| {
                ui.set_min_width(220.0);
                if ui
                    .add(egui::Button::new(format!("Exécuter « {label} »")).shortcut_text("F5"))
                    .clicked()
                {
                    self.run_file();
                    ui.close();
                }
                self.external_commands_menu(ui);
            });
    }

    /// Minimize / maximize / close, laid out right to left. Closing goes through the
    /// regular close request, so unsaved changes are still confirmed.
    fn window_controls(&self, ui: &mut egui::Ui, maximized: bool) {
        let ctx = ui.ctx().clone();
        let t = self.theme(&ctx);
        let size = vec2(46.0, TITLE_HEIGHT);
        ui.spacing_mut().item_spacing.x = 0.0;
        let control = |ui: &mut egui::Ui, icon: &str, close: bool, tooltip: &str| {
            let (rect, response) = ui.allocate_exact_size(size, Sense::click());
            let hovered = response.hovered();
            if hovered {
                ui.painter()
                    .rect_filled(rect, 0.0, if close { CLOSE_HOVER } else { t.hover });
            }
            let image = if close && hovered {
                egui::Image::new(ui::icon_on_accent(icon))
            } else {
                ui::icon(ui, icon)
            };
            image
                .fit_to_exact_size(vec2(16.0, 16.0))
                .paint_at(ui, Rect::from_center_size(rect.center(), vec2(16.0, 16.0)));
            response.on_hover_text(tooltip).clicked()
        };
        if control(ui, "winClose", true, "Fermer") {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let (icon, tooltip) = if maximized {
            ("winRestore", "Restaurer")
        } else {
            ("winMaximize", "Agrandir")
        };
        if control(ui, icon, false, tooltip) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }
        if control(ui, "winMinimize", false, "Réduire") {
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

    // ---- Tool windows -------------------------------------------------------------------

    fn all_tools(&self) -> Vec<&'static ToolWindow> {
        let mut tools = vec![&PROJECT_TOOL];
        tools.extend(self.tool_windows());
        tools.extend([&TERMINAL_TOOL, &NOTIFICATIONS_TOOL]);
        tools
    }

    fn tool_visible(&self, tool: &ToolWindow) -> bool {
        match tool.id {
            "project" => self.show_project && self.left_tool.is_none(),
            "terminal" => self.show_terminal && self.bottom_tool.is_none(),
            "notifications" => self.show_notifications,
            _ => self.tool_open(tool),
        }
    }

    fn stripe(&mut self, ctx: &egui::Context, side: Dock) {
        let t = self.theme(ctx);
        let panel = match side {
            Dock::Right => egui::SidePanel::right("stripe_right"),
            _ => egui::SidePanel::left("stripe_left"),
        };
        panel
            .exact_width(STRIPE_WIDTH)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(t.frame)
                    .inner_margin(Margin::symmetric(5, 6)),
            )
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                let tools = self.all_tools();
                let top: Vec<_> = tools
                    .iter()
                    .copied()
                    .filter(|t| {
                        t.dock
                            == if side == Dock::Right {
                                Dock::Right
                            } else {
                                Dock::Left
                            }
                    })
                    .collect();
                for tool in top {
                    if self.stripe_button(ui, tool) {
                        self.toggle_tool(tool);
                    }
                }
                if side == Dock::Right {
                    return;
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.add_space(2.0);
                    let bottom: Vec<_> = tools
                        .iter()
                        .copied()
                        .filter(|t| t.dock == Dock::Bottom)
                        .collect();
                    for tool in bottom.into_iter().rev() {
                        if self.stripe_button(ui, tool) {
                            self.toggle_tool(tool);
                        }
                    }
                });
            });
    }

    fn stripe_button(&self, ui: &mut egui::Ui, tool: &'static ToolWindow) -> bool {
        let t = self.theme(ui.ctx());
        let open = self.tool_visible(tool);
        let focused = open && self.focus == Some(tool.id);
        let (rect, response) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
        let fill = if focused {
            t.accent
        } else if open {
            t.tool_open
        } else if response.hovered() {
            t.hover
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 6.0, fill);
        let image = if focused {
            egui::Image::new(ui::icon_on_accent(tool.icon))
        } else {
            egui::Image::new(ui::icon_source(tool.icon, t.dark))
        };
        image.paint_at(ui, Rect::from_center_size(rect.center(), vec2(20.0, 20.0)));
        let tooltip = if tool.shortcut.is_empty() {
            tool.title.to_string()
        } else {
            format!("{}  {}", tool.title, tool.shortcut)
        };
        response.on_hover_text(tooltip).clicked()
    }

    /// Title row of a tool window. Returns true when its hide button was clicked.
    fn tool_header(
        &mut self,
        ui: &mut egui::Ui,
        title: &str,
        extra: impl FnOnce(&mut Self, &mut egui::Ui),
    ) -> bool {
        let mut hide = false;
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), HEADER_HEIGHT), Sense::hover());
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(vec2(8.0, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.spacing_mut().item_spacing.x = 2.0;
        child.add_space(4.0);
        child.label(RichText::new(title).font(ui::semibold(13.0)));
        child.add_space(8.0);
        child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            hide = ui::icon_button(ui, "hide", "Masquer").clicked();
            extra(self, ui);
        });
        hide
    }

    fn panel_focus(&mut self, id: Option<&'static str>, rect: Rect) {
        self.panel_rects.push((id, rect));
    }

    /// The focused tool window follows the last click, as the blue stripe button in IntelliJ.
    fn track_focus(&mut self, ctx: &egui::Context) {
        let pressed = ctx.input(|i| {
            i.pointer
                .any_pressed()
                .then(|| i.pointer.interact_pos())
                .flatten()
        });
        if let Some(pos) = pressed
            && let Some((id, _)) = self.panel_rects.iter().find(|(_, r)| r.contains(pos))
        {
            self.focus = *id;
        }
        self.panel_rects.clear();
    }

    // ---- Context graph --------------------------------------------------------------------

    /// The menu of the graph button of the project window.
    fn graph_menu(&mut self, ui: &mut egui::Ui) {
        ui::menu_begin(ui);
        ui::menu_title(ui, "Graphe de contexte");
        let status = self.graph.status();
        if !self.graph.enabled() {
            if ui::menu_item(ui, Some("refresh"), "Générer le graphe de contexte", None, false).clicked() {
                self.graph.enable(self.repaint.clone());
                ui.close();
            }
            ui.label(RichText::new("Les IA lisent ce plan du projet au lieu de ses fichiers.").small().color(self.theme(ui.ctx()).text_secondary));
            return;
        }
        let state = if status.busy {
            "Mise à jour…".to_string()
        } else {
            format!("À jour · {} fichiers · {} éléments · {} groupes", status.files, status.symbols, status.groups)
        };
        ui.label(RichText::new(state).small().color(self.theme(ui.ctx()).text_secondary));
        if let Some(error) = &status.error {
            ui.colored_label(self.theme(ui.ctx()).error, error);
        }
        if ui::menu_item(ui, Some("editorPreview"), "Ouvrir le rapport", None, false).clicked() {
            let report = crate::graph::folder(&self.root).join("GRAPH_REPORT.md");
            if report.exists() {
                self.open_file(report);
            }
            ui.close();
        }
        if ui::menu_item(ui, Some("refresh"), "Régénérer", None, false).clicked() {
            self.graph.regenerate();
            ui.close();
        }
        if ui::menu_item(ui, Some("closeSmall"), "Désactiver", None, false).clicked() {
            self.graph.disable();
            ui.close();
        }
    }

    /// The offer to make the graph, at the top of the project window of a repository nobody has
    /// answered for.
    fn graph_offer(&mut self, ui: &mut egui::Ui) {
        if !self.graph.should_offer() {
            return;
        }
        let t = self.theme(ui.ctx());
        egui::Frame::new().fill(t.hover).corner_radius(8).inner_margin(Margin::same(10)).outer_margin(Margin::symmetric(8, 4)).show(ui, |ui| {
            ui.label(RichText::new("Générer le graphe de contexte ?").font(ui::semibold(12.5)));
            ui.label(RichText::new("Un plan du projet, fait sans IA et tenu à jour, que les IA lisent au lieu d'ouvrir tous les fichiers.").small().color(t.text_secondary));
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                if ui::primary_button(ui, "Générer").clicked() {
                    self.graph.enable(self.repaint.clone());
                }
                if ui::button(ui, "Plus tard").clicked() {
                    self.graph.postpone();
                }
                if ui::button(ui, "Ne plus proposer").clicked() {
                    self.graph.decline();
                }
            });
        });
    }

    // ---- Project tool window ------------------------------------------------------------

    fn left_panel(&mut self, ctx: &egui::Context) {
        if let Some(tool) = self.left_tool {
            let response = egui::SidePanel::left("tool_left")
                .default_width(340.0)
                .width_range(220.0..=760.0)
                .frame(self.island(ctx))
                .show_separator_line(false)
                .show(ctx, |ui| self.tool_panel(ui, tool));
            self.panel_focus(Some(tool.id), response.response.rect);
            return;
        }
        if !self.show_project {
            return;
        }
        let response = egui::SidePanel::left("project")
            .default_width(300.0)
            .width_range(180.0..=600.0)
            .frame(self.island(ctx))
            .show_separator_line(false)
            .show(ctx, |ui| {
                let hide = self.tool_header(ui, "Projet", |ide, ui| {
                    if ui::icon_button(ui, "collapseAll", "Tout replier").clicked() {
                        ide.expanded = BTreeSet::from([ide.root.clone()]);
                    }
                    if ui::icon_button(ui, "refresh", "Actualiser fichiers et plugins").clicked() {
                        ide.tree.clear();
                        (ide.plugins, ide.plugin_errors) = plugins::load(&ide.root);
                    }
                    if ui::icon_button(ui, "add", "Nouveau fichier").clicked() {
                        ide.show_new_file = true;
                    }
                    let graph = ui::icon_button(ui, "graph", if ide.graph.enabled() { "Graphe de contexte (activé)" } else { "Graphe de contexte" });
                    egui::Popup::menu(&graph).id(egui::Id::new("graph_menu")).show(|ui| ide.graph_menu(ui));
                });
                if hide {
                    self.show_project = false;
                }
                self.graph_offer(ui);
                let root = self.root.clone();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let response = self.tree_row(ui, &root, true, 0);
                        if response.clicked() && self.tree_click(ui.ctx(), &root, false) {
                            self.toggle_folder(&root);
                        }
                        self.tree_menu(&response, &root, true);
                        self.tree_drag_row(ui, &response, &root, true);
                        if self.expanded.contains(&root) {
                            self.tree_ui(ui, &root, 1);
                        }
                        ui.add_space(8.0);
                        // Dragging near the top or bottom edge scrolls the tree.
                        if self.tree_drag.is_some()
                            && let Some(p) = ui.ctx().pointer_interact_pos()
                        {
                            let clip = ui.clip_rect();
                            let edge = 32.0;
                            let dy = if p.y < clip.top() + edge {
                                clip.top() + edge - p.y
                            } else if p.y > clip.bottom() - edge {
                                (clip.bottom() - edge) - p.y
                            } else {
                                0.0
                            };
                            if dy != 0.0 && clip.contains(p) {
                                ui.scroll_with_delta(vec2(0.0, dy * 0.6));
                                ui.ctx().request_repaint();
                            }
                        }
                    });
            });
        self.panel_focus(Some("project"), response.response.rect);
        self.finish_tree_drag(ctx);
        self.tree_keys(ctx);
        self.scroll_to_selected = false;
    }

    fn tool_panel(&mut self, ui: &mut egui::Ui, tool: &'static ToolWindow) {
        if self.tool_header(ui, tool.title, |_, _| {}) {
            self.toggle_tool(tool);
        }
        egui::Frame::new()
            .inner_margin(Margin {
                left: 8,
                right: 8,
                top: 0,
                bottom: 6,
            })
            .show(ui, |ui| {
                self.each_builtin(|plugin, ide| {
                    if plugin.tool_windows().iter().any(|t| t.id == tool.id) {
                        plugin.tool_window(tool.id, ui, ide);
                    }
                });
            });
    }

    fn toggle_folder(&mut self, path: &Path) {
        if !self.expanded.remove(path) {
            self.expanded.insert(path.to_path_buf());
        }
    }

    fn tree_menu(&mut self, response: &egui::Response, path: &Path, is_dir: bool) {
        response.context_menu(|ui| {
            ui::menu_begin(ui);
            let is_root = path == self.root;
            let selection: Vec<PathBuf> = self
                .tree_selection()
                .into_iter()
                .filter(|p| *p != self.root)
                .collect();
            if selection.len() > 1 && selection.iter().any(|p| p == path) {
                ui::menu_title(ui, &format!("{} éléments sélectionnés", selection.len()));
                if ui::menu_item(ui, Some("delete"), "Supprimer…", Some("Suppr"), true).clicked() {
                    self.tree_dialog = Some(TreeDialog::Delete(selection));
                    ui.close();
                } else if ui::menu_item(ui, Some("copy"), "Copier les chemins", None, false).clicked() {
                    let paths: Vec<_> = selection
                        .iter()
                        .map(|p| project::simplify(p).display().to_string())
                        .collect();
                    ui.ctx().copy_text(paths.join("\n"));
                    ui.close();
                }
                return;
            }
            if !is_dir && ui::menu_item(ui, Some("openNewTab"), "Ouvrir", Some("Entrée"), false).clicked() {
                self.open_file(path.to_path_buf());
                ui.close();
            }
            let base = if is_dir {
                path
            } else {
                path.parent().unwrap_or(&self.root)
            };
            let prefix = base
                .strip_prefix(&self.root)
                .map(|p| {
                    let p = p.to_string_lossy().replace('\\', "/");
                    if p.is_empty() { p } else { format!("{p}/") }
                })
                .unwrap_or_default();
            if ui::menu_item(ui, Some("add"), "Nouveau fichier…", Some("Ctrl+N"), false).clicked() {
                self.new_name = prefix.clone();
                self.show_new_file = true;
                ui.close();
            }
            if ui::menu_item(ui, Some("newFolder"), "Nouveau dossier…", None, false).clicked() {
                self.tree_dialog = Some(TreeDialog::NewFolder(prefix));
                ui.close();
            }
            if !is_root {
                ui::menu_separator(ui);
                if ui::menu_item(ui, Some("edit"), "Renommer…", Some("F2"), false).clicked() {
                    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                    self.tree_dialog = Some(TreeDialog::Rename { path: path.to_path_buf(), name });
                    ui.close();
                }
                if ui::menu_item(ui, Some("delete"), "Supprimer…", Some("Suppr"), true).clicked() {
                    self.tree_dialog = Some(TreeDialog::Delete(vec![path.to_path_buf()]));
                    ui.close();
                }
            }
            ui::menu_separator(ui);
            if ui::menu_item(ui, Some("copy"), "Copier le chemin", None, false).clicked() {
                ui.ctx().copy_text(project::simplify(path).display().to_string());
                ui.close();
            }
            if ui::menu_item(ui, None, "Copier le chemin relatif", None, false).clicked() {
                let relative = path.strip_prefix(&self.root).unwrap_or(path);
                ui.ctx().copy_text(relative.display().to_string());
                ui.close();
            }
            ui::menu_separator(ui);
            if ui::menu_item(ui, Some("locate"), "Afficher dans l'explorateur", None, false).clicked() {
                let target = project::simplify(path);
                #[cfg(windows)]
                let _ = std::process::Command::new("explorer")
                    .arg(format!("/select,{}", target.display()))
                    .spawn();
                #[cfg(not(windows))]
                let _ = std::process::Command::new("xdg-open")
                    .arg(target.parent().unwrap_or(&target))
                    .spawn();
                ui.close();
            }
            if is_dir && ui::menu_item(ui, Some("terminal"), "Ouvrir dans le terminal", None, false).clicked() {
                let dir = project::simplify(path).display().to_string();
                let command = if cfg!(windows) {
                    format!("Set-Location -LiteralPath '{}'", dir.replace('\'', "''"))
                } else {
                    format!("cd '{}'", dir.replace('\'', "'\\''"))
                };
                self.execute(&command);
                ui.close();
            }
            if !is_dir {
                let commands: Vec<_> = self
                    .builtins
                    .iter()
                    .filter(|p| self.builtin_enabled(p.id()))
                    .flat_map(|p| {
                        p.gutter_commands(path)
                            .into_iter()
                            .map(move |c| (p.id(), c))
                    })
                    .collect();
                if !commands.is_empty() {
                    ui::menu_separator(ui);
                    ui::menu_title(ui, "Git");
                    for (plugin, command) in commands {
                        if ui::menu_item(ui, Some("git"), &command.label, None, false).clicked() {
                            // File commands act on the active document.
                            self.open_file(path.to_path_buf());
                            self.run_builtin_command(plugin, command.id);
                            ui.close();
                        }
                    }
                }
            }
        });
    }

    /// Drag and drop of a tree row: the folder under the pointer receives the dragged entry.
    fn tree_drag_row(&mut self, ui: &egui::Ui, response: &egui::Response, entry: &Path, is_dir: bool) {
        if response.dragged()
            && ui.input(|i| i.pointer.is_decidedly_dragging())
            && entry != self.root
            && self.tree_drag.is_none()
        {
            // Grabbing a selected row carries the whole selection, else just that row.
            if !self.tree_is_selected(entry) {
                self.tree_select_only(entry);
            }
            self.tree_drag = Some(entry.to_path_buf());
            self.tree_drag_set = self
                .tree_selection()
                .into_iter()
                .filter(|p| *p != self.root)
                .collect();
        }
        if self.tree_drag.is_none() {
            return;
        }
        let over = ui
            .ctx()
            .pointer_interact_pos()
            .is_some_and(|p| response.rect.contains(p));
        if over {
            let dir = if is_dir {
                Some(entry)
            } else {
                entry.parent()
            };
            if let Some(dir) = dir.filter(|d| !self.tree_drag_set.iter().any(|from| d.starts_with(from))) {
                self.tree_drop_next = Some(dir.to_path_buf());
            }
        }
    }

    /// Ends a drag in the tree: moves the entry into the folder it was dropped on.
    fn finish_tree_drag(&mut self, ctx: &egui::Context) {
        if let Some(from) = &self.tree_drag
            && let Some(pointer) = ctx.pointer_interact_pos()
        {
            let name = if self.tree_drag_set.len() > 1 {
                format!("{} éléments", self.tree_drag_set.len())
            } else {
                from.file_name().unwrap_or_default().to_string_lossy().into_owned()
            };
            let t = self.theme(ctx);
            ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("tree_drag")))
                .text(pointer + vec2(12.0, 12.0), Align2::LEFT_TOP, name, FontId::proportional(13.0), t.text);
        }
        let target = self.tree_drop_next.take();
        if self.tree_drag.is_some() {
            match (&target, &self.tree_hover) {
                (Some(dir), Some((hovered, since))) if dir == hovered => {
                    if since.elapsed() > Duration::from_millis(700) {
                        self.expanded.insert(dir.clone());
                    }
                }
                (Some(dir), _) => self.tree_hover = Some((dir.clone(), Instant::now())),
                (None, _) => self.tree_hover = None,
            }
            ctx.request_repaint_after(Duration::from_millis(100));
        } else {
            self.tree_hover = None;
        }
        if ctx.input(|i| i.pointer.any_released()) {
            if let (Some(grabbed), Some(to)) = (self.tree_drag.take(), target.clone()) {
                let mut set = std::mem::take(&mut self.tree_drag_set);
                if set.is_empty() {
                    set.push(grabbed);
                }
                let mut moved = Vec::new();
                for from in set {
                    match project::move_into(&from, &to) {
                        Ok(new) => {
                            self.path_changed(&from, &new);
                            moved.push(new);
                        }
                        Err(e) => {
                            self.status = format!("Erreur : {e:#}");
                            break;
                        }
                    }
                }
                if let [only] = &moved[..] {
                    self.status = format!("Déplacé : {}", only.display());
                } else if moved.len() > 1 {
                    self.status = format!("{} éléments déplacés", moved.len());
                    self.tree_multi = moved.into_iter().collect();
                }
                if self.tree_multi.len() > 1 || !self.status.starts_with("Erreur") {
                    self.expanded.insert(to);
                }
            }
            self.tree_drag = None;
            self.tree_drag_set.clear();
            self.tree_drop = None;
        } else {
            self.tree_drop = target;
            if !ctx.input(|i| i.pointer.any_down()) {
                self.tree_drag = None;
            }
        }
    }

    /// A file or folder was renamed or moved: documents, tree and selection follow it.
    fn path_changed(&mut self, from: &Path, to: &Path) {
        let remap = |p: &Path| p.strip_prefix(from).ok().map(|rest| to.join(rest));
        for doc in &mut self.documents {
            if let Some(new) = doc.path.as_deref().and_then(remap) {
                doc.relocate(new);
            }
        }
        self.expanded = self
            .expanded
            .iter()
            .map(|p| remap(p).unwrap_or_else(|| p.clone()))
            .collect();
        self.tree_selected = self.tree_selected.take().map(|p| remap(&p).unwrap_or(p));
        self.tree_multi = self.tree_multi.iter().map(|p| remap(p).unwrap_or_else(|| p.clone())).collect();
        self.tree_anchor = self.tree_anchor.take().map(|p| remap(&p).unwrap_or(p));
        self.tree.clear();
        self.project_files = None;
    }

    /// A file or folder was deleted: unmodified documents close, modified ones become untitled.
    fn path_deleted(&mut self, path: &Path) {
        let active = self.documents.get(self.active).map(|d| d.path.clone());
        for doc in &mut self.documents {
            if doc.path.as_deref().is_some_and(|p| p.starts_with(path)) && doc.dirty() {
                doc.path = None;
            }
        }
        self.documents
            .retain(|d| !d.path.as_deref().is_some_and(|p| p.starts_with(path)));
        self.active = match active {
            Some(Some(p)) => self.documents.iter().position(|d| d.path.as_ref() == Some(&p)),
            _ => None,
        }
        .unwrap_or_else(|| self.active.min(self.documents.len().saturating_sub(1)));
        self.expanded.retain(|p| !p.starts_with(path));
        if self.tree_selected.as_deref().is_some_and(|p| p.starts_with(path)) {
            self.tree_selected = None;
        }
        self.tree_multi.retain(|p| !p.starts_with(path));
        if self.tree_multi.len() < 2 {
            self.tree_multi.clear();
        }
        self.tree.clear();
        self.project_files = None;
    }

    /// Selects `path` in the project view, opening its folders.
    fn reveal(&mut self, path: &Path) {
        self.show_project = true;
        self.left_tool = None;
        self.tree_select_only(path);
        self.scroll_to_selected = true;
        let mut dir = path.parent();
        while let Some(d) = dir.filter(|d| d.starts_with(&self.root)) {
            self.expanded.insert(d.to_path_buf());
            dir = d.parent();
        }
    }

    fn tree_is_selected(&self, path: &Path) -> bool {
        self.tree_selected.as_deref() == Some(path) || self.tree_multi.contains(path)
    }

    /// The selected entries, in tree order of their paths.
    fn tree_selection(&self) -> Vec<PathBuf> {
        if self.tree_multi.is_empty() {
            self.tree_selected.iter().cloned().collect()
        } else {
            self.tree_multi.iter().cloned().collect()
        }
    }

    fn tree_select_only(&mut self, path: &Path) {
        self.tree_multi.clear();
        self.tree_selected = Some(path.to_path_buf());
        self.tree_anchor = Some(path.to_path_buf());
    }

    /// Selects every row between the anchor and `to`.
    fn tree_select_range(&mut self, to: &Path) {
        let rows = self.visible_rows();
        let anchor = self.tree_anchor.clone().or_else(|| self.tree_selected.clone());
        let from = anchor.as_ref().and_then(|a| rows.iter().position(|(p, _)| p == a));
        let end = rows.iter().position(|(p, _)| p == to);
        let (Some(from), Some(end)) = (from, end) else {
            self.tree_select_only(to);
            return;
        };
        let (low, high) = (from.min(end), from.max(end));
        self.tree_multi = rows[low..=high]
            .iter()
            .map(|(p, _)| p.clone())
            .filter(|p| *p != self.root)
            .collect();
        if self.tree_anchor.is_none() {
            self.tree_anchor = anchor;
        }
        self.tree_selected = Some(to.to_path_buf());
        if self.tree_multi.len() < 2 {
            self.tree_multi.clear();
        }
    }

    /// A click (or right click) on a row. Ctrl toggles the row in the selection, Shift selects
    /// the rows from the anchor. True for a plain click, which also opens the entry.
    fn tree_click(&mut self, ctx: &egui::Context, path: &Path, secondary: bool) -> bool {
        let modifiers = ctx.input(|i| i.modifiers);
        if secondary {
            if !self.tree_is_selected(path) {
                self.tree_select_only(path);
            }
            return false;
        }
        if modifiers.command || modifiers.ctrl {
            if self.tree_multi.is_empty() {
                self.tree_multi.extend(self.tree_selected.clone());
            }
            if !self.tree_multi.remove(path) {
                self.tree_multi.insert(path.to_path_buf());
                self.tree_selected = Some(path.to_path_buf());
            } else if self.tree_selected.as_deref() == Some(path) {
                self.tree_selected = self.tree_multi.iter().next().cloned();
            }
            self.tree_anchor = Some(path.to_path_buf());
            if self.tree_multi.len() < 2 {
                self.tree_selected = self.tree_multi.iter().next().cloned().or(self.tree_selected.take());
                self.tree_multi.clear();
            }
            return false;
        }
        if modifiers.shift {
            self.tree_select_range(path);
            return false;
        }
        self.tree_select_only(path);
        true
    }

    /// Rows currently shown in the tree, root first.
    fn visible_rows(&self) -> Vec<(PathBuf, bool)> {
        fn walk(ide: &Ide, dir: &Path, rows: &mut Vec<(PathBuf, bool)>) {
            for (entry, is_dir) in ide.tree.get(dir).into_iter().flatten() {
                rows.push((entry.clone(), *is_dir));
                if *is_dir && ide.expanded.contains(entry) {
                    walk(ide, entry, rows);
                }
            }
        }
        let mut rows = vec![(self.root.clone(), true)];
        if self.expanded.contains(&self.root) {
            walk(self, &self.root, &mut rows);
        }
        rows
    }

    /// Arrow keys, Enter, F2 and Delete in the project view, like IntelliJ.
    fn tree_keys(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers};
        if self.focus != Some("project")
            || !self.show_project
            || self.tree_dialog.is_some()
            || self.show_new_file
            || self.show_palette
            || self.pending.is_some()
            || ctx.memory(|m| m.focused()).is_some()
        {
            return;
        }
        let key = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k));
        let rows = self.visible_rows();
        let current = self
            .tree_selected
            .as_ref()
            .and_then(|s| rows.iter().position(|(p, _)| p == s));
        let mut select = None;
        let extend = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::SHIFT, k));
        if let Some(cur) = current {
            // Shift + arrows grow the selection from the anchor.
            let target = if extend(Key::ArrowDown) {
                rows.get(cur + 1)
            } else if extend(Key::ArrowUp) {
                cur.checked_sub(1).and_then(|i| rows.get(i))
            } else {
                None
            };
            if let Some((path, _)) = target.cloned() {
                self.tree_select_range(&path);
                self.scroll_to_selected = true;
                return;
            }
        }
        if key(Key::ArrowDown) {
            select = rows.get(current.map_or(0, |i| i + 1)).map(|r| r.0.clone());
        } else if key(Key::ArrowUp) {
            select = current.and_then(|i| i.checked_sub(1)).and_then(|i| rows.get(i)).map(|r| r.0.clone());
        }
        if let Some((path, is_dir)) = current.map(|i| rows[i].clone()) {
            let expanded = self.expanded.contains(&path);
            if key(Key::ArrowRight) && is_dir {
                if expanded {
                    select = rows.get(current.unwrap() + 1).filter(|r| r.0.parent() == Some(&path)).map(|r| r.0.clone());
                } else {
                    self.expanded.insert(path.clone());
                }
            } else if key(Key::ArrowLeft) {
                if is_dir && expanded && path != self.root {
                    self.expanded.remove(&path);
                } else if let Some(parent) = path.parent().filter(|p| p.starts_with(&self.root)) {
                    select = Some(parent.to_path_buf());
                }
            } else if key(Key::Enter) {
                if is_dir {
                    self.toggle_folder(&path);
                } else {
                    self.open_file(path.clone());
                }
            } else if path != self.root && key(Key::F2) {
                let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                self.tree_dialog = Some(TreeDialog::Rename { path: path.clone(), name });
            } else if path != self.root && key(Key::Delete) {
                let selection: Vec<_> = self
                    .tree_selection()
                    .into_iter()
                    .filter(|p| *p != self.root)
                    .collect();
                self.tree_dialog = Some(TreeDialog::Delete(selection));
            }
        }
        if let Some(path) = select {
            self.tree_select_only(&path);
            self.scroll_to_selected = true;
        }
    }

    fn tree_dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.tree_dialog.take() else { return };
        let t = self.theme(ctx);
        let mut keep = None;
        match dialog {
            TreeDialog::NewFolder(mut name) => {
                let modal = egui::Modal::new(egui::Id::new("new_folder")).show(ctx, |ui| {
                    ui.set_width(420.0);
                    ui::dialog_title(ui, "Nouveau dossier");
                    ui.add(
                        egui::TextEdit::singleline(&mut name)
                            .hint_text("src/module")
                            .desired_width(f32::INFINITY),
                    )
                    .request_focus();
                    ui.label(RichText::new("Chemin relatif au projet").small().color(t.text_secondary));
                    ui::dialog_buttons(ui, "Créer")
                });
                let (ok, cancel) = modal.inner;
                if ok {
                    match project::create_dir(&self.root, &name) {
                        Ok(path) => {
                            self.reveal(&path);
                            self.expanded.insert(path);
                            self.tree.clear();
                        }
                        Err(e) => {
                            self.status = format!("Erreur : {e:#}");
                            keep = Some(TreeDialog::NewFolder(name));
                        }
                    }
                } else if !(cancel || modal.should_close()) {
                    keep = Some(TreeDialog::NewFolder(name));
                }
            }
            TreeDialog::Rename { path, mut name } => {
                let modal = egui::Modal::new(egui::Id::new("rename")).show(ctx, |ui| {
                    ui.set_width(420.0);
                    ui::dialog_title(ui, "Renommer");
                    ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY))
                        .request_focus();
                    ui::dialog_buttons(ui, "Renommer")
                });
                let (ok, cancel) = modal.inner;
                if ok {
                    match project::rename_path(&path, &name) {
                        Ok(new) => {
                            self.path_changed(&path, &new);
                            self.reveal(&new);
                        }
                        Err(e) => {
                            self.status = format!("Erreur : {e:#}");
                            keep = Some(TreeDialog::Rename { path, name });
                        }
                    }
                } else if !(cancel || modal.should_close()) {
                    keep = Some(TreeDialog::Rename { path, name });
                }
            }
            TreeDialog::Delete(paths) => {
                let modal = egui::Modal::new(egui::Id::new("delete")).show(ctx, |ui| {
                    ui.set_width(420.0);
                    ui::dialog_title(ui, "Supprimer");
                    if let [path] = &paths[..] {
                        let what = if path.is_dir() { "ce dossier et son contenu" } else { "ce fichier" };
                        ui.label(format!(
                            "Supprimer « {} » ? Cette action supprime {what} définitivement.",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        ));
                    } else {
                        ui.label(format!(
                            "Supprimer ces {} éléments et le contenu des dossiers ? Cette action est définitive.",
                            paths.len()
                        ));
                    }
                    ui::dialog_buttons(ui, "Supprimer")
                });
                let (ok, cancel) = modal.inner;
                if ok {
                    for path in &paths {
                        // Inside a folder that is deleted too: already gone.
                        if paths.iter().any(|other| other != path && path.starts_with(other)) {
                            continue;
                        }
                        match project::delete_path(path) {
                            Ok(()) => self.path_deleted(path),
                            Err(e) => {
                                self.status = format!("Erreur : {e:#}");
                                break;
                            }
                        }
                    }
                } else if !(cancel || modal.should_close()) {
                    keep = Some(TreeDialog::Delete(paths));
                }
            }
        }
        self.tree_dialog = keep;
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
                    self.tree_stamps.insert(path.into(), project::modified(path));
                    self.tree.insert(path.into(), entries);
                }
                Err(e) => {
                    ui.colored_label(
                        self.theme(ui.ctx()).error,
                        format!("Lecture impossible : {e}"),
                    );
                    return;
                }
            }
        }
        for (entry, is_dir) in self.tree.get(path).cloned().unwrap_or_default() {
            let response = self.tree_row(ui, &entry, is_dir, depth);
            let mut plain = false;
            if response.clicked() || response.secondary_clicked() {
                plain = self.tree_click(ui.ctx(), &entry, response.secondary_clicked());
                // The keyboard goes to the tree, not to the editor.
                response.surrender_focus();
            }
            self.tree_menu(&response, &entry, is_dir);
            self.tree_drag_row(ui, &response, &entry, is_dir);
            if is_dir {
                if response.clicked() && plain {
                    self.toggle_folder(&entry);
                }
                if self.expanded.contains(&entry) {
                    self.tree_ui(ui, &entry, depth + 1);
                }
            } else if response.clicked() && plain {
                self.open_file(entry);
            }
        }
    }

    /// One explorer line: indent, chevron (folders), icon, name colored by its VCS status.
    fn tree_row(
        &mut self,
        ui: &mut egui::Ui,
        path: &Path,
        is_dir: bool,
        depth: usize,
    ) -> egui::Response {
        const INDENT: f32 = 19.0;
        let t = self.theme(ui.ctx());
        let width = ui.available_width().max(200.0);
        let (rect, response) = ui.allocate_exact_size(vec2(width, ui::ROW), Sense::click_and_drag());
        #[cfg(test)]
        self.row_rects.push((path.to_path_buf(), rect));
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let selected = self.tree_is_selected(path);
        if self.tree_selected.as_deref() == Some(path) && self.scroll_to_selected {
            response.scroll_to_me(None);
        }
        ui::paint_row(ui, rect, selected, response.hovered() && self.tree_drag.is_none());
        if self.tree_drag.is_some() && self.tree_drop.as_deref() == Some(path) {
            ui.painter().rect(
                rect.shrink2(vec2(4.0, 0.0)),
                4.0,
                t.selection.gamma_multiply(0.5),
                Stroke::new(1.0_f32, t.accent),
                StrokeKind::Inside,
            );
        }
        let center_y = rect.center().y;
        let mut x = rect.left() + 8.0 + depth as f32 * INDENT;
        if is_dir {
            ui::paint_chevron(ui, pos2(x + 8.0, center_y), self.expanded.contains(path));
        }
        x += 18.0;
        let icon_rect = Rect::from_min_size(pos2(x, center_y - 8.0), vec2(16.0, 16.0));
        if is_dir {
            ui::paint_icon(ui, "folder", icon_rect);
        } else {
            egui::Image::new(self.file_icon(ui.ctx(), path)).paint_at(ui, icon_rect);
        }
        x += 22.0;
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let dirty = self
            .documents
            .iter()
            .any(|d| d.path.as_deref() == Some(path) && d.dirty());
        let vcs = self
            .builtins
            .iter()
            .filter(|p| self.builtin_enabled(p.id()))
            .find_map(|p| p.file_color(path));
        // Dependencies and build output are dimmed.
        let library = path.strip_prefix(&self.root).is_ok_and(|inside| {
            inside
                .components()
                .any(|c| project::is_library(&c.as_os_str().to_string_lossy()))
        });
        let color = if dirty {
            t.modified
        } else {
            vcs.unwrap_or(if library { t.text_secondary } else { t.text })
        };
        let font = FontId::proportional(13.0);
        let end = ui::paint_text(ui, rect, x, &name, color, font);
        if path == self.root {
            ui::paint_text(
                ui,
                rect,
                end + 8.0,
                &project::simplify(&self.root).display().to_string(),
                t.text_secondary,
                FontId::proportional(12.5),
            );
        }
        response
    }

    // ---- Bottom and right tool windows --------------------------------------------------

    fn bottom_panel(&mut self, ctx: &egui::Context) {
        let tool = match self.bottom_tool {
            Some(tool) => Some(tool),
            None if self.show_terminal => None,
            None => return,
        };
        let response = egui::TopBottomPanel::bottom("tool_bottom")
            .default_height(300.0)
            .height_range(140.0..=820.0)
            .resizable(true)
            .frame(self.island(ctx))
            .show_separator_line(false)
            .show(ctx, |ui| match tool {
                Some(tool) => self.tool_panel(ui, tool),
                None => self.terminal_panel(ui),
            });
        let id = tool.map_or("terminal", |t| t.id);
        self.panel_focus(Some(id), response.response.rect);
    }

    fn terminal_panel(&mut self, ui: &mut egui::Ui) {
        if self.terminals.is_empty() && self.terminal_error.is_empty() {
            self.start_terminal(None);
        }
        let ctx = ui.ctx().clone();
        let t = self.theme(&ctx);
        let mut restart = false;
        let mut new_shell: Option<Shell> = None;
        let mut split: Option<bool> = None;
        let mut drop: Option<(u64, bool, bool)> = None;
        let hide = self.tool_header(ui, "Terminal", |ide, ui| {
            if ui::icon_button(ui, "rerun", "Redémarrer le shell").clicked() {
                restart = true;
            }
            if ui::icon_button(ui, "stop", "Interrompre (Ctrl+C)").clicked()
                && let Some(pane) = ide
                    .terminals
                    .get_mut(ide.active_terminal)
                    .and_then(|tab| tab.pane())
            {
                let result = pane.term.write(&[3]);
                ide.report(result);
            }
            if ui::icon_button(ui, "unified", "Fractionner vers le bas").clicked() {
                split = Some(true);
            }
            if ui::icon_button(ui, "sideBySide", "Fractionner à droite").clicked() {
                split = Some(false);
            }
            let plus = ui::icon_button(ui, "add", "Nouveau terminal");
            egui::Popup::menu(&plus).id(egui::Id::new("terminal_shells")).show(|ui| {
                ui::menu_begin(ui);
                ui::menu_title(ui, "Nouveau terminal");
                let shells = ide.shells.get_or_insert_with(Shell::available);
                for shell in shells.iter() {
                    if ui::menu_item(ui, Some("terminal"), &shell.name, None, false).clicked() {
                        new_shell = Some(shell.clone());
                        ui.close();
                    }
                }
            });
        });
        // Session tabs, next to the title.
        let top = ui.min_rect().top() + 6.0;
        let mut x = ui.min_rect().left() + 92.0;
        let limit = ui.max_rect().right() - 130.0;
        let mut select = None;
        let mut close = None;
        let mut close_pane = false;
        let mut tab_rects: Vec<Rect> = Vec::new();
        for (index, tab) in self.terminals.iter().enumerate() {
            let galley = ui.painter().layout_no_wrap(tab.name().to_string(), FontId::proportional(12.5), t.text);
            let width = galley.size().x + 44.0;
            if x + width > limit {
                break;
            }
            let rect = Rect::from_min_size(pos2(x, top), vec2(width, 26.0));
            tab_rects.push(rect);
            let active = index == self.active_terminal;
            let response = ui.interact(rect, egui::Id::new(("terminal_tab", index)), Sense::click_and_drag());
            if response.dragged() && ui.input(|i| i.pointer.is_decidedly_dragging()) {
                self.drag = Some(Drag::Tab(index));
            }
            if active || response.hovered() {
                ui.painter().rect(
                    rect,
                    6.0,
                    if active { t.frame } else { t.hover },
                    Stroke::new(1.0_f32, if active { t.border } else { Color32::TRANSPARENT }),
                    StrokeKind::Inside,
                );
            }
            ui.painter().galley(
                pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0),
                galley,
                t.text,
            );
            let cross = Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(18.0, 18.0));
            let cross_response = ui.interact(cross, egui::Id::new(("terminal_close", index)), Sense::click());
            if cross_response.hovered() {
                ui.painter().rect_filled(cross, 4.0, t.pressed);
            }
            ui.painter().text(
                cross.center(),
                Align2::CENTER_CENTER,
                "×",
                FontId::proportional(14.0),
                t.text_secondary,
            );
            if cross_response.clicked() {
                close = Some(index);
            } else if response.clicked() {
                select = Some(index);
            }
            response.context_menu(|ui| {
                ui::menu_begin(ui);
                select = Some(index);
                if ui::menu_item(ui, Some("sideBySide"), "Fractionner à droite", None, false).clicked() {
                    split = Some(false);
                    ui.close();
                }
                if ui::menu_item(ui, Some("unified"), "Fractionner vers le bas", None, false).clicked() {
                    split = Some(true);
                    ui.close();
                }
                ui::menu_separator(ui);
                if tab.count() > 1 && ui::menu_item(ui, None, "Fermer le volet", None, false).clicked() {
                    close_pane = true;
                    ui.close();
                }
                if ui::menu_item(ui, Some("closeSmall"), "Fermer l'onglet", None, false).clicked() {
                    close = Some(index);
                    ui.close();
                }
            });
            x += width + 4.0;
        }
        if let Some(index) = select {
            self.active_terminal = index;
        }
        if let Some(index) = close {
            self.close_terminal(index);
        }
        if hide {
            self.show_terminal = false;
        }
        if close_pane {
            self.close_pane();
        }
        if let Some(stacked) = split {
            self.split_terminal(stacked);
        }
        if let Some(shell) = new_shell {
            self.start_terminal(Some(shell));
        }
        if restart {
            self.restart_terminal();
        }
        // AIs started in the panes, to be offered the MCP server once the panes are drawn.
        let mut mcp_offers: Vec<(u64, crate::plugins::builtins::mcp::agents::Agent)> = Vec::new();
        let ai_watch = self.builtin_enabled("mcp");
        // The menu of the terminal area (right click): applied once the panes are drawn.
        let mut menu_split: Option<bool> = None;
        let mut menu_close_pane = false;
        let mut menu_close_tab = false;
        egui::Frame::new()
            .inner_margin(Margin {
                left: 14,
                right: 10,
                top: 2,
                bottom: 8,
            })
            .show(ui, |ui| {
                if !self.terminal_error.is_empty() {
                    ui.colored_label(t.error, &self.terminal_error);
                }
                let Some(tab) = self.terminals.get_mut(self.active_terminal) else {
                    return;
                };
                if tab.count() == 1
                    && tab.root.leaves().first().is_some_and(|p| p.term.exited)
                {
                    ui.colored_label(
                        t.warning,
                        "Session terminée · Redémarrer pour ouvrir un nouveau shell",
                    );
                }
                const GAP: f32 = 6.0;
                let full = ui.available_rect_before_wrap();
                let (rects, dividers) = tab.root.layout(full, GAP);
                let mut focused = None;
                let mut pane_drag = None;
                let split_view = tab.count() > 1;
                const HEAD: f32 = 22.0;
                let active_id = tab.active;
                for (pane, rect) in tab.root.leaves_mut().into_iter().zip(rects.iter()) {
                    if !pane.term.has_wake() {
                        let ctx = ctx.clone();
                        pane.term.on_output(move || ctx.request_repaint());
                    }
                    let mut body = *rect;
                    if split_view {
                        // The title of a pane: a handle to drag it elsewhere.
                        let head = Rect::from_min_size(rect.min, vec2(rect.width(), HEAD));
                        body.min.y += HEAD;
                        let response = ui.interact(head, egui::Id::new(("pane_head", pane.id)), Sense::click_and_drag());
                        ui.painter().rect_filled(head, 0.0, if pane.id == active_id { t.hover } else { t.frame });
                        ui.painter().text(
                            pos2(head.left() + 8.0, head.center().y),
                            Align2::LEFT_CENTER,
                            &pane.term.shell.name,
                            FontId::proportional(12.0),
                            if pane.id == active_id { t.text } else { t.text_secondary },
                        );
                        if response.clicked() {
                            focused = Some(pane.id);
                            pane.term.want_focus = true;
                        }
                        if response.dragged() && ui.input(|i| i.pointer.is_decidedly_dragging()) {
                            pane_drag = Some(pane.id);
                        }
                        response.on_hover_cursor(egui::CursorIcon::Grab);
                    }
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
                    pane.term.ai_watch = ai_watch;
                    let area = crate::terminal_ui::show(&mut child, &mut pane.term, &mut pane.selection, &t);
                    if let Some(agent) = pane.term.pending_ai.take() {
                        mcp_offers.push((pane.id, agent));
                    }
                    if area.secondary_clicked() {
                        focused = Some(pane.id);
                    }
                    area.context_menu(|ui| {
                        ui::menu_begin(ui);
                        if ui::menu_item(ui, Some("sideBySide"), "Fractionner à droite", None, false).clicked() {
                            menu_split = Some(false);
                            ui.close();
                        }
                        if ui::menu_item(ui, Some("unified"), "Fractionner vers le bas", None, false).clicked() {
                            menu_split = Some(true);
                            ui.close();
                        }
                        ui::menu_separator(ui);
                        if split_view && ui::menu_item(ui, None, "Fermer le volet", None, false).clicked() {
                            menu_close_pane = true;
                            ui.close();
                        }
                        if ui::menu_item(ui, Some("closeSmall"), "Fermer l'onglet", None, false).clicked() {
                            menu_close_tab = true;
                            ui.close();
                        }
                    });
                    if pane.term.focused {
                        focused = Some(pane.id);
                    }
                }
                if pane_drag.is_some() {
                    self.drag = pane_drag.map(Drag::Pane);
                }
                if let Some(id) = focused {
                    tab.active = id;
                }
                // Dragging a divider moves the split. Interacting after the panes, with a
                // wider area, makes it easy to grab.
                let mut moves = Vec::new();
                for (index, divider) in dividers.iter().enumerate() {
                    let grab = divider.rect.expand(if divider.stacked { 0.0 } else { 3.0 }).expand2(
                        if divider.stacked { vec2(0.0, 3.0) } else { vec2(0.0, 0.0) },
                    );
                    let response = ui
                        .interact(grab, egui::Id::new(("terminal_divider", index)), Sense::drag())
                        .on_hover_cursor(if divider.stacked {
                            egui::CursorIcon::ResizeVertical
                        } else {
                            egui::CursorIcon::ResizeHorizontal
                        });
                    let line = if divider.stacked {
                        Rect::from_center_size(divider.rect.center(), vec2(divider.rect.width(), 1.0))
                    } else {
                        Rect::from_center_size(divider.rect.center(), vec2(1.0, divider.rect.height()))
                    };
                    let color = if response.hovered() || response.dragged() { t.border } else { t.separator };
                    ui.painter().rect_filled(line, 0.0, color);
                    if let Some(pointer) = response.interact_pointer_pos()
                        && response.dragged()
                    {
                        let at = if divider.stacked { pointer.y } else { pointer.x };
                        moves.push((divider.path.clone(), divider.ratio_at(at, GAP)));
                    }
                }
                for (path, ratio) in moves {
                    if let Some(slot) = tab.root.ratio_mut(&path) {
                        *slot = ratio;
                    }
                }
                // A tab or a pane dragged over a pane shows where it would land.
                if let Some(drag) = self.drag
                    && let Some(pointer) = ctx.pointer_interact_pos()
                    && let Some((pane, rect)) = tab
                        .root
                        .leaves()
                        .into_iter()
                        .zip(rects.iter())
                        .find(|(_, r)| r.contains(pointer))
                    && match drag {
                        // The tab of this very terminal dragged on its panes splits them with a new shell.
                        Drag::Tab(_) => true,
                        Drag::Pane(id) => id != pane.id,
                    }
                {
                    let (stacked, before, zone) = drop_zone(*rect, pointer);
                    ui.painter().rect_filled(zone, 4.0, t.accent.gamma_multiply(0.3));
                    ui.painter().rect_stroke(zone, 4.0, Stroke::new(1.5_f32, t.accent), StrokeKind::Inside);
                    drop = Some((pane.id, stacked, before));
                }
                ui.allocate_rect(full, Sense::hover());
            });
        for (pane, agent) in mcp_offers {
            self.offer_mcp(pane, agent);
        }
        if let Some(stacked) = menu_split {
            self.split_terminal(stacked);
        }
        if menu_close_pane {
            self.close_pane();
        }
        if menu_close_tab {
            self.close_terminal(self.active_terminal);
        }
        // Over the tab bar, a dragged tab is reordered and a dragged pane gets its own tab.
        let bar = Rect::from_min_max(
            pos2(ui.min_rect().left(), top - 6.0),
            pos2(ui.max_rect().right(), top + 32.0),
        );
        let pointer = ctx.pointer_interact_pos();
        let in_bar = pointer.is_some_and(|p| bar.contains(p) && drop.is_none());
        let insert = pointer
            .filter(|_| in_bar)
            .map(|p| tab_rects.iter().filter(|r| r.center().x < p.x).count());
        if let (Some(at), Some(drag)) = (insert, self.drag) {
            let x = match tab_rects.get(at) {
                Some(r) => r.left() - 2.0,
                None => tab_rects.last().map_or(bar.left(), |r| r.right() + 2.0),
            };
            let is_tab = matches!(drag, Drag::Tab(_));
            if is_tab {
                ui.painter().rect_filled(
                    Rect::from_min_size(pos2(x, top), vec2(2.0, 26.0)),
                    1.0,
                    t.accent,
                );
            } else {
                ui.painter().rect_stroke(bar.shrink(2.0), 4.0, Stroke::new(1.5_f32, t.accent), StrokeKind::Inside);
            }
        }
        if let Some(drag) = self.drag
            && let Some(pointer) = pointer
        {
            // A label follows the pointer.
            let name = match drag {
                Drag::Tab(index) => self.terminals.get(index).map(|tab| tab.name().to_string()),
                Drag::Pane(id) => self
                    .terminals
                    .iter()
                    .flat_map(|tab| tab.root.leaves())
                    .find(|p| p.id == id)
                    .map(|p| p.term.shell.name.clone()),
            };
            if let Some(name) = name {
                ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("terminal_drag")))
                    .text(pointer + vec2(12.0, 12.0), Align2::LEFT_TOP, name, FontId::proportional(12.5), t.text);
            }
        }
        if ctx.input(|i| i.pointer.any_released())
            && let Some(drag) = self.drag.take()
        {
            match (drag, drop) {
                (Drag::Tab(from), Some((target, stacked, before))) if from == self.active_terminal => {
                    self.split_terminal_at(target, stacked, before)
                }
                (Drag::Tab(from), Some((target, stacked, before))) => {
                    self.merge_terminal_tab(from, target, stacked, before)
                }
                (Drag::Pane(id), Some((target, stacked, before))) => {
                    self.move_pane(id, target, stacked, before)
                }
                (Drag::Tab(from), None) => {
                    if let Some(at) = insert {
                        self.reorder_terminal_tab(from, at);
                    }
                }
                (Drag::Pane(id), None) => {
                    if in_bar {
                        self.pane_to_tab(id);
                    }
                }
            }
        } else if !ctx.input(|i| i.pointer.any_down()) {
            self.drag = None;
        }
    }

    /// Puts tab `from` at position `at` of the tab bar (counted before the move).
    fn reorder_terminal_tab(&mut self, from: usize, at: usize) {
        if from >= self.terminals.len() {
            return;
        }
        let to = if at > from { at - 1 } else { at }.min(self.terminals.len() - 1);
        if to == from {
            return;
        }
        let tab = self.terminals.remove(from);
        self.terminals.insert(to, tab);
        // The active tab follows its content.
        self.active_terminal = match self.active_terminal {
            a if a == from => to,
            a if from < a && a <= to => a - 1,
            a if to <= a && a < from => a + 1,
            a => a,
        };
    }

    /// Moves pane `id` of the active tab next to pane `target` of the same tab.
    fn move_pane(&mut self, id: u64, target: u64, stacked: bool, before: bool) {
        let Some(tab) = self.terminals.get_mut(self.active_terminal) else {
            return;
        };
        if id == target || tab.count() < 2 {
            return;
        }
        if let Some(pane) = tab.root.take(&|p: &Pane| p.id == id) {
            tab.root
                .split_with(&|p: &Pane| p.id == target, stacked, before, Node::Leaf(pane));
            tab.active = id;
            if let Some(pane) = tab.pane() {
                pane.term.want_focus = true;
            }
        }
    }

    /// Detaches pane `id` of the active tab into a tab of its own.
    fn pane_to_tab(&mut self, id: u64) {
        let Some(tab) = self.terminals.get_mut(self.active_terminal) else {
            return;
        };
        if tab.count() < 2 {
            return;
        }
        let Some(mut pane) = tab.root.take(&|p: &Pane| p.id == id) else {
            return;
        };
        if let Some(first) = tab.root.leaves().first() {
            tab.active = first.id;
        }
        pane.term.want_focus = true;
        self.terminals.push(TerminalTab {
            active: pane.id,
            root: Node::Leaf(pane),
        });
        self.active_terminal = self.terminals.len() - 1;
    }

    /// Moves the panes of tab `from` next to pane `target` of the active tab.
    fn merge_terminal_tab(&mut self, from: usize, target: u64, stacked: bool, before: bool) {
        if from >= self.terminals.len() || from == self.active_terminal {
            return;
        }
        let moved = self.terminals.remove(from);
        let active = self.active_terminal - usize::from(from < self.active_terminal);
        let tab = &mut self.terminals[active];
        let id = moved.active;
        if tab.root.split_with(&|p: &Pane| p.id == target, stacked, before, moved.root) {
            tab.active = id;
            if let Some(pane) = tab.pane() {
                pane.term.want_focus = true;
            }
        }
        self.active_terminal = active;
    }

    fn right_panel(&mut self, ctx: &egui::Context) {
        if !self.show_notifications {
            return;
        }
        let response = egui::SidePanel::right("notifications")
            .default_width(340.0)
            .width_range(240.0..=600.0)
            .frame(self.island(ctx))
            .show_separator_line(false)
            .show(ctx, |ui| {
                let hide = self.tool_header(ui, "Notifications", |ide, ui| {
                    if ui::icon_button(ui, "delete", "Tout effacer").clicked() {
                        ide.notes.clear();
                        ide.balloons.clear();
                    }
                });
                if hide {
                    self.show_notifications = false;
                }
                let t = self.theme(ui.ctx());
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.notes.is_empty() {
                            ui.add_space(30.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new("Aucune notification").color(t.text_secondary),
                                );
                            });
                        }
                        for note in self.notes.iter().rev() {
                            egui::Frame::new()
                                .inner_margin(Margin::symmetric(14, 8))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.add(ui::icon(
                                            ui,
                                            if note.error { "error" } else { "info" },
                                        ));
                                        ui.label(
                                            RichText::new(&note.title).font(ui::semibold(13.0)),
                                        );
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new(ago(note.at))
                                                        .small()
                                                        .color(t.text_secondary),
                                                );
                                            },
                                        );
                                    });
                                    ui.label(&note.text);
                                });
                            ui.separator();
                        }
                    });
            });
        self.panel_focus(Some("notifications"), response.response.rect);
    }

    fn balloons(&mut self, ctx: &egui::Context) {
        let t = self.theme(ctx);
        self.balloons.retain(|&i| {
            self.notes
                .get(i)
                .is_some_and(|n| n.error || n.at.elapsed() < Duration::from_secs(6))
        });
        if self.balloons.is_empty() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(500));
        let mut close = None;
        let mut open_log = false;
        egui::Area::new(egui::Id::new("balloons"))
            .anchor(Align2::RIGHT_BOTTOM, vec2(-16.0, -(STATUS_HEIGHT + 12.0)))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                for &index in self.balloons.iter().rev().take(3) {
                    let note = &self.notes[index];
                    egui::Frame::popup(ui.style())
                        .fill(t.popup)
                        .corner_radius(10)
                        .inner_margin(Margin::symmetric(14, 12))
                        .show(ui, |ui| {
                            ui.set_width(360.0);
                            ui.horizontal(|ui| {
                                ui.add(ui::icon(ui, if note.error { "error" } else { "info" }));
                                ui.label(RichText::new(&note.title).font(ui::semibold(13.0)));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui::icon_button(ui, "closeSmall", "Fermer").clicked() {
                                            close = Some(index);
                                        }
                                    },
                                );
                            });
                            let text: String = note.text.chars().take(600).collect();
                            ui.label(text);
                            if note.error && ui.link("Afficher les notifications").clicked() {
                                open_log = true;
                            }
                        });
                    ui.add_space(8.0);
                }
            });
        if let Some(index) = close {
            self.balloons.retain(|&i| i != index);
        }
        if open_log {
            self.show_notifications = true;
            self.balloons.clear();
        }
    }

    // ---- Editor -------------------------------------------------------------------------

    fn editor(&mut self, ctx: &egui::Context) {
        let gutter = self.gutter();
        let mut gutter_command = None;
        let mut navigate: Option<(crate::languages::Location, Option<PathBuf>, usize)> = None;
        let mut file_edits: Vec<(PathBuf, Vec<crate::languages::TextEdit>)> = Vec::new();
        let mut new_layout = None;
        let t = self.theme(ctx);
        let response = egui::CentralPanel::default()
            .frame(self.island(ctx))
            .show(ctx, |ui| {
                if self.documents.is_empty() {
                    self.welcome(ui);
                    return;
                }
                self.tabs(ui);
                let Some(doc) = self.documents.get_mut(self.active) else {
                    return;
                };
                if let Some(loading) = &doc.loading {
                    ui.add(
                        egui::ProgressBar::new(loading.progress())
                            .desired_height(3.0)
                            .corner_radius(0),
                    );
                    return;
                }
                let mut find_action = self.find_request.take().unwrap_or(crate::find::Action::None);
                if self.find.open {
                    self.find.refresh(self.active, doc.view.revision(), &doc.text);
                    let selection = doc.view.selection();
                    egui::Frame::new()
                        .fill(t.frame)
                        .inner_margin(Margin::symmetric(10, 4))
                        .show(ui, |ui| {
                            let action = self.find.ui(ui, &t, &selection);
                            if action != crate::find::Action::None {
                                find_action = action;
                            }
                        });
                } else if find_action != crate::find::Action::None && !self.find.query.is_empty() {
                    self.find.refresh(self.active, doc.view.revision(), &doc.text);
                }
                // A plugin previewing this document (Markdown): layout switch and preview.
                let preview = doc.path.clone().and_then(|path| {
                    self.builtins
                        .iter()
                        .enumerate()
                        .filter(|(_, p)| !self.settings.disabled_builtins.contains(p.id()))
                        .find_map(|(i, p)| {
                            p.editor_layout(&path)
                                .map(|layout| (i, path.clone(), layout))
                        })
                });
                if let Some((plugin, path, layout)) = &preview {
                  if self.builtins[*plugin].layout_switch(path) {
                    let width = 3.0 * ui::BUTTON + 6.0;
                    let pos = pos2(
                        ui.max_rect().right() - width - 22.0,
                        ui.cursor().top() + 8.0,
                    );
                    egui::Area::new(egui::Id::new("editor_layout"))
                        .fixed_pos(pos)
                        .order(egui::Order::Middle)
                        .show(ctx, |ui| {
                            egui::Frame::new()
                                .fill(t.frame)
                                .stroke(Stroke::new(1.0_f32, t.border))
                                .corner_radius(7)
                                .inner_margin(2)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 0.0;
                                        for (choice, icon, tooltip) in [
                                            (EditorLayout::Text, "editorOnly", "Éditeur seul"),
                                            (
                                                EditorLayout::Split,
                                                "editorPreview",
                                                "Éditeur et aperçu",
                                            ),
                                            (EditorLayout::Preview, "previewOnly", "Aperçu seul"),
                                        ] {
                                            if ui::icon_toggle(ui, icon, *layout == choice, tooltip)
                                                .clicked()
                                            {
                                                new_layout = Some((*plugin, path.clone(), choice));
                                            }
                                        }
                                    });
                                });
                        });
                  }
                    match layout {
                        EditorLayout::Preview => {
                            let sync = self.scroll_sync.take();
                            self.builtins[*plugin].preview(ui, path, &mut doc.text, sync);
                            return;
                        }
                        EditorLayout::Split => {
                            let sync = self.scroll_sync.take();
                            let half = (ui.available_width() / 2.0).max(200.0);
                            egui::SidePanel::right(egui::Id::new(("preview", path)))
                                .resizable(true)
                                .default_width(half)
                                .width_range(200.0..=(ui.available_width() - 200.0).max(220.0))
                                .frame(egui::Frame::NONE)
                                .show_inside(ui, |ui| {
                                    self.builtins[*plugin].preview(ui, path, &mut doc.text, sync);
                                });
                        }
                        EditorLayout::Text => {}
                    }
                }
                let split = preview.as_ref().is_some_and(|p| p.2 == EditorLayout::Split);
                // Ctrl + wheel over the editor zooms the font (egui reports it as a zoom).
                if !self.settings.wheel_zoom_off && ui.rect_contains_pointer(ui.max_rect()) {
                    let zoom = ui.ctx().input(|i| i.zoom_delta());
                    if (zoom - 1.0).abs() > f32::EPSILON {
                        // One point per notch of a wheel; a touchpad accumulates small steps.
                        let id = egui::Id::new("editor_wheel_zoom");
                        let total = ui.data_mut(|d| {
                            let acc = d.get_temp_mut_or_default::<f32>(id);
                            *acc += zoom.ln();
                            let steps = (*acc / 0.1).trunc();
                            *acc -= steps * 0.1;
                            steps
                        });
                        let size = (self.settings.font_size + total).clamp(8.0, 36.0);
                        if size != self.settings.font_size {
                            self.settings.font_size = size;
                            self.zoom_hint = Some(Instant::now());
                            ui::install(ui.ctx(), ui::theme_by_index(self.settings.theme), size);
                        }
                    }
                }
                if let Some(at) = self.zoom_hint {
                    const SHOW_FOR: Duration = Duration::from_millis(1200);
                    if at.elapsed() >= SHOW_FOR {
                        self.zoom_hint = None;
                    } else {
                        let painter = ui.ctx().layer_painter(egui::LayerId::new(
                            egui::Order::Tooltip,
                            egui::Id::new("zoom_hint"),
                        ));
                        let galley = painter.layout_no_wrap(
                            format!("{} px", self.settings.font_size),
                            ui::semibold(13.0),
                            t.text,
                        );
                        let rect = Rect::from_min_size(
                            pos2(ui.max_rect().right() - galley.size().x - 40.0, ui.max_rect().top() + 14.0),
                            galley.size() + vec2(24.0, 14.0),
                        );
                        painter.rect(
                            rect,
                            6.0,
                            t.popup,
                            Stroke::new(1.0_f32, t.border),
                            StrokeKind::Inside,
                        );
                        painter.galley(rect.min + vec2(12.0, 7.0), galley, t.text);
                        ui.ctx().request_repaint_after(SHOW_FOR - at.elapsed());
                    }
                }
                let font = FontId::monospace(self.settings.font_size);
                let annotations = gutter.annotations.as_deref();
                let assist = self.languages.handles(&doc.language).then(|| crate::editor::Assist {
                    service: &self.languages,
                    doc: crate::languages::DocRef {
                        key: doc_key(&doc.path, self.active),
                        path: doc.path.clone(),
                        language: doc.language.clone(),
                    },
                });
                let options = crate::editor::Options {
                    language: &doc.language,
                    font,
                    theme: t,
                    marks: &gutter.marks,
                    annotations,
                    gutter_commands: &gutter.commands,
                    assist,
                    search: if self.find.open { self.find.matches() } else { &[] },
                };
                let editor_id = egui::Id::new(("source", doc.path.clone(), self.active));
                let output = doc.view.show(ui, editor_id, &mut doc.text, &options);
                self.cursor = (output.line, output.column);
                self.problems = (output.errors, output.warnings);
                if let Some(target) = output.navigate {
                    navigate = Some((target, doc.path.clone(), doc.view.cursor));
                }
                file_edits = output.file_edits;
                if output.command.is_some() {
                    gutter_command = output.command;
                }
                {
                    use crate::find::Action;
                    let selection = doc.view.selection();
                    match find_action {
                        Action::Next | Action::Previous => {
                            if let Some(found) = self.find.neighbor(&selection, find_action == Action::Next) {
                                doc.view.select(found.start, found.end);
                            }
                        }
                        Action::Replace => {
                            // The selected match is replaced, then the next one is selected.
                            if self.find.position(&selection).is_some()
                                && let Some(re) = self.find.regex().cloned()
                            {
                                let new = crate::search::replace_all(&doc.text[selection.clone()], &re, &self.find.replace, self.find.options);
                                doc.view.replace_range(&mut doc.text, selection.clone(), &new);
                                self.find.refresh(self.active, doc.view.revision(), &doc.text);
                                let caret = doc.view.selection();
                                if let Some(next) = self.find.neighbor(&(caret.start..caret.start), true) {
                                    doc.view.select(next.start, next.end);
                                }
                            } else if let Some(found) = self.find.neighbor(&selection, true) {
                                doc.view.select(found.start, found.end);
                            }
                        }
                        Action::ReplaceAll => {
                            if let Some(re) = self.find.regex().cloned() {
                                let new = crate::search::replace_all(&doc.text, &re, &self.find.replace, self.find.options);
                                if new != doc.text {
                                    let count = self.find.matches().len();
                                    doc.view.replace_text(&mut doc.text, &new);
                                    self.status = format!("{count} remplacement(s)");
                                }
                            }
                        }
                        Action::Close => self.find.open = false,
                        Action::None => {}
                    }
                }
                if split {
                    let fraction = doc.view.scroll_fraction;
                    if output.scrolled && (fraction - self.editor_scroll).abs() > 0.0005 {
                        self.editor_scroll = fraction;
                        self.scroll_sync = Some(fraction);
                        ctx.request_repaint();
                    }
                }
            });
        self.panel_focus(None, response.response.rect);
        if let Some((plugin, path, layout)) = new_layout {
            self.builtins[plugin].set_editor_layout(&path, layout);
        }
        if let Some((plugin, command)) = gutter_command {
            self.run_builtin_command(plugin, command);
        }
        if let Some((target, from, offset)) = navigate {
            self.go_to(target, from, offset);
        }
        if !file_edits.is_empty() {
            self.apply_file_edits(file_edits);
        }
    }

    fn tabs(&mut self, ui: &mut egui::Ui) {
        let t = self.theme(ui.ctx());
        let mut close = None;
        let mut select = None;
        let mut action = None;
        let mut tab_rects: Vec<Rect> = Vec::new();
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(bar.shrink2(vec2(6.0, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let more = ui::icon_button(ui, "moreVertical", "Options des onglets");
            egui::Popup::menu(&more)
                .id(egui::Id::new("tabs_more"))
                .show(|ui| {
                    ui::menu_begin(ui);
                    if ui::menu_item(ui, Some("closeSmall"), "Fermer l'onglet", None, false).clicked() {
                        close = Some(self.active);
                        ui.close();
                    }
                    if ui::menu_item(ui, None, "Fermer les autres onglets", None, false).clicked() {
                        // Modified documents stay open, like the active one.
                        let keep = self.active;
                        // The active tab moves left by the number of closed tabs before it.
                        self.active = self.documents[..keep].iter().filter(|d| d.dirty()).count();
                        let mut index = 0;
                        self.documents.retain(|d| {
                            let kept = index == keep || d.dirty();
                            index += 1;
                            kept
                        });
                        ui.close();
                    }
                    if ui::menu_item(ui, None, "Tout fermer", None, false).clicked() {
                        self.documents.retain(Document::dirty);
                        self.active = 0;
                        ui.close();
                    }
                });
            let list = ui::icon_button(ui, "chevronDown", "Onglets ouverts");
            egui::Popup::menu(&list)
                .id(egui::Id::new("tabs_list"))
                .show(|ui| {
                    ui::menu_begin(ui);
                    for (i, doc) in self.documents.iter().enumerate() {
                        let mark = (i == self.active).then_some("greenCheckmark");
                        if ui::menu_item(ui, mark, &doc.title(), None, false).clicked() {
                            select = Some(i);
                            ui.close();
                        }
                    }
                });
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                egui::ScrollArea::horizontal()
                    .id_salt("tabs")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        for (i, doc) in self.documents.iter().enumerate() {
                            let active = i == self.active;
                            let title = doc.title();
                            let path = doc.path.clone().unwrap_or_default();
                            let vcs = self
                                .builtins
                                .iter()
                                .filter(|p| self.builtin_enabled(p.id()))
                                .find_map(|p| p.file_color(&path));
                            let color = vcs.unwrap_or(t.text);
                            let galley = ui.painter().layout_no_wrap(
                                title.clone(),
                                FontId::proportional(13.0),
                                color,
                            );
                            let size =
                                vec2(10.0 + 16.0 + 6.0 + galley.size().x + 6.0 + 16.0 + 8.0, 28.0);
                            let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
                            tab_rects.push(rect);
                            if response.dragged() && ui.input(|i| i.pointer.is_decidedly_dragging()) {
                                self.doc_drag = Some(i);
                            }
                            if active {
                                ui.painter().rect(
                                    rect,
                                    6.0,
                                    t.frame,
                                    Stroke::new(1.0_f32, t.border),
                                    StrokeKind::Inside,
                                );
                            } else if response.hovered() {
                                ui.painter().rect_filled(rect, 6.0, t.hover);
                            }
                            let icon = Rect::from_min_size(
                                pos2(rect.left() + 10.0, rect.center().y - 8.0),
                                vec2(16.0, 16.0),
                            );
                            egui::Image::new(self.file_icon(ui.ctx(), &path)).paint_at(ui, icon);
                            ui.painter().galley(
                                pos2(icon.right() + 6.0, rect.center().y - galley.size().y / 2.0),
                                galley,
                                color,
                            );
                            let close_rect = Rect::from_min_size(
                                pos2(rect.right() - 24.0, rect.center().y - 8.0),
                                vec2(16.0, 16.0),
                            );
                            let close_response = ui.interact(
                                close_rect,
                                egui::Id::new(("tab_close", i)),
                                Sense::click(),
                            );
                            if doc.dirty() && !response.hovered() && !close_response.hovered() {
                                ui.painter().circle_filled(
                                    close_rect.center(),
                                    3.5,
                                    t.text_secondary,
                                );
                            } else if active || response.hovered() || close_response.hovered() {
                                let icon = if close_response.hovered() {
                                    "closeSmallHovered"
                                } else {
                                    "closeSmall"
                                };
                                ui::paint_icon(ui, icon, close_rect);
                            }
                            if close_response.clicked() || response.middle_clicked() {
                                close = Some(i);
                            } else if response.clicked() {
                                select = Some(i);
                            }
                            let tooltip = doc.path.as_deref().map_or("Sans titre".into(), |p| {
                                project::simplify(p).display().to_string()
                            });
                            let has_path = doc.path.is_some();
                            response.context_menu(|ui| {
                                ui::menu_begin(ui);
                                if ui::menu_item(ui, Some("closeSmall"), "Fermer", Some("Ctrl+W"), false).clicked() {
                                    close = Some(i);
                                    ui.close();
                                }
                                if ui::menu_item(ui, None, "Fermer les autres", None, false).clicked() {
                                    action = Some(TabAction::CloseOthers(i));
                                    ui.close();
                                }
                                if ui::menu_item(ui, None, "Fermer les onglets à droite", None, false).clicked() {
                                    action = Some(TabAction::CloseRight(i));
                                    ui.close();
                                }
                                if ui::menu_item(ui, None, "Tout fermer", None, false).clicked() {
                                    action = Some(TabAction::CloseAll);
                                    ui.close();
                                }
                                if has_path {
                                    ui::menu_separator(ui);
                                    if ui::menu_item(ui, Some("copy"), "Copier le chemin", None, false).clicked() {
                                        action = Some(TabAction::CopyPath(i));
                                        ui.close();
                                    }
                                    if ui::menu_item(ui, Some("project"), "Afficher dans le projet", None, false).clicked() {
                                        action = Some(TabAction::Reveal(i));
                                        ui.close();
                                    }
                                    if ui::menu_item(ui, Some("locate"), "Afficher dans l'explorateur", None, false).clicked() {
                                        action = Some(TabAction::Explorer(i));
                                        ui.close();
                                    }
                                }
                            });
                            response.on_hover_text(tooltip);
                        }
                    });
            });
        });
        if let Some(i) = select {
            self.active = i;
        }
        if let Some(i) = close {
            self.request(Pending::Close(i));
        }
        if let Some(action) = action {
            self.tab_action(action);
        }
        self.tab_drag(ui, bar, &tab_rects, &t);
    }

    /// Drag along the tab bar reorders the editor tabs.
    fn tab_drag(&mut self, ui: &egui::Ui, bar: Rect, rects: &[Rect], t: &Theme) {
        let Some(from) = self.doc_drag else { return };
        let ctx = ui.ctx();
        let pointer = ctx.pointer_interact_pos().filter(|p| bar.contains(*p));
        let at = pointer.map(|p| rects.iter().filter(|r| r.center().x < p.x).count());
        if let Some(at) = at {
            let x = match rects.get(at) {
                Some(r) => r.left() - 1.0,
                None => rects.last().map_or(bar.left(), |r| r.right() + 1.0),
            };
            ui.painter().rect_filled(
                Rect::from_min_size(pos2(x, bar.center().y - 14.0), vec2(2.0, 28.0)),
                1.0,
                t.accent,
            );
        }
        if ctx.input(|i| i.pointer.any_released()) {
            self.doc_drag = None;
            if let Some(at) = at
                && from < self.documents.len()
            {
                let to = if at > from { at - 1 } else { at }.min(self.documents.len() - 1);
                if to != from {
                    let doc = self.documents.remove(from);
                    self.documents.insert(to, doc);
                    // The active tab follows its content.
                    self.active = match self.active {
                        a if a == from => to,
                        a if from < a && a <= to => a - 1,
                        a if to <= a && a < from => a + 1,
                        a => a,
                    };
                }
            }
        } else if !ctx.input(|i| i.pointer.any_down()) {
            self.doc_drag = None;
        }
    }

    /// Closes the documents `keep` rejects; modified ones always stay.
    fn retain_documents(&mut self, keep: impl Fn(usize) -> bool) {
        let (mut index, mut kept, mut active) = (0, 0, None);
        let current = self.active;
        self.documents.retain(|d| {
            let stay = d.dirty() || keep(index);
            if stay {
                if index == current {
                    active = Some(kept);
                }
                kept += 1;
            }
            index += 1;
            stay
        });
        self.active = active.unwrap_or(current.min(self.documents.len().saturating_sub(1)));
    }

    fn tab_action(&mut self, action: TabAction) {
        let path_of = |ide: &Ide, i: usize| ide.documents.get(i).and_then(|d| d.path.clone());
        match action {
            TabAction::CloseOthers(i) => {
                self.active = i.min(self.documents.len().saturating_sub(1));
                self.retain_documents(|n| n == i);
            }
            TabAction::CloseRight(i) => self.retain_documents(|n| n <= i),
            TabAction::CloseAll => self.retain_documents(|_| false),
            TabAction::CopyPath(i) => {
                if let Some(path) = path_of(self, i) {
                    self.pending_copy = Some(project::simplify(&path).display().to_string());
                }
            }
            TabAction::Reveal(i) => {
                if let Some(path) = path_of(self, i) {
                    self.reveal(&path);
                }
            }
            TabAction::Explorer(i) => {
                if let Some(path) = path_of(self, i) {
                    let target = project::simplify(&path);
                    #[cfg(windows)]
                    let _ = std::process::Command::new("explorer")
                        .arg(format!("/select,{}", target.display()))
                        .spawn();
                    #[cfg(not(windows))]
                    let _ = std::process::Command::new("xdg-open")
                        .arg(target.parent().unwrap_or(&target))
                        .spawn();
                }
            }
        }
    }

    fn welcome(&mut self, ui: &mut egui::Ui) {
        let t = self.theme(ui.ctx());
        let rect = ui.max_rect();
        let lines = [
            ("Rechercher partout", "Double Maj"),
            ("Aller au fichier", "Ctrl+Maj+N"),
            ("Ouvrir un fichier", "Ctrl+O"),
            ("Nouveau fichier", "Ctrl+N"),
            ("Terminal", "Ctrl+`"),
            ("Commit", "Ctrl+K"),
        ];
        let top = rect.center().y - lines.len() as f32 * 15.0 - 20.0;
        for (i, (label, shortcut)) in lines.iter().enumerate() {
            let y = top + i as f32 * 30.0;
            ui.painter().text(
                pos2(rect.center().x - 6.0, y),
                Align2::RIGHT_CENTER,
                *label,
                FontId::proportional(13.5),
                t.text_secondary,
            );
            ui.painter().text(
                pos2(rect.center().x + 6.0, y),
                Align2::LEFT_CENTER,
                *shortcut,
                FontId::proportional(13.5),
                t.link,
            );
        }
        ui.painter().text(
            pos2(rect.center().x, top + lines.len() as f32 * 30.0 + 16.0),
            Align2::CENTER_CENTER,
            "Déposez des fichiers ici pour les ouvrir",
            FontId::proportional(13.5),
            t.text_secondary,
        );
    }

    // ---- Status bar ---------------------------------------------------------------------

    fn status_bar(&mut self, ctx: &egui::Context) {
        let t = self.theme(ctx);
        let mut open_problems = false;
        egui::TopBottomPanel::bottom("status")
            .exact_height(STATUS_HEIGHT)
            .frame(
                egui::Frame::new()
                    .fill(t.frame)
                    .inner_margin(Margin::symmetric(8, 0)),
            )
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    // Breadcrumbs: project, folders, file.
                    let mut crumbs = vec![(
                        self.root
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                        self.root.clone(),
                    )];
                    if let Some(path) = self.documents.get(self.active).and_then(|d| d.path.clone())
                    {
                        let relative = path
                            .strip_prefix(&self.root)
                            .map(Path::to_path_buf)
                            .unwrap_or_else(|_| path.clone());
                        let mut current = self.root.clone();
                        for part in relative.components() {
                            current.push(part);
                            crumbs.push((
                                part.as_os_str().to_string_lossy().into_owned(),
                                current.clone(),
                            ));
                        }
                    }
                    let last = crumbs.len() - 1;
                    for (i, (name, path)) in crumbs.into_iter().enumerate() {
                        if i > 0 {
                            let (rect, _) =
                                ui.allocate_exact_size(vec2(14.0, 16.0), Sense::hover());
                            egui::Image::new(ui::icon_source("chevronRight", t.dark))
                                .tint(Color32::from_white_alpha(150))
                                .paint_at(ui, rect.expand(1.0));
                        }
                        if ui::status_text(ui, &name).clicked() && i < last {
                            self.show_project = true;
                            self.left_tool = None;
                            self.tree_select_only(&path);
                            let mut dir = Some(path.as_path());
                            while let Some(d) = dir.filter(|d| d.starts_with(&self.root)) {
                                self.expanded.insert(d.to_path_buf());
                                dir = d.parent();
                            }
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        // Memory and processor use of the IDE, in the corner.
                        if let Some(summary) = crate::stats::summary() {
                            ui::status_text_fixed(ui, &summary, "RAM 9999 Mo · CPU 99,9 %").on_hover_text(format!(
                                "Mémoire (RAM) et processeur utilisés par Atelier
{} · {:.1} % de la capacité totale du processeur",
                                crate::stats::format_memory(crate::stats::memory_bytes()),
                                crate::stats::cpu_percent()
                            ));
                            ui.add_space(8.0);
                            ctx.request_repaint_after(Duration::from_secs(1));
                        }
                        // Background tasks, next to it, as IntelliJ shows them.
                        let running = crate::tasks::running();
                        if let Some(first) = running.first() {
                            let more = if running.len() > 1 { format!(" (+{})", running.len() - 1) } else { String::new() };
                            let text = ui
                                .label(RichText::new(format!("{}{more}…", first.title)).size(12.5).color(t.text_secondary))
                                .on_hover_ui(|ui| {
                                    for task in &running {
                                        ui.label(format!("{} · {} s", task.title, task.elapsed.as_secs()));
                                    }
                                });
                            let _ = text;
                            ui.add(egui::Spinner::new().size(12.0).color(t.text_secondary));
                            ui.add_space(10.0);
                            ctx.request_repaint_after(Duration::from_millis(250));
                        }
                        if self.graph.enabled() {
                            let state = self.graph.status();
                            let text = if state.updated == 0 {
                                "Graphe de contexte en préparation".to_string()
                            } else {
                                format!("Graphe de contexte : {} fichiers · {} éléments · {} groupes", state.files, state.symbols, state.groups)
                            };
                            ui::status_text(ui, "Graphe").on_hover_text(text);
                        }
                        self.each_builtin(|plugin, ide| plugin.status_bar(ui, ide));
                        if let Some(doc) = self.documents.get(self.active) {
                            // Sampled: a full scan on every frame freezes big files.
                            let sample = doc.text.lines().take(2000).collect::<Vec<_>>().join("\n");
                            let indent = if sample.contains("\n\t") {
                                "Tabulation".to_string()
                            } else {
                                let width = sample
                                    .lines()
                                    .filter_map(|l| {
                                        let n = l.len() - l.trim_start_matches(' ').len();
                                        (n > 0 && !l.trim().is_empty()).then_some(n)
                                    })
                                    .min()
                                    .unwrap_or(4);
                                format!("{width} espaces")
                            };
                            ui::status_text(ui, &indent);
                            ui::status_text(ui, "UTF-8");
                            ui::status_text(
                                ui,
                                if doc.text.contains("\r\n") {
                                    "CRLF"
                                } else {
                                    "LF"
                                },
                            );
                            ui::status_text(ui, &format!("{}:{}", self.cursor.0, self.cursor.1));
                            let (errors, warnings) = self.problems;
                            if errors + warnings > 0 {
                                let mut parts = Vec::new();
                                if errors > 0 {
                                    parts.push(format!("{errors} erreur{}", if errors > 1 { "s" } else { "" }));
                                }
                                if warnings > 0 {
                                    parts.push(format!("{warnings} avertissement{}", if warnings > 1 { "s" } else { "" }));
                                }
                                ui.add_space(10.0);
                                let label = ui
                                    .add(
                                        egui::Label::new(
                                            RichText::new(parts.join(", "))
                                                .size(12.5)
                                                .color(if errors > 0 { Color32::from_rgb(0xF0, 0x56, 0x56) } else { Color32::from_rgb(0xE6, 0xB5, 0x4A) }),
                                        )
                                        .sense(Sense::click()),
                                    )
                                    .on_hover_text("Afficher les problèmes · Alt+6")
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                if label.clicked() {
                                    open_problems = true;
                                }
                            }
                            let engine = self.languages.status(&doc.language);
                            if !engine.is_empty() {
                                ui::status_text(ui, &engine);
                            }
                        }
                    });
                });
            });
        if open_problems {
            self.open_tool(&crate::features::problems::TOOL_WINDOWS[0]);
        }
    }

    // ---- Dialogs and popups -------------------------------------------------------------

    fn dialogs(&mut self, ctx: &egui::Context) {
        self.settings_dialog(ctx);
        self.tree_dialogs(ctx);
        if self.show_new_file {
            let modal = egui::Modal::new(egui::Id::new("new_file")).show(ctx, |ui| {
                ui.set_width(420.0);
                ui::dialog_title(ui, "Nouveau fichier");
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.new_name)
                        .hint_text("src/main.rs")
                        .desired_width(f32::INFINITY),
                );
                field.request_focus();
                ui.label(
                    RichText::new("Chemin relatif au projet · les dossiers manquants seront créés")
                        .small()
                        .color(self.theme(ui.ctx()).text_secondary),
                );
                ui::dialog_buttons(ui, "Créer")
            });
            let (ok, cancel) = modal.inner;
            if ok {
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
            } else if cancel || modal.should_close() {
                self.show_new_file = false;
            }
        }
        self.palette(ctx);
        self.each_builtin(|plugin, ide| plugin.windows(ctx, ide));
        if let Some((pane, agent)) = self.mcp_offer {
            let modal = egui::Modal::new(egui::Id::new("mcp_offer")).show(ctx, |ui| {
                ui.set_width(500.0);
                ui::dialog_title(ui, &format!("Connecter {} à Atelier ?", agent.name()));
                ui.label(format!(
                    "Atelier peut donner à {} l'accès à ses outils : fichiers ouverts, éditeur actif, problèmes du code, \
                     recherche dans le projet, ouverture d'un fichier à une ligne.",
                    agent.name()
                ));
                ui.add_space(6.0);
                ui.label(
                    RichText::new(match agent {
                        crate::plugins::builtins::mcp::agents::Agent::Codex => {
                            "La connexion s'écrit dans .codex/config.toml du projet (sans secret). Codex doit faire confiance au projet."
                        }
                        _ => "La connexion s'écrit dans les fichiers de configuration du projet, sans y mettre de secret.",
                    })
                        .small()
                        .color(self.theme(ui.ctx()).text_secondary),
                );
                ui.add_space(12.0);
                let mut choice = None;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui::primary_button(ui, "Connecter").clicked() {
                        choice = Some(1);
                    }
                    if ui::button(ui, "Pas maintenant").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        choice = Some(0);
                    }
                    if ui::button(ui, "Ne plus proposer").clicked() {
                        choice = Some(2);
                    }
                });
                choice
            });
            let key = format!("{}|{}", self.root.display(), agent.id());
            let choice = modal.inner.or_else(|| modal.should_close().then_some(0));
            if let Some(choice) = choice {
                self.mcp_offer = None;
                match choice {
                    1 => {
                        self.settings.mcp_choices.insert(key, true);
                        self.connect_mcp(agent);
                    }
                    2 => {
                        self.settings.mcp_choices.insert(key, false);
                    }
                    _ => {}
                }
                self.release_enter(pane);
            }
        }
        if let Some(path) = self.project_choice.clone() {
            let modal = egui::Modal::new(egui::Id::new("project_choice")).show(ctx, |ui| {
                ui.set_width(460.0);
                ui::dialog_title(ui, "Ouvrir le projet");
                ui.label(format!(
                    "Ouvrir « {} » dans cette fenêtre ou dans une nouvelle fenêtre ?",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ));
                ui.add_space(12.0);
                let mut choice = None;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui::button(ui, "Annuler").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        choice = Some(0);
                    }
                    if ui::button(ui, "Cette fenêtre").clicked() {
                        choice = Some(1);
                    }
                    if ui::primary_button(ui, "Nouvelle fenêtre").clicked() {
                        choice = Some(2);
                    }
                });
                choice
            });
            match modal.inner {
                Some(1) => {
                    self.project_choice = None;
                    self.request(Pending::Project(path));
                }
                Some(2) => {
                    self.project_choice = None;
                    self.open_window(path);
                }
                Some(_) => self.project_choice = None,
                None if modal.should_close() => self.project_choice = None,
                None => {}
            }
        }
        if let Some(action) = self.pending.clone() {
            egui::Modal::new(egui::Id::new("unsaved")).show(ctx, |ui| {
                ui.set_width(460.0);
                ui::dialog_title(ui, "Modifications non enregistrées");
                ui.label("Enregistrer les fichiers modifiés avant de continuer ?");
                ui.add_space(4.0);
                for (i, doc) in self.documents.iter().enumerate() {
                    if doc.dirty() && (!matches!(action, Pending::Close(index) if index != i)) {
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::Image::new(self.file_icon(ui.ctx(), 
                                    doc.path.as_deref().unwrap_or(Path::new("")),
                                ))
                                .fit_to_exact_size(vec2(16.0, 16.0)),
                            );
                            ui.label(doc.title());
                        });
                    }
                }
                ui.add_space(12.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui::button(ui, "Annuler").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        self.pending = None;
                    }
                    if ui::button(ui, "Ne pas enregistrer").clicked() {
                        self.pending = None;
                        self.perform(action.clone());
                    }
                    if ui::primary_button(ui, "Enregistrer").clicked() {
                        let saved = match action {
                            Pending::Close(i) => self.save_document(i, false),
                            _ => self.save_all(),
                        };
                        if saved {
                            self.pending = None;
                            self.perform(action.clone());
                        }
                    }
                });
            });
        }
    }

    fn settings_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let t = self.theme(ctx);
        let mut close = false;
        let mut command = None;
        let modal = egui::Modal::new(egui::Id::new("settings")).show(ctx, |ui| {
            ui.set_width(920.0);
            ui.set_height(640.0);
            ui::dialog_title(ui, "Paramètres");
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(180.0);
                    for (index, page) in ["Apparence", "Éditeur", "Raccourcis clavier", "Plugins"].iter().enumerate() {
                        let (rect, response) = ui::row(ui, self.settings_page == index);
                        ui::paint_text(ui, rect, rect.left() + 14.0, page, t.text, FontId::proportional(13.0));
                        if response.clicked() {
                            self.settings_page = index;
                        }
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_width(700.0);
                    ui.set_min_height(550.0);
                    egui::ScrollArea::vertical().max_height(550.0).show(ui, |ui| match self.settings_page {
                        0 => {
                            ui::section(ui, "Thème");
                            for (index, name) in ui::THEMES.iter().enumerate() {
                                if ui::radio_value(ui, &mut self.settings.theme, index, name).changed() {
                                    self.apply_theme(ctx);
                                }
                            }
                        }
                        1 => {
                            ui::section(ui, "Police de l'éditeur");
                            ui.label("JetBrains Mono");
                            if ui
                                .add(
                                    egui::Slider::new(&mut self.settings.font_size, 8.0..=36.0)
                                        .max_decimals(0)
                                        .text("Taille"),
                                )
                                .changed()
                            {
                                self.apply_theme(ctx);
                            }
                            let mut wheel_zoom = !self.settings.wheel_zoom_off;
                            if ui::checkbox(ui, &mut wheel_zoom, "Changer la taille de la police avec Ctrl + molette").changed() {
                                self.settings.wheel_zoom_off = !wheel_zoom;
                            }
                            ui.add_space(10.0);
                            ui::section(ui, "Outils d'exécution");
                            ui.label("Les commandes cargo, python, java, node et tsx doivent être installées et dans le PATH.");
                        }
                        2 => {
                            egui::Grid::new("shortcut_list").num_columns(2).spacing([24.0, 8.0]).show(ui, |ui| {
                                for (key, action) in SHORTCUTS {
                                    ui.label(action);
                                    ui.label(RichText::new(key).color(t.text_secondary));
                                    ui.end_row();
                                }
                            });
                        }
                        _ => self.plugins_page(ui, t, &mut command),
                    });
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close |= ui::primary_button(ui, "Fermer").clicked();
            });
        });
        if close || modal.should_close() {
            self.show_settings = false;
        }
        if let Some(cmd) = command {
            self.show_settings = false;
            self.execute(&cmd);
        }
    }

    fn plugin_key(&self, id: &str) -> String {
        format!("{}::{id}", self.root.display())
    }

    /// The files used, most recent first: the open ones and the ones closed since.
    fn recent_hits(&self, query: &str) -> Vec<Hit> {
        self.recent_files
            .iter()
            .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().contains(query)))
            .map(|path| Hit {
                label: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                hint: path
                    .parent()
                    .map(|p| p.strip_prefix(&self.root).unwrap_or(p).display().to_string())
                    .unwrap_or_default(),
                icon: None,
                file: Some(path.clone()),
                target: Target::File(path.clone()),
            })
            .collect()
    }

    /// The window is named after the project and the open file, as IntelliJ names it: that is
    /// what the taskbar and the window switcher show.
    fn update_window_title(&mut self, ctx: &egui::Context) {
        let project = self.root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let title = match self.documents.get(self.active) {
            Some(doc) if !project.is_empty() => format!("{project} – {}", doc.title()),
            _ if !project.is_empty() => project,
            _ => "Atelier IDE".to_string(),
        };
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    /// Keeps `recent_files` in the order the files were last in front.
    fn track_recent_files(&mut self) {
        let Some(path) = self.documents.get(self.active).and_then(|d| d.path.clone()) else {
            return;
        };
        if self.recent_files.first() != Some(&path) {
            self.recent_files.retain(|p| p != &path);
            self.recent_files.insert(0, path);
            self.recent_files.truncate(50);
        }
    }

    fn palette_hits(&mut self) -> Vec<Hit> {
        let query = self.palette_query.to_lowercase();
        match self.palette_mode {
            PaletteMode::Recent => return self.recent_hits(&query),
            PaletteMode::Line => return line_hit(&query).into_iter().collect(),
            PaletteMode::Everywhere => {}
        }
        let mut hits = Vec::new();
        for (i, doc) in self.documents.iter().enumerate() {
            if doc.title().to_lowercase().contains(&query) {
                hits.push(Hit {
                    label: doc.title(),
                    hint: "Onglet ouvert".into(),
                    icon: None,
                    file: Some(doc.path.clone().unwrap_or_default()),
                    target: Target::Tab(i),
                });
            }
        }
        for (i, (label, shortcut, icon)) in ACTIONS.iter().enumerate() {
            if label.to_lowercase().contains(&query) {
                hits.push(Hit {
                    label: label.to_string(),
                    hint: shortcut.to_string(),
                    icon: Some(*icon),
                    file: None,
                    target: Target::Action(i),
                });
            }
        }
        for plugin in &self.builtins {
            if !self.builtin_enabled(plugin.id()) {
                continue;
            }
            for command in plugin.commands() {
                if command.label.to_lowercase().contains(&query) {
                    let (label, hint) = command
                        .label
                        .split_once("  ")
                        .unwrap_or((&command.label, ""));
                    hits.push(Hit {
                        label: label.to_string(),
                        hint: hint.to_string(),
                        icon: Some("git"),
                        file: None,
                        target: Target::Builtin(plugin.id(), command.id),
                    });
                }
            }
        }
        if !query.is_empty() {
            let root = self.root.clone();
            let files = self.project_files.get_or_insert_with(|| list_files(&root));
            let mut matches: Vec<_> = files
                .iter()
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().to_lowercase().contains(&query))
                })
                .take(60)
                .cloned()
                .collect();
            matches.sort_by_key(|p| {
                let name = p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase();
                (!name.starts_with(&query), name.len())
            });
            for path in matches {
                let relative = path
                    .parent()
                    .and_then(|p| p.strip_prefix(&self.root).ok())
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                hits.push(Hit {
                    label: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    hint: relative,
                    icon: None,
                    file: Some(path.clone()),
                    target: Target::File(path),
                });
            }
        }
        hits
    }

    /// Search Everywhere: a popup under the title bar, driven by the keyboard.
    fn palette(&mut self, ctx: &egui::Context) {
        if !self.show_palette {
            return;
        }
        let t = self.theme(ctx);
        let hits = self.palette_hits();
        let (down, up, enter, escape) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        if down {
            self.palette_index = (self.palette_index + 1).min(hits.len().saturating_sub(1));
        }
        if up {
            self.palette_index = self.palette_index.saturating_sub(1);
        }
        self.palette_index = self.palette_index.min(hits.len().saturating_sub(1));
        let mut chosen = enter
            .then(|| hits.get(self.palette_index).map(|h| h.target.clone()))
            .flatten();
        let width = 680.0_f32.min(ctx.content_rect().width() - 40.0);
        let area = egui::Area::new(egui::Id::new("search_everywhere"))
            .anchor(Align2::CENTER_TOP, vec2(0.0, 90.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .fill(t.popup)
                    .corner_radius(10)
                    .inner_margin(Margin::same(0))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        egui::Frame::new()
                            .inner_margin(Margin::symmetric(14, 10))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let (title, subtitle) = match self.palette_mode {
                                        PaletteMode::Everywhere => ("Rechercher partout", "Fichiers, actions, onglets"),
                                        PaletteMode::Recent => ("Fichiers récents", "Dans l'ordre d'utilisation"),
                                        PaletteMode::Line => ("Aller à la ligne", "ligne ou ligne:colonne"),
                                    };
                                    ui.label(RichText::new(title).font(ui::semibold(13.0)));
                                    ui.label(RichText::new(subtitle).small().color(t.text_secondary));
                                });
                                ui.add_space(6.0);
                                let field = ui::search_field(
                                    ui,
                                    &mut self.palette_query,
                                    if self.palette_mode == PaletteMode::Line { "Numéro de ligne…" } else { "Tapez pour rechercher…" },
                                    width - 28.0,
                                );
                                if !field.has_focus() {
                                    field.request_focus();
                                }
                                if field.changed() {
                                    self.palette_index = 0;
                                }
                            });
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .max_height(420.0)
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                if hits.is_empty() {
                                    ui.add_space(12.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(
                                            RichText::new("Aucun résultat").color(t.text_secondary),
                                        );
                                    });
                                    ui.add_space(12.0);
                                }
                                for (i, hit) in hits.iter().enumerate() {
                                    let selected = i == self.palette_index;
                                    let (rect, response) = ui::row(ui, selected);
                                    if selected && (down || up) {
                                        response.scroll_to_me(None);
                                    }
                                    let icon = Rect::from_min_size(
                                        pos2(rect.left() + 16.0, rect.center().y - 8.0),
                                        vec2(16.0, 16.0),
                                    );
                                    match (&hit.file, hit.icon) {
                                        (Some(path), _) => {
                                            egui::Image::new(self.file_icon(ui.ctx(), path)).paint_at(ui, icon)
                                        }
                                        (None, Some(name)) => ui::paint_icon(ui, name, icon),
                                        _ => {}
                                    }
                                    let end = ui::paint_text(
                                        ui,
                                        rect,
                                        icon.right() + 8.0,
                                        &hit.label,
                                        t.text,
                                        FontId::proportional(13.0),
                                    );
                                    ui.painter().with_clip_rect(rect).text(
                                        pos2(rect.right() - 16.0, rect.center().y),
                                        Align2::RIGHT_CENTER,
                                        &hit.hint,
                                        FontId::proportional(12.0),
                                        t.text_secondary,
                                    );
                                    let _ = end;
                                    if response.clicked() {
                                        chosen = Some(hit.target.clone());
                                    }
                                    if response.hovered()
                                        && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO)
                                    {
                                        self.palette_index = i;
                                    }
                                }
                                ui.add_space(4.0);
                            });
                    });
            });
        let clicked_outside = ctx.input(|i| i.pointer.any_pressed())
            && ctx
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|p| !area.response.rect.contains(p));
        if escape || clicked_outside {
            self.show_palette = false;
        }
        if let Some(target) = chosen {
            self.show_palette = false;
            match target {
                Target::Action(index) => self.perform_action(index),
                Target::Builtin(plugin, command) => self.run_builtin_command(plugin, command),
                Target::Tab(index) => self.active = index,
                Target::File(path) => self.open_file(path),
                Target::Line(line, column) => self.go_to_line(line, column),
            }
        }
    }

    // ---- Built-in plugins ---------------------------------------------------------------

    fn set_terminal(&mut self, show: bool) {
        self.show_terminal = show;
        if show {
            self.bottom_tool = None;
            self.focus = Some("terminal");
        }
    }

    /// Icon of a file: the one a plugin gives to its own files, else the standard one.
    fn file_icon(&self, ctx: &egui::Context, path: &Path) -> egui::ImageSource<'static> {
        let dark = self.theme(ctx).dark;
        self.builtins
            .iter()
            .filter(|p| self.builtin_enabled(p.id()))
            .find_map(|p| p.file_icon_for(path))
            .and_then(|key| crate::icon_set::source(key, dark))
            .unwrap_or_else(|| icons::file(path, dark))
    }

    fn builtin_enabled(&self, id: &str) -> bool {
        // The IDE's own tool windows cannot be turned off.
        self.builtins.iter().any(|p| p.id() == id && p.core()) || !self.settings.disabled_builtins.contains(id)
    }

    fn set_builtin_enabled(&mut self, id: &str, enabled: bool) {
        if enabled {
            self.settings.disabled_builtins.remove(id);
            return;
        }
        self.settings.disabled_builtins.insert(id.to_string());
        let owned = |tool: Option<&ToolWindow>| {
            tool.is_some_and(|t| {
                self.builtins
                    .iter()
                    .any(|p| p.id() == id && p.tool_windows().iter().any(|w| w.id == t.id))
            })
        };
        if owned(self.left_tool) {
            self.left_tool = None;
        }
        if owned(self.bottom_tool) {
            self.bottom_tool = None;
        }
    }

    /// Calls `f` on each enabled built-in plugin, with the IDE as its host. The plugins are
    /// taken out of `self` meanwhile, so `f` must not expect to find them there.
    fn each_builtin(&mut self, mut f: impl FnMut(&mut dyn BuiltinPlugin, &mut Ide)) {
        let mut builtins = std::mem::take(&mut self.builtins);
        for plugin in &mut builtins {
            if self.builtin_enabled(plugin.id()) {
                f(plugin.as_mut(), self);
            }
        }
        self.builtins = builtins;
    }

    fn tool_windows(&self) -> Vec<&'static ToolWindow> {
        self.builtins
            .iter()
            .filter(|p| self.builtin_enabled(p.id()))
            .flat_map(|p| p.tool_windows())
            .collect()
    }

    fn tool_open(&self, tool: &ToolWindow) -> bool {
        let open = match tool.dock {
            Dock::Left => self.left_tool,
            Dock::Bottom => self.bottom_tool,
            Dock::Right => None,
        };
        open.is_some_and(|t| t.id == tool.id)
    }

    fn toggle_tool(&mut self, tool: &'static ToolWindow) {
        if self.tool_visible(tool) {
            match tool.id {
                "project" => self.show_project = false,
                "terminal" => self.show_terminal = false,
                "notifications" => self.show_notifications = false,
                _ if tool.dock == Dock::Left => self.left_tool = None,
                _ => self.bottom_tool = None,
            }
        } else {
            self.open_tool(tool);
        }
    }

    fn open_tool(&mut self, tool: &'static ToolWindow) {
        match (tool.id, tool.dock) {
            ("project", _) => {
                self.show_project = true;
                self.left_tool = None;
            }
            ("terminal", _) => self.set_terminal(true),
            ("notifications", _) => self.show_notifications = true,
            (_, Dock::Left) => self.left_tool = Some(tool),
            (_, Dock::Bottom) => {
                self.bottom_tool = Some(tool);
                self.show_terminal = false;
            }
            (_, Dock::Right) => {}
        }
        self.focus = Some(tool.id);
    }

    /// Change markers, annotations and menu entries of the active document's gutter.
    fn gutter(&mut self) -> Gutter {
        let mut gutter = Gutter::default();
        let Some(path) = self
            .documents
            .get(self.active)
            .filter(|d| d.loading.is_none() && !d.binary)
            .and_then(|d| d.path.clone())
        else {
            return gutter;
        };
        for plugin in &mut self.builtins {
            if self.settings.disabled_builtins.contains(plugin.id()) {
                continue;
            }
            let text = &self.documents[self.active].text;
            gutter.marks.extend(plugin.line_marks(&path, text));
            if gutter.annotations.is_none() {
                gutter.annotations = plugin.annotations(&path);
            }
            let id = plugin.id();
            gutter
                .commands
                .extend(plugin.gutter_commands(&path).into_iter().map(|c| (id, c)));
        }
        gutter
    }

    fn run_builtin_command(&mut self, plugin: &str, command: &str) {
        self.each_builtin(|p, ide| {
            if p.id() == plugin {
                p.run_command(command, ide);
            }
        });
    }

    fn dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        for path in dropped {
            if path.is_dir() {
                self.open_project(path);
            } else {
                self.open_file(path);
            }
        }
    }
}

/// Dialog opened from the file tree.
enum TreeDialog {
    NewFolder(String),
    Rename { path: PathBuf, name: String },
    Delete(Vec<PathBuf>),
}

/// Action chosen in the menu of an editor tab.
enum TabAction {
    CloseOthers(usize),
    CloseRight(usize),
    CloseAll,
    CopyPath(usize),
    Reveal(usize),
    Explorer(usize),
}

/// What the mouse is dragging in the terminal.
#[derive(Clone, Copy, PartialEq)]
enum Drag {
    /// A tab, by index, onto the panes (split) or along the tab bar (reorder).
    Tab(usize),
    /// A pane, by id, onto another pane (move) or onto the tab bar (own tab).
    Pane(u64),
}

/// A shell shown in a terminal tab, and the text selected in it.
struct Pane {
    id: u64,
    term: Terminal,
    selection: crate::terminal_ui::Selection,
}

/// A terminal tab: shells split side by side or stacked, nested as in IntelliJ.
struct TerminalTab {
    root: Node<Pane>,
    /// Id of the pane that has the keyboard.
    active: u64,
}

impl TerminalTab {
    fn pane(&mut self) -> Option<&mut Pane> {
        let active = self.active;
        self.root.leaves_mut().into_iter().find(|p| p.id == active)
    }
    fn name(&self) -> &str {
        self.root
            .leaves()
            .first()
            .map_or("", |p| p.term.shell.name.as_str())
    }
    fn count(&self) -> usize {
        self.root.leaves().len()
    }
}

/// Where a tab dropped on `rect` goes: (stacked, before, highlighted area). The outer parts
/// of the pane split it on that side, the middle splits it to the right.
fn drop_zone(rect: Rect, pointer: egui::Pos2) -> (bool, bool, Rect) {
    let x = (pointer.x - rect.left()) / rect.width().max(1.0);
    let y = (pointer.y - rect.top()) / rect.height().max(1.0);
    let (left, top) = (rect.min, rect.min);
    let half_w = vec2(rect.width() / 2.0, rect.height());
    let half_h = vec2(rect.width(), rect.height() / 2.0);
    if x < 0.3 {
        (false, true, Rect::from_min_size(left, half_w))
    } else if x > 0.7 {
        (false, false, Rect::from_min_size(pos2(rect.center().x, rect.top()), half_w))
    } else if y < 0.5 {
        (true, true, Rect::from_min_size(top, half_h))
    } else {
        (true, false, Rect::from_min_size(pos2(rect.left(), rect.center().y), half_h))
    }
}

#[derive(Default)]
struct Gutter {
    marks: Vec<builtin::LineMark>,
    annotations: Option<String>,
    commands: Vec<(&'static str, builtin::Command)>,
}

impl Host for Ide {
    fn root(&self) -> &Path {
        &self.root
    }
    fn active_file(&self) -> Option<PathBuf> {
        self.documents.get(self.active).and_then(|d| d.path.clone())
    }
    fn open_file(&mut self, path: PathBuf) {
        Ide::open_file(self, path);
    }
    fn selected_text(&self) -> Option<String> {
        Ide::selected_text(self)
    }
    fn run_in_new_terminal(&mut self, command: String) {
        self.set_terminal(true);
        self.start_terminal(None);
        self.execute(&command);
    }
    fn open_documents(&self) -> Vec<builtin::OpenDocument> {
        self.documents
            .iter()
            .enumerate()
            .map(|(index, doc)| builtin::OpenDocument {
                path: doc.path.clone(),
                title: doc.title(),
                dirty: doc.dirty(),
                active: index == self.active,
            })
            .collect()
    }
    fn editor_state(&self) -> Option<builtin::EditorSnapshot> {
        let doc = self.documents.get(self.active)?;
        let (line, column) = doc.view.position(&doc.text);
        Some(builtin::EditorSnapshot {
            path: doc.path.clone(),
            language: doc.language.clone(),
            line,
            column,
            selection: Ide::selected_text(self),
        })
    }
    fn problems(&self) -> Vec<builtin::Problem> {
        let mut out = Vec::new();
        for (index, doc) in self.documents.iter().enumerate() {
            let Some(path) = &doc.path else { continue };
            if !self.languages.handles(&doc.language) {
                continue;
            }
            let reference = crate::languages::DocRef {
                key: doc_key(&doc.path, index),
                path: doc.path.clone(),
                language: doc.language.clone(),
            };
            for d in self.languages.diagnostics(&reference).iter() {
                let (line, _) = crate::languages::lsp::to_position(&doc.text, d.range.start);
                let start = doc.text[..d.range.start.min(doc.text.len())].rfind('\n').map_or(0, |i| i + 1);
                let end = d.range.end.min(doc.text.len()).max(d.range.start.min(doc.text.len()));
                out.push(builtin::Problem {
                    path: path.clone(),
                    line,
                    column: doc.text[start..d.range.start.min(doc.text.len())].chars().count(),
                    length: doc.text[d.range.start.min(doc.text.len())..end].chars().count().max(1),
                    severity: match d.severity {
                        crate::languages::Severity::Error => 1,
                        crate::languages::Severity::Warning => 2,
                        crate::languages::Severity::Info => 3,
                        crate::languages::Severity::Hint => 4,
                    },
                    message: d.message.clone(),
                    source: d.source.clone(),
                });
            }
        }
        out
    }
    fn open_at(&mut self, path: PathBuf, line: usize, column: usize, length: usize) {
        let place = crate::languages::Location {
            path: Some(path),
            line,
            column,
            end_line: line,
            end_column: column + length,
            preview: String::new(),
        };
        let from = self
            .documents
            .get(self.active)
            .and_then(|d| Some((d.path.clone()?, d.view.cursor)));
        match from {
            Some((path, offset)) => self.go_to(place, Some(path), offset),
            None => self.go_to(place, None, 0),
        }
    }
    fn open_project(&mut self, path: PathBuf) {
        Ide::open_project(self, path);
    }
    fn save_files(&mut self) -> bool {
        for index in 0..self.documents.len() {
            let doc = &self.documents[index];
            if doc.dirty() && doc.path.is_some() && !self.save_document(index, false) {
                return false;
            }
        }
        true
    }
    fn reload_files(&mut self) {
        let mut errors = Vec::new();
        for doc in &mut self.documents {
            if let Err(e) = doc.reload() {
                errors.push(format!("{} : {e:#}", doc.title()));
            }
        }
        if let Some(error) = errors.first() {
            self.status = format!("Erreur : rechargement de {error}");
        }
        self.tree.clear();
        self.project_files = None;
    }
    fn show_tool_window(&mut self, tool: &'static ToolWindow) {
        self.open_tool(tool);
    }
    fn notify(&mut self, message: String) {
        self.status = message;
    }
    fn balloon(&mut self, title: &str, text: String, error: bool) {
        self.push_note(title, text, error, true);
    }
}

impl Ide {
    /// Debug builds only: `ATELIER_SHOT=out.png` saves a screenshot of the window and quits.
    /// `ATELIER_SHOT_CLICK=x,y` clicks there, `ATELIER_SHOT_PAGE=3` opens that page of the settings first, `ATELIER_SHOT_OPEN=file` a file, `ATELIER_SHOT_CURSOR=n` puts the caret at byte n and runs `ATELIER_SHOT_ACTION` (complete by default).
    #[cfg(debug_assertions)]
    fn debug_screenshot(&mut self, ctx: &egui::Context) {
        let Ok(path) = std::env::var("ATELIER_SHOT") else {
            return;
        };
        let pass = ctx.cumulative_pass_nr();
        if pass == 5
            && let Some(page) = std::env::var("ATELIER_SHOT_PAGE").ok().and_then(|p| p.parse().ok())
        {
            self.show_settings = true;
            self.settings_page = page;
        }
        if pass == 5
            && let Some(file) = std::env::var_os("ATELIER_SHOT_OPEN")
        {
            self.open_file(PathBuf::from(file));
        }
        if pass == 6 {
            if std::env::var_os("ATELIER_SHOT_SEARCH").is_some() {
                self.open_tool(&crate::features::search::TOOL_WINDOWS[0]);
            }
            if std::env::var_os("ATELIER_SHOT_MCP").is_some() {
                self.mcp_offer = Some((0, crate::plugins::builtins::mcp::agents::Agent::Claude));
            }
            if std::env::var_os("ATELIER_SHOT_SESSIONS").is_some() {
                self.open_tool(&crate::plugins::builtins::mcp::TOOL_WINDOWS[0]);
            }
            if std::env::var_os("ATELIER_SHOT_TERMINAL").is_some() {
                self.set_terminal(true);
            }
            if std::env::var_os("ATELIER_SHOT_PROBLEMS").is_some() {
                self.open_tool(&crate::features::problems::TOOL_WINDOWS[0]);
            }
            if let Ok(query) = std::env::var("ATELIER_SHOT_FIND") {
                self.find.show(Some(query), std::env::var_os("ATELIER_SHOT_REPLACE").is_some());
            }
        }
        // A language server needs a few seconds before it can answer.
        let slow = std::env::var_os("ATELIER_SHOT_CURSOR").is_some();
        if pass == if slow { 150 } else { 8 }
            && let Some(offset) = std::env::var("ATELIER_SHOT_CURSOR").ok().and_then(|o| o.parse::<usize>().ok())
            && let Some(doc) = self.documents.get_mut(self.active)
        {
            doc.view.select(offset, offset);
            let action = std::env::var("ATELIER_SHOT_ACTION").unwrap_or_default();
            if action == "rename" {
                doc.view.request_rename(&doc.text);
            } else {
                doc.view.debug_action(&action);
            }
        }
        let late = std::env::var_os("ATELIER_SHOT_DRAG").is_some();
        if pass == if slow { 300 } else if late { 60 } else { 20 } {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let _ = image::save_buffer(
                &path,
                image.as_raw(),
                image.width() as u32,
                image.height() as u32,
                image::ColorType::Rgba8,
            );
            self.can_quit = true;
        }
        ctx.request_repaint();
    }
}

impl eframe::App for Ide {
    /// Debug builds only: `ATELIER_SHOT_CLICK=x,y` clicks there (in points) on the 10th frame.
    #[cfg(debug_assertions)]
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        // `ATELIER_SHOT_DRAG=x1,y1,x2,y2` drags from the first point to the second.
        if let Some(v) = std::env::var("ATELIER_SHOT_DRAG").ok().and_then(|v| {
            let n: Vec<f32> = v.split(',').filter_map(|p| p.parse().ok()).collect();
            (n.len() == 4).then_some(n)
        }) {
            let (a, b) = (egui::pos2(v[0], v[1]), egui::pos2(v[2], v[3]));
            raw_input.focused = true;
            let pass = ctx.cumulative_pass_nr();
            let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
            match pass {
                6..=9 => raw_input.events.push(egui::Event::PointerMoved(a)),
                10 => raw_input.events.push(button(a, true)),
                11..=14 => {
                    let f = (pass - 10) as f32 / 4.0;
                    raw_input.events.push(egui::Event::PointerMoved(a + (b - a) * f));
                }
                15..=22 => raw_input.events.push(egui::Event::PointerMoved(b)),
                23 => raw_input.events.push(button(b, false)),
                _ => {}
            }
            return;
        }
        let Some((x, y)) = std::env::var("ATELIER_SHOT_CLICK").ok().and_then(|v| {
            let (x, y) = v.split_once(',')?;
            Some((x.parse::<f32>().ok()?, y.parse::<f32>().ok()?))
        }) else {
            return;
        };
        let pos = egui::pos2(x, y);
        raw_input.focused = true;
        let pass = ctx.cumulative_pass_nr();
        if (6..=9).contains(&pass) {
            raw_input.events.push(egui::Event::PointerMoved(pos));
        }
        if pass == 10 || pass == 12 {
            raw_input.events.push(egui::Event::PointerButton {
                pos,
                button: if std::env::var_os("ATELIER_SHOT_RIGHT").is_some() {
                    egui::PointerButton::Secondary
                } else {
                    egui::PointerButton::Primary
                },
                pressed: pass == 10,
                modifiers: egui::Modifiers::NONE,
            });
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(debug_assertions)]
        self.debug_screenshot(ctx);
        self.apply_pending_nav();
        self.track_recent_files();
        self.update_window_title(ctx);
        self.serve_command_line(ctx);
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
        self.each_builtin(|plugin, ide| plugin.update(ctx, ide));
        if ctx.input(|i| i.viewport().close_requested()) && !self.can_quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request(Pending::Quit);
        }
        if self.can_quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.pending.is_none()
            && self.project_choice.is_none()
            && !self.show_palette
            && !self.show_settings
            && !self
                .terminals
                .get(self.active_terminal)
                .is_some_and(|tab| tab.root.leaves().iter().any(|p| p.term.focused) && self.show_terminal)
        {
            self.shortcuts(ctx);
        }
        for tab in &mut self.terminals {
            for pane in tab.root.leaves_mut() {
                if pane.term.poll() {
                    ctx.request_repaint();
                }
            }
            // A shell that exits inside a split goes away, as in IntelliJ.
            while tab.count() > 1 {
                let Some(id) = tab.root.leaves().iter().find(|p| p.term.exited).map(|p| p.id) else {
                    break;
                };
                tab.root.remove(&|p: &Pane| p.id == id);
                if let Some(first) = tab.root.leaves_mut().into_iter().next() {
                    if tab.active == id {
                        first.term.want_focus = true;
                        tab.active = first.id;
                    }
                }
            }
        }
        if !self.terminals.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        self.poll_loading(ctx);
        if let Some(text) = self.pending_copy.take() {
            ctx.copy_text(text);
        }
        self.watch_files(ctx);
        self.track_focus(ctx);
        self.dropped_files(ctx);
        Self::resize_edges(ctx);
        self.toolbar(ctx);
        self.status_bar(ctx);
        self.stripe(ctx, Dock::Left);
        self.stripe(ctx, Dock::Right);
        self.left_panel(ctx);
        self.right_panel(ctx);
        self.bottom_panel(ctx);
        self.editor(ctx);
        self.dialogs(ctx);
        self.log_status();
        self.balloons(ctx);
    }
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        ui::theme_by_index(self.settings.theme)
            .frame
            .to_normalized_gamma_f32()
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "settings", &self.settings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ide() -> Ide {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .unwrap();
        Ide::blank(
            root,
            Settings {
                font_size: 14.0,
                ..Default::default()
            },
            None,
        )
    }

    fn frame_with(ctx: &egui::Context, ide: &mut Ide, time: f64, events: Vec<egui::Event>) {
        frame_mod(ctx, ide, time, events, Default::default());
    }

    fn frame_mod(
        ctx: &egui::Context,
        ide: &mut Ide,
        time: f64,
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) {
        let _ = ctx.run(
            egui::RawInput {
                modifiers,
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1440.0, 920.0))),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ctx| {
                ide.track_focus(ctx);
                ide.left_panel(ctx);
                ide.editor(ctx);
                ide.dialogs(ctx);
            },
        );
    }

    #[test]
    fn ctrl_and_shift_select_several_entries_in_the_tree() {
        use egui::{Event, Modifiers, PointerButton};
        let base = std::env::temp_dir().join(format!("atelier-multi-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("dest")).unwrap();
        for name in ["a.txt", "b.txt", "c.txt", "d.txt"] {
            fs::write(base.join(name), "x").unwrap();
        }
        let base = base.canonicalize().unwrap();
        let ctx = egui::Context::default();
        ui::install(&ctx, ui::theme_by_index(0), 14.0);
        let mut ide = Ide::blank(base.clone(), Settings { font_size: 14.0, ..Default::default() }, None);
        ide.show_project = true;
        let mut time = 0.0;
        let mut step = |ide: &mut Ide, events: Vec<Event>, modifiers: Modifiers| {
            time += 0.05;
            frame_mod(&ctx, ide, time, events, modifiers);
        };
        step(&mut ide, vec![], Modifiers::NONE);
        step(&mut ide, vec![], Modifiers::NONE);
        let click = |ide: &mut Ide, name: &str, modifiers: Modifiers, step: &mut dyn FnMut(&mut Ide, Vec<Event>, Modifiers)| {
            let at = ide.row_rects.iter().rev().find(|(p, _)| p.ends_with(name)).unwrap().1.center();
            let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers };
            step(ide, vec![Event::PointerMoved(at)], modifiers);
            step(ide, vec![button(true)], modifiers);
            step(ide, vec![button(false)], modifiers);
            step(ide, vec![], modifiers);
        };
        click(&mut ide, "a.txt", Modifiers::NONE, &mut step);
        click(&mut ide, "c.txt", Modifiers::CTRL, &mut step);
        click(&mut ide, "d.txt", Modifiers::CTRL, &mut step);
        let want: BTreeSet<_> = ["a.txt", "c.txt", "d.txt"].iter().map(|n| base.join(n)).collect();
        assert_eq!(ide.tree_multi, want);
        // Ctrl on a selected row takes it out of the selection.
        click(&mut ide, "c.txt", Modifiers::CTRL, &mut step);
        assert!(!ide.tree_multi.contains(&base.join("c.txt")) && ide.tree_multi.len() == 2);
        // Shift selects the rows between the anchor and the click.
        click(&mut ide, "b.txt", Modifiers::NONE, &mut step);
        assert!(ide.tree_multi.is_empty());
        click(&mut ide, "d.txt", Modifiers::SHIFT, &mut step);
        let want: BTreeSet<_> = ["b.txt", "c.txt", "d.txt"].iter().map(|n| base.join(n)).collect();
        assert_eq!(ide.tree_multi, want);
        assert_eq!(ide.tree_selection().len(), 3);
        // Several documents are not opened by these clicks.
        assert!(ide.documents.iter().all(|d| d.path.as_deref() != Some(&base.join("c.txt"))));
        // Dragging one of them onto the folder moves them all.
        let from = ide.row_rects.iter().rev().find(|(p, _)| p.ends_with("c.txt")).unwrap().1.center();
        let to = ide.row_rects.iter().rev().find(|(p, _)| p.ends_with("dest")).unwrap().1.center();
        let button = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        step(&mut ide, vec![Event::PointerMoved(from)], Modifiers::NONE);
        step(&mut ide, vec![button(from, true)], Modifiers::NONE);
        for i in 1..=6 {
            let pos = from + (to - from) * (i as f32 / 6.0);
            step(&mut ide, vec![Event::PointerMoved(pos)], Modifiers::NONE);
        }
        step(&mut ide, vec![button(to, false)], Modifiers::NONE);
        step(&mut ide, vec![], Modifiers::NONE);
        for name in ["b.txt", "c.txt", "d.txt"] {
            assert!(base.join("dest").join(name).exists(), "{name} moved");
            assert!(!base.join(name).exists());
        }
        assert!(base.join("a.txt").exists());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn the_tree_follows_changes_made_outside() {
        let base = std::env::temp_dir().join(format!("atelier-refresh-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("sub")).unwrap();
        fs::write(base.join("a.txt"), "x").unwrap();
        let base = base.canonicalize().unwrap();
        let ctx = egui::Context::default();
        ui::install(&ctx, ui::theme_by_index(0), 14.0);
        let mut ide = Ide::blank(base.clone(), Settings { font_size: 14.0, ..Default::default() }, None);
        ide.show_project = true;
        ide.expanded.insert(base.join("sub"));
        let mut time = 0.0;
        let mut step = |ide: &mut Ide| {
            time += 0.05;
            frame_with(&ctx, ide, time, vec![]);
        };
        step(&mut ide);
        step(&mut ide);
        let names = |ide: &Ide, dir: &Path| -> Vec<String> {
            ide.tree[dir].iter().map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned()).collect()
        };
        assert_eq!(names(&ide, &base), ["sub", "a.txt"]);
        std::thread::sleep(Duration::from_millis(30));
        fs::write(base.join("new.txt"), "x").unwrap();
        fs::write(base.join("sub/inner.txt"), "x").unwrap();
        fs::remove_file(base.join("a.txt")).unwrap();
        ide.last_watch = Instant::now() - Duration::from_secs(2);
        ide.watch_files(&ctx);
        step(&mut ide);
        step(&mut ide);
        assert_eq!(names(&ide, &base), ["sub", "new.txt"]);
        assert_eq!(names(&ide, &base.join("sub")), ["inner.txt"]);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn arrow_keys_move_the_selection_in_the_tree() {
        use egui::{Event, PointerButton};
        let base = std::env::temp_dir().join(format!("atelier-keys-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("dest")).unwrap();
        fs::write(base.join("a.txt"), "x").unwrap();
        fs::write(base.join("b.txt"), "x").unwrap();
        let base = base.canonicalize().unwrap();
        let ctx = egui::Context::default();
        ui::install(&ctx, ui::theme_by_index(0), 14.0);
        let mut ide = Ide::blank(base.clone(), Settings { font_size: 14.0, ..Default::default() }, None);
        ide.show_project = true;
        let mut time = 0.0;
        let mut step = |ide: &mut Ide, events: Vec<Event>| {
            time += 0.05;
            frame_with(&ctx, ide, time, events);
        };
        step(&mut ide, vec![]);
        step(&mut ide, vec![]);
        let at = ide.row_rects.iter().rev().find(|(p, _)| p.ends_with("dest")).unwrap().1.center();
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        step(&mut ide, vec![Event::PointerMoved(at)]);
        step(&mut ide, vec![button(true)]);
        step(&mut ide, vec![button(false)]);
        step(&mut ide, vec![]);
        assert_eq!(ide.tree_selected, Some(base.join("dest")));
        let key = |key| Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        };
        step(&mut ide, vec![key(egui::Key::ArrowDown)]);
        assert_eq!(ide.tree_selected, Some(base.join("a.txt")), "focus: {:?}", ide.focus);
        step(&mut ide, vec![key(egui::Key::ArrowDown)]);
        assert_eq!(ide.tree_selected, Some(base.join("b.txt")));
        step(&mut ide, vec![key(egui::Key::ArrowUp)]);
        assert_eq!(ide.tree_selected, Some(base.join("a.txt")));
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn dragging_a_file_onto_a_folder_moves_it() {
        use egui::{Event, PointerButton};
        let base = std::env::temp_dir().join(format!("atelier-dnd-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("dest")).unwrap();
        fs::write(base.join("a.txt"), "x").unwrap();
        let base = base.canonicalize().unwrap();
        let ctx = egui::Context::default();
        ui::install(&ctx, ui::theme_by_index(0), 14.0);
        let mut ide = Ide::blank(base.clone(), Settings { font_size: 14.0, ..Default::default() }, None);
        ide.show_project = true;
        let mut time = 0.0;
        let mut step = |ide: &mut Ide, events: Vec<Event>| {
            time += 0.05;
            frame_with(&ctx, ide, time, events);
        };
        step(&mut ide, vec![]);
        step(&mut ide, vec![]);
        let center = |ide: &Ide, name: &str| {
            ide.row_rects.iter().rev().find(|(p, _)| p.file_name().is_some_and(|n| n == name)).unwrap().1.center()
        };
        let (from, to) = (center(&ide, "a.txt"), center(&ide, "dest"));
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        step(&mut ide, vec![Event::PointerMoved(from)]);
        step(&mut ide, vec![button(from, true)]);
        for i in 1..=8 {
            let at = from + (to - from) * (i as f32 / 8.0);
            step(&mut ide, vec![Event::PointerMoved(at)]);
        }
        assert!(ide.tree_drag.is_some(), "the drag started");
        assert_eq!(ide.tree_drop.as_deref(), Some(base.join("dest").as_path()), "the folder is the target");
        step(&mut ide, vec![button(to, false)]);
        step(&mut ide, vec![]);
        assert!(base.join("dest/a.txt").exists(), "{}", ide.status);
        assert!(!base.join("a.txt").exists());
        fs::remove_dir_all(&base).unwrap();
    }

    fn frame(ctx: &egui::Context, ide: &mut Ide) {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1440.0, 920.0))),
                ..Default::default()
            },
            |ctx| {
                ide.each_builtin(|plugin, ide| plugin.update(ctx, ide));
                ide.toolbar(ctx);
                ide.status_bar(ctx);
                ide.stripe(ctx, Dock::Left);
                ide.stripe(ctx, Dock::Right);
                ide.left_panel(ctx);
                ide.right_panel(ctx);
                ide.bottom_panel(ctx);
                ide.editor(ctx);
                ide.dialogs(ctx);
                ide.log_status();
                ide.balloons(ctx);
            },
        );
        assert!(
            !ctx.tessellate(output.shapes, output.pixels_per_point)
                .is_empty()
        );
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
    fn opening_another_project_asks_for_the_window() {
        let mut ide = ide();
        let demo = ide.root.join("examples/demo");
        ide.open_project(demo.clone());
        assert_eq!(ide.project_choice, Some(demo.canonicalize().unwrap()));
        let original = ide.root.clone();
        ide.open_project(original);
        assert_eq!(
            ide.project_choice.as_ref().map(|p| p.ends_with("demo")),
            Some(true)
        );
    }

    #[test]
    fn dev_reload_keeps_project_tabs_and_unsaved_text() {
        let mut ide = ide();
        ide.open_file(ide.root.join("examples/demo/hello.py"));
        ide.open_file(ide.root.join("examples/demo/app.js"));
        ide.wait_loaded();
        ide.documents[0].text.push_str("# pas encore enregistré\n");
        ide.documents.push(Document::untitled());
        ide.documents[2].text = "brouillon".into();
        ide.active = 1;
        ide.show_terminal = true;
        let path = std::env::temp_dir().join(format!("atelier-session-{}.json", std::process::id()));
        ide.save_session(&path).unwrap();

        let mut restored = super::tests::ide();
        restored.root = std::env::temp_dir();
        restored.restore_session(&path);
        assert_eq!(restored.root, ide.root);
        assert_eq!(restored.documents.len(), 3);
        assert!(restored.documents[0].dirty());
        assert!(
            restored.documents[0]
                .text
                .ends_with("# pas encore enregistré\n")
        );
        assert!(!restored.documents[1].dirty());
        assert_eq!(restored.documents[2].text, "brouillon");
        assert_eq!(restored.active, 1);
        assert!(restored.show_terminal);
        assert!(!path.exists(), "the session file is consumed on restore");

        // Built-in tool windows survive the reload too.
        let commit = restored.tool_windows()[0];
        restored.open_tool(commit);
        restored.save_session(&path).unwrap();
        let mut again = super::tests::ide();
        again.restore_session(&path);
        assert!(again.tool_visible(commit));
    }

    #[test]
    fn dropped_tabs_split_the_pane_on_the_side_they_land() {
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 100.0));
        assert_eq!(drop_zone(r, pos2(10.0, 50.0)).0, false);
        assert!(drop_zone(r, pos2(10.0, 50.0)).1, "left of the pane");
        assert!(!drop_zone(r, pos2(90.0, 50.0)).1, "right of the pane");
        let (stacked, before, zone) = drop_zone(r, pos2(50.0, 20.0));
        assert!(stacked && before, "top half");
        assert_eq!(zone.height(), 50.0);
        assert!(!drop_zone(r, pos2(50.0, 80.0)).1, "bottom half");
    }

    #[test]
    fn opening_the_same_file_reuses_its_tab() {
        let mut ide = ide();
        let path = ide.root.join("examples/demo/src/main.rs");
        ide.open_file(path.clone());
        ide.open_file(path);
        ide.wait_loaded();
        assert_eq!(ide.documents.len(), 1);
        assert_eq!(ide.documents[0].language, "rs");
        ide.request(Pending::Close(0));
        assert!(ide.documents.is_empty());
        assert_eq!(ide.active, 0);
    }

    #[test]
    fn errors_become_balloons_and_notes() {
        let mut ide = ide();
        ide.status = "Erreur : disque plein".into();
        ide.log_status();
        assert_eq!(ide.notes.len(), 1);
        assert!(ide.notes[0].error);
        assert_eq!(ide.balloons, [0]);
        ide.status = "Enregistré".into();
        ide.log_status();
        assert_eq!(ide.notes.len(), 2);
        assert_eq!(
            ide.balloons,
            [0],
            "plain status messages only go to the log"
        );
    }

    #[test]
    fn search_everywhere_finds_project_files() {
        let mut ide = ide();
        ide.open_palette();
        ide.palette_query = "cargo.tom".into();
        let hits = ide.palette_hits();
        assert!(
            hits.iter()
                .any(|h| matches!(&h.target, Target::File(p) if p.ends_with("Cargo.toml")))
        );
    }

    #[test]
    fn initials_follow_intellij() {
        assert_eq!(initials("integration-rfe"), "IR");
        assert_eq!(initials("rust"), "RU");
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
        ide.wait_loaded();
        }
        assert_eq!(ide.documents.len(), 7);
        ide.show_terminal = false;
        ide.show_notifications = true;
        for theme in 0..ui::THEMES.len() {
            ide.settings.theme = theme;
            ide.apply_theme(&ctx);
            for active in 0..7 {
                ide.active = active;
                frame(&ctx, &mut ide);
            }
        }
        ide.show_settings = true;
        for page in 0..4 {
            ide.settings_page = page;
            frame(&ctx, &mut ide);
        }
        ide.show_settings = false;
        ide.open_palette();
        ide.palette_query = "main".into();
        frame(&ctx, &mut ide);
    }

    /// Drives the frames of a real session on this repository with the Git tool windows open.
    #[test]
    fn git_plugin_windows_render_on_this_repository() {
        let mut ide = ide();
        ide.open_file(ide.root.join("src/main.rs"));
        ide.wait_loaded();
        let ctx = egui::Context::default();
        ide.apply_theme(&ctx);
        let tools = ide.tool_windows();
        assert_eq!(tools.len(), 6, "Commit, Git, Kubernetes, Search, Problems and AI sessions tool windows");
        for tool in tools {
            ide.open_tool(tool);
        }
        frame(&ctx, &mut ide);
        ide.run_builtin_command("git", "git.annotate");
        // Status, log and blame load in the background.
        for _ in 0..100 {
            frame(&ctx, &mut ide);
            if ide.gutter().annotations.is_some_and(|a| a != "Chargement…") {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let annotations = ide.gutter().annotations.unwrap();
        assert_eq!(
            annotations.lines().count(),
            ide.documents[0].text.lines().count(),
            "one blame annotation per line"
        );
        for command in [
            "git.file_diff",
            "git.push",
            "git.branch",
            "git.stash",
            "git.remotes",
        ] {
            ide.run_builtin_command("git", command);
            frame(&ctx, &mut ide);
        }
    }
}

#[cfg(test)]
mod perf_probe {
    use super::*;
    #[test]
    #[ignore]
    fn probe_big_file() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).canonicalize().unwrap();
        let mut ide = Ide::blank(root, Settings { font_size: 14.0, ..Default::default() }, None);
        let ctx = egui::Context::default();
        ide.apply_theme(&ctx);
        let lines: usize = std::env::var("LINES").ok().and_then(|v| v.parse().ok()).unwrap_or(40_000);
        let text: String = (0..lines).map(|i| format!("    let value_{i} = compute(\"chaine {i}\", {i}); // commentaire\n")).collect();
        let path = std::env::temp_dir().join(std::env::var("NAME").unwrap_or("atelier-big.rs".into()));
        fs::write(&path, &text).unwrap();
        println!("taille {} Ko, {} lignes", text.len() / 1024, lines);
        ide.open_file(path);
        ide.wait_loaded();
        for i in 0..4 {
            let t0 = Instant::now();
            let output = ctx.run(egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1440.0, 920.0))), ..Default::default() }, |ctx| {
                let t = Instant::now(); ide.status_bar(ctx); let a = t.elapsed();
                let t = Instant::now(); ide.left_panel(ctx); let b = t.elapsed();
                let t = Instant::now(); ide.editor(ctx); let c = t.elapsed();
                println!("  frame {i}: status {a:?} tree {b:?} editor {c:?}");
            });
            let t = Instant::now();
            let prims = ctx.tessellate(output.shapes, output.pixels_per_point);
            let verts: usize = prims.iter().map(|p| match &p.primitive { egui::epaint::Primitive::Mesh(m) => m.vertices.len(), _ => 0 }).sum();
            println!("  frame {i}: total run {:?}, tessellate {:?}, vertices {verts}", t0.elapsed() - t.elapsed(), t.elapsed());
        }
    }
}
