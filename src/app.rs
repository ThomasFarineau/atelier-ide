use crate::{
    i18n::Text,
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

mod bookmarks;
mod file_index;
mod fonts_page;
mod history;
mod inspect;
mod navigation;
mod plugins_page;
mod portable;
mod projects_page;
mod reformat;
mod run_configs;
mod settings_page;
mod split;
mod symbols;
use crate::editor::FormatRequest;
use settings_page::{Draft, Page, RunTool};

const TITLE_HEIGHT: f32 = 40.0;
/// Size of the window that opens without a project, in points.
const START_SCREEN_SIZE: [f32; 2] = [980.0, 680.0];
const STATUS_HEIGHT: f32 = 28.0;
const STRIPE_WIDTH: f32 = 40.0;
const HEADER_HEIGHT: f32 = 38.0;
const ISLAND_RADIUS: u8 = 10;
const CLOSE_HOVER: Color32 = Color32::from_rgb(0xE8, 0x11, 0x23);

/// The IDE's own tool windows, next to those of the built-in plugins.
static PROJECT_TOOL: ToolWindow = ToolWindow {
    id: "project",
    title: Text("project"),
    icon: "project",
    dock: Dock::Left,
    shortcut: "Alt+1",
};
static TERMINAL_TOOL: ToolWindow = ToolWindow {
    id: "terminal",
    title: Text("terminal"),
    icon: "terminal",
    dock: Dock::Bottom,
    shortcut: "Ctrl+`",
};
static NOTIFICATIONS_TOOL: ToolWindow = ToolWindow {
    id: "notifications",
    title: Text("notifications"),
    icon: "notifications",
    dock: Dock::Right,
    shortcut: "",
};

/// Actions of Search Everywhere and of the main menu: (label, shortcut, icon).
const ACTIONS: [(Text, &str, &str); 21] = [
    (Text("open_project_dialog"), "", "project"),
    (Text("open_file_dialog"), "Ctrl+O", "openNewTab"),
    (Text("save_all"), "Ctrl+S", "save"),
    (Text("terminal"), "Alt+F12", "terminal"),
    (Text("settings_dialog"), "Ctrl+Alt+S", "settings"),
    (Text("plugins_dialog"), "", "settings"),
    (Text("run_current_file"), "Shift+F10", "run"),
    (Text("refresh_project"), "", "refresh"),
    (Text("open_project_new_window"), "", "openNewTab"),
    (Text("new_file_project"), "", "add"),
    (Text("notifications"), "", "notifications"),
    (Text("reformat_code"), "Ctrl+Alt+L", "edit"),
    (Text("optimize_imports"), "Ctrl+Alt+O", "edit"),
    (Text("projects.all"), "", "project"),
    (Text("toggle_bookmark"), "F11", "add"),
    (Text("show_bookmarks"), "Shift+F11", "project"),
    (Text("show_history"), "", "refresh"),
    (Text("split_right"), "", "sideBySide"),
    (Text("split_down"), "", "unified"),
    (Text("close_split"), "", "editorOnly"),
    (Text("inspect_code"), "", "problems"),
];

const SHORTCUTS: [(Text, Text); 64] = [
    (Text("ctrl_o"), Text("open_file")),
    (Text("ctrl_n"), Text("go_symbol")),
    (Text("alt_insert"), Text("new_file")),
    (Text("ctrl_s"), Text("save")),
    (Text("ctrl_shift_s"), Text("save_as")),
    (Text("ctrl_f4"), Text("close_tab")),
    (Text("ctrl_w_ctrl_shift_w"), Text("extend_shrink_selection")),
    (Text("ctrl_shift_j"), Text("join_lines")),
    (Text("f2_shift_f2"), Text("next_previous_problem")),
    (Text("ctrl_shift_u"), Text("toggle_case")),
    (Text("alt_shift_up_down"), Text("move_line")),
    (Text("ctrl_f"), Text("find_file")),
    (Text("double_shift_ctrl_p"), Text("search_everywhere")),
    (Text("ctrl_shift_n"), Text("go_file")),
    (Text("ctrl_alt_s"), Text("settings")),
    (Text("alt_1"), Text("project_window")),
    (Text("alt_f12_ctrl"), Text("terminal")),
    (Text("shift_f10_f5"), Text("run_current_file")),
    (Text("ctrl_f2"), Text("stop_run")),
    (Text("ctrl_space"), Text("code_completion")),
    (Text("shift_f6"), Text("rename")),
    (Text("ctrl_alt_l"), Text("reformat_code")),
    (Text("ctrl_alt_shift_l"), Text("reformat_file")),
    (Text("ctrl_alt_o"), Text("optimize_imports")),
    (
        Text("ctrl_f_ctrl_r_f3"),
        Text("find_replace_next_occurrence"),
    ),
    (Text("ctrl_shift_f"), Text("find_project")),
    (Text("ctrl_click_ctrl_b"), Text("go_declaration_usages")),
    (
        Text("ctrl_shift_b_ctrl_alt"),
        Text("type_declaration_implementations"),
    ),
    (Text("alt_f7"), Text("usages")),
    (Text("ctrl_f12"), Text("file_structure")),
    (Text("alt_enter"), Text("quick_actions_import")),
    (Text("ctrl_shift_m"), Text("matching_bracket_brace")),
    (Text("ctrl_e"), Text("recent_files")),
    (Text("ctrl_g"), Text("go_line")),
    (Text("ctrl_ctrl"), Text("collapse_expand_code")),
    (
        Text("ctrl_shift_ctrl_shift"),
        Text("collapse_all_expand_all"),
    ),
    (Text("ctrl_alt_left"), Text("back_previous_location")),
    (Text("ctrl_alt_right"), Text("forward_next_location")),
    (Text("ctrl_k"), Text("commit")),
    (Text("ctrl_shift_k"), Text("push")),
    (Text("ctrl_t"), Text("update_project")),
    (Text("alt_0"), Text("commit_window")),
    (Text("alt_9"), Text("git_window")),
    (Text("alt_8"), Text("kubernetes_window")),
    (Text("f7_shift_f7"), Text("next_previous_difference")),
    (Text("esc"), Text("give_keyboard_back_ide_terminal")),
    (Text("ctrl_shift_a"), Text("find_action")),
    (Text("ctrl_shift_e"), Text("recent_locations")),
    (Text("ctrl_tab"), Text("switch_file")),
    (Text("f11"), Text("toggle_bookmark")),
    (Text("shift_f11"), Text("show_bookmarks")),
    (Text("ctrl_shift_1_9"), Text("numbered_bookmark")),
    (Text("ctrl_1_9"), Text("go_numbered_bookmark")),
    (Text("alt_j"), Text("add_next_occurrence")),
    (Text("ctrl_alt_shift_j"), Text("select_all_occurrences")),
    (Text("alt_shift_click"), Text("add_remove_caret")),
    (Text("alt_shift_insert"), Text("column_selection")),
    (Text("ctrl_shift_up_down"), Text("move_statement")),
    (Text("ctrl_alt_t"), Text("surround_with")),
    (Text("ctrl_p"), Text("parameter_info")),
    (Text("ctrl_q"), Text("quick_documentation")),
    (Text("ctrl_shift_i"), Text("quick_definition")),
    (Text("ctrl_h_ctrl_alt"), Text("hierarchies")),
    (Text("ctrl_alt_shift_t"), Text("refactor_this")),
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
        label: trf!("line_column", line = line, column = column),
        hint: tr!("enter_go_there").into(),
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

/// The options of the IDE, kept in `settings.json` of the data folder of Atelier
/// (`%LOCALAPPDATA%\Atelier` on Windows, `~/Library/Application Support/Atelier` on macOS,
/// `$XDG_DATA_HOME/Atelier` or `~/.local/share/Atelier` on Linux).
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
    /// What the editor shows besides the code; each is on unless set here.
    inlay_hints_off: bool,
    sticky_lines_off: bool,
    breadcrumbs_off: bool,
    spellcheck_off: bool,
    /// Files that an ignore file (`.gitignore`…) names are hidden from the project tree.
    hide_ignored: bool,
    /// No check of the code before a commit.
    commit_check_off: bool,
    /// A folder that carries the settings from a machine to another (OneDrive, Dropbox…).
    sync_folder: Option<PathBuf>,
    /// The fonts of the interface and of the code.
    fonts: ui::Fonts,
    /// What the user answered when offered to connect an AI to the MCP server of a project,
    /// by `project|ai`.
    mcp_choices: BTreeMap<String, bool>,
    /// When each project was last opened, in seconds since 1970, for the Projects page.
    opened: BTreeMap<PathBuf, u64>,
    /// The language of the interface; the one of the system until the user picks one.
    /// Its identifier (`fr_FR`, `en_US`).
    language: Option<String>,
    /// The last window closed was maximized: the next one opens maximized.
    window_maximized: bool,
}

/// The projects the Projects page keeps; the project widget shows the first ten.
const MAX_RECENT: usize = 50;

fn settings_path() -> PathBuf {
    crate::cli::data_dir().join("settings.json")
}

fn load_settings(path: &Path) -> Option<Settings> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Puts into `file` the options of `mine` that differ from `base`: the other options stay as
/// another window wrote them. Several windows of the IDE share one file, and the last one to
/// write used to put back the old value of what the others had just changed.
fn merge_settings(
    file: &mut serde_json::Value,
    mine: &serde_json::Value,
    base: &serde_json::Value,
) {
    let (Some(file), Some(mine)) = (file.as_object_mut(), mine.as_object()) else {
        return;
    };
    for (key, value) in mine {
        if base.get(key) != Some(value) {
            file.insert(key.clone(), value.clone());
        }
    }
}

/// Writes the settings through a temporary file, so that a crash never leaves half a file.
#[cfg(test)]
fn save_settings(path: &Path, settings: &Settings) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_string_pretty(settings)?)?;
    fs::rename(&temporary, path)?;
    Ok(())
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
    /// A 0-based line of a file: a symbol.
    Place(PathBuf, usize),
    /// A saved version of the active file, to go back to.
    Version(PathBuf),
}

/// What a kept search depends on: the popup mode, what is typed and the list searched.
#[derive(Clone, PartialEq, Eq)]
struct PaletteKey {
    mode: PaletteMode,
    query: String,
    version: (usize, u64),
}

/// What the Search Everywhere popup is asked.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PaletteMode {
    Everywhere,
    Recent,
    Line,
    /// Go to Symbol (Ctrl+N).
    Symbols,
    /// Find Action (Ctrl+Shift+A): the actions and commands only.
    Actions,
    /// Recent Locations (Ctrl+Shift+E).
    Locations,
    /// The bookmarks (Shift+F11).
    Bookmarks,
    /// The saved versions of the active file (Local History).
    History,
}

#[derive(Clone)]
struct Hit {
    label: String,
    hint: String,
    icon: Option<&'static str>,
    file: Option<PathBuf>,
    target: Target,
}

pub struct Ide {
    settings: Settings,
    /// The settings as they were when this window read or wrote them: what changed since is
    /// what this window has to write, the rest belongs to the other windows.
    settings_base: serde_json::Value,
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
    /// The Projects page, open, and its search.
    show_projects: bool,
    projects_filter: String,
    /// Started without a folder: the window shows the Projects page alone, until a project
    /// is opened or created.
    start_screen: bool,
    /// The dialog that creates a project: its name and the folder it goes in.
    new_project: Option<projects_page::NewProject>,
    /// The native window, placed and kept on the taskbar of its monitor.
    window: Option<crate::window::Window>,
    /// The project chosen on the start screen, opened by the next frame.
    pending_start: Option<PathBuf>,
    /// The files of the open documents the last frame, to tell the plugins which are closed.
    open_paths: Vec<PathBuf>,
    settings_page: Page,
    /// What the settings dialog edits, and the settings it started from.
    settings_draft: Draft,
    settings_original: Draft,
    settings_filter: String,
    /// The Fonts page: its search and the fonts being installed.
    fonts_page: fonts_page::State,
    run_tools: Vec<RunTool>,
    /// The dialog opened this frame: the key that opened it is not its own.
    settings_fresh: bool,
    /// The Reformat File dialog, open with its choices, and the choices of the last time.
    reformat_dialog: Option<reformat::Choice>,
    reformat_choice: reformat::Choice,
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
    /// The run configurations, and the project they were read for.
    run_configs: (PathBuf, run_configs::RunConfigs),
    /// The terminal pane each configuration last ran in.
    run_panes: std::collections::HashMap<String, u64>,
    run_dialog: Option<run_configs::Dialog>,
    symbol_index: symbols::Index,
    /// Files in the order they were last used, the most recent first.
    recent_files: Vec<PathBuf>,
    show_new_file: bool,
    find: crate::find::Find,
    /// A find action asked for outside the bar (F3).
    find_request: Option<crate::find::Action>,
    palette_query: String,
    /// The file chosen in the switcher (Ctrl+Tab), while Ctrl is held.
    switcher: Option<usize>,
    /// The second editor beside the first one (Split Right).
    split: Option<split::SplitPane>,
    /// A saved version of the active file, set against the text now (Local History).
    comparison: Option<history::Comparison>,
    /// What Inspect Code found in the project files.
    inspection: Option<inspect::Inspection>,
    /// The lines to come back to, and the document and revision they were last matched to.
    bookmarks: Vec<bookmarks::Bookmark>,
    bookmark_rev: (usize, u64),
    palette_index: usize,
    /// Files of the project for Search Everywhere, listed in the background.
    file_index: file_index::Index,
    /// The last search of the files or symbols, kept while what it depends on is the same.
    palette_cache: Option<(PaletteKey, Vec<Hit>)>,
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
    /// A plugin's tool window on the right (npm, Maven, Gradle), in the place of the notifications.
    right_tool: Option<&'static ToolWindow>,
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
        0..60 => tr!("just_now").into(),
        s @ 60..3600 => trf!("min_ago", s = s / 60),
        s => trf!("h_ago", s = s / 3600),
    }
}

impl Ide {
    /// Writes what this window changed in the settings, keeping what the other windows wrote.
    pub(super) fn persist_settings(&mut self) -> anyhow::Result<()> {
        let mine = serde_json::to_value(&self.settings)?;
        let path = settings_path();
        let mut file = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .filter(serde_json::Value::is_object)
            .unwrap_or_else(|| serde_json::json!({}));
        merge_settings(&mut file, &mine, &self.settings_base);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_string_pretty(&file)?)?;
        fs::rename(&temporary, &path)?;
        self.settings_base = mine;
        Ok(())
    }

    fn blank(root: PathBuf, settings: Settings, reload: Option<Arc<AtomicBool>>) -> Self {
        let (plugins, plugin_errors) = plugins::load(&root);
        let draft = Draft::of(&settings);
        let repaint: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
        Self {
            languages: Arc::new(crate::languages::Service::new(&root, repaint.clone())),
            repaint,
            problems: (0, 0),
            bookmarks: bookmarks::load(&crate::cli::data_dir(), &root),
            bookmark_rev: (usize::MAX, 0),
            nav_back: Vec::new(),
            nav_forward: Vec::new(),
            pending_nav: None,
            settings_base: serde_json::to_value(&settings).unwrap_or_default(),
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
            show_projects: false,
            projects_filter: String::new(),
            start_screen: false,
            new_project: None,
            window: None,
            pending_start: None,
            open_paths: Vec::new(),
            settings_page: Page::Appearance,
            settings_draft: draft.clone(),
            settings_original: draft,
            settings_filter: String::new(),
            fonts_page: Default::default(),
            run_tools: Vec::new(),
            settings_fresh: false,
            reformat_dialog: None,
            reformat_choice: Default::default(),
            plugin_filter: String::new(),
            zoom_hint: None,
            plugin_selected: None,
            show_palette: false,
            window_title: String::new(),
            mcp_offer: None,
            cli: None,
            graph: crate::graph::Service::default(),
            palette_mode: PaletteMode::Everywhere,
            run_configs: Default::default(),
            run_panes: std::collections::HashMap::new(),
            run_dialog: None,
            symbol_index: symbols::Index::default(),
            recent_files: Vec::new(),
            show_new_file: false,
            find: crate::find::Find::default(),
            find_request: None,
            palette_query: String::new(),
            palette_index: 0,
            file_index: file_index::Index::default(),
            palette_cache: None,
            switcher: None,
            split: None,
            comparison: None,
            inspection: None,
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
            status: tr!("ready").into(),
            logged_status: tr!("ready").into(),
            notes: Vec::new(),
            balloons: Vec::new(),
            pending: None,
            project_choice: None,
            can_quit: false,
            cursor: (1, 1),
            reload,
            builtins: builtin::all(),
            left_tool: None,
            right_tool: None,
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
        // The file of the data folder; the first time, what eframe kept before it existed.
        let mut settings: Settings = load_settings(&settings_path())
            .or_else(|| cc.storage.and_then(|s| eframe::get_value(s, "settings")))
            .unwrap_or_default();
        if settings.font_size < 10.0 {
            settings.font_size = 14.0;
        }
        match &settings.language {
            Some(language) => crate::i18n::set(language),
            None => crate::i18n::set(crate::i18n::system()),
        }
        let target = crate::cli::target();
        let reload = crate::dev::listen(&cc.egui_ctx);
        let session = crate::dev::session_path().filter(|p| reload.is_some() && p.exists());
        // Without a folder to open, the window offers to open or create one: the folder it was
        // started from (often the one of the program) is no project.
        let start_screen = target.is_none() && session.is_none();
        let root = if start_screen {
            let dir = crate::cli::data_dir();
            let _ = fs::create_dir_all(&dir);
            dir
        } else {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        };
        let mut ide = Self::blank(root, settings, reload);
        ide.start_screen = start_screen;
        ide.window = crate::window::Window::new(cc);
        if let Some(window) = &mut ide.window {
            let size = if start_screen {
                START_SCREEN_SIZE
            } else {
                crate::window::DEFAULT_SIZE
            };
            window.center(size);
        }
        if ide.settings.window_maximized && !start_screen {
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }
        let ctx = cc.egui_ctx.clone();
        ide.repaint = Arc::new(move || ctx.request_repaint());
        ide.languages = Arc::new(crate::languages::Service::new(
            &ide.root,
            ide.repaint.clone(),
        ));
        egui_extras::install_image_loaders(&cc.egui_ctx);
        ide.sync_in(&cc.egui_ctx);
        // `atelier <folder|file>`: a folder is the project, a file opens in the current one.
        if let Some(path) = target.clone().filter(|p| p.is_dir()) {
            ide.switch_project(path);
        }
        if let Some(path) = session {
            ide.restore_session(&path);
        }
        if let Some(file) = target.filter(|p| p.is_file()) {
            ide.open_file(file);
        }
        ide.cli = crate::cli::serve(&ide.root, ide.repaint.clone());
        if !start_screen {
            ide.graph.set_root(&ide.root, ide.repaint.clone());
        }
        crate::stats::start();
        ide.apply_theme(&cc.egui_ctx);
        if let Some(line) = crate::crash::previous() {
            ide.push_note(
                tr!("crash.previous_title"),
                trf!(
                    "crash.previous",
                    line = line,
                    path = crate::crash::crash_log().display()
                ),
                true,
                true,
            );
        }
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
            if self.start_screen {
                // The window had no project: what it is given becomes it.
                let project = if path.is_dir() {
                    Some(path.clone())
                } else {
                    path.parent().map(Path::to_path_buf)
                };
                if let Some(project) = project {
                    self.leave_start_screen(ctx, project);
                }
                if path.is_file() {
                    self.open_file(path);
                }
            } else if path.is_dir() {
                self.open_project(path);
            } else {
                self.open_file(path);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    fn apply_theme(&self, ctx: &egui::Context) {
        ui::set_fonts(ctx, &self.settings.fonts);
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
                self.status = trf!("error_unreadable_reload_session", e = e);
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
        self.status = tr!("hot_reloaded").into();
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
                self.status = trf!("error_detail", e = format!("{:#}", e));
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
        match crate::i18n::error_message(&message) {
            Some(error) => self.push_note(tr!("error"), error.to_string(), true, true),
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
                let read_only = doc.read_only;
                self.documents.push(doc);
                self.active = self.documents.len() - 1;
                self.status = if read_only {
                    tr!("file_opened_read_only")
                } else {
                    tr!("file_opened")
                }
                .into();
            }
            Err(e) => self.status = trf!("error_detail", e = format!("{:#}", e)),
        }
    }
    /// Once a second, reloads the unmodified documents that another program changed.
    fn watch_files(&mut self, ctx: &egui::Context) {
        const EVERY: Duration = Duration::from_secs(1);
        if self.last_watch.elapsed() >= EVERY {
            self.last_watch = Instant::now();
            let mut errors = Vec::new();
            for doc in &mut self.documents {
                if !doc.dirty() && doc.changed_on_disk() {
                    // What was there is kept before another program replaces it.
                    if let Some(path) = &doc.path {
                        history::record(&crate::cli::data_dir(), &self.root, path, &doc.text);
                    }
                }
                if !doc.dirty()
                    && doc.changed_on_disk()
                    && let Err(e) = doc.reload()
                {
                    errors.push(format!("{} : {e:#}", doc.title()));
                }
            }
            if let Some(error) = errors.first() {
                self.status = trf!("error_reloading", error = error);
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
        self.file_index.invalidate();
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
                failed.push((index, trf!("error_detail", e = format!("{:#}", e))));
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
            .set_directory(self.projects_folder())
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
        if self.start_screen {
            // Nothing to replace: no question.
            self.pending_start = Some(path);
        } else if path != self.root {
            self.project_choice = Some(path);
        }
    }
    /// The window of the start screen becomes the one of `project`.
    fn leave_start_screen(&mut self, ctx: &egui::Context, project: PathBuf) {
        self.start_screen = false;
        self.show_projects = false;
        self.switch_project(project);
        if let Some(window) = &mut self.window {
            window.center(crate::window::DEFAULT_SIZE);
        }
        if self.settings.window_maximized {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }
    }
    /// Each project window is its own process, as IntelliJ on Windows.
    fn open_window(&mut self, path: PathBuf) {
        let result = std::env::current_exe().and_then(|exe| {
            // Detached: closing this window, or what started it, leaves the new one open.
            crate::window::spawn_detached(
                std::process::Command::new(exe)
                    .arg("--new-window")
                    .arg(project::simplify(&path))
                    .current_dir(project::simplify(&path))
                    .env_remove(crate::dev::CHILD_ENV)
                    .env_remove(crate::dev::SESSION_ENV),
            )
        });
        match result {
            Ok(_) => {
                self.remember(&path);
                self.status = trf!("project_opened_new_window", path = path.display());
            }
            Err(e) => self.status = trf!("error_cannot_open_new_window", e = e),
        }
    }
    /// Remembers whether the window is maximized, for the next one, and keeps its taskbar
    /// button on the monitor it is on.
    fn track_window(&mut self, ctx: &egui::Context) {
        let (maximized, minimized) =
            ctx.input(|i| (i.viewport().maximized, i.viewport().minimized));
        if !self.start_screen
            && minimized != Some(true)
            && let Some(maximized) = maximized
        {
            self.settings.window_maximized = maximized;
        }
        if let Some(wait) = self.window.as_mut().and_then(|w| w.follow_monitor()) {
            ctx.request_repaint_after(wait);
        }
    }
    /// The plugins let go of what they kept for the files that were closed (pictures,
    /// copies of the text): a big file gives its memory back when its tab closes.
    fn release_closed_files(&mut self, ctx: &egui::Context) {
        let open = self.documents.iter().filter_map(|d| d.path.as_ref());
        if open.eq(self.open_paths.iter()) {
            return;
        }
        let now: Vec<PathBuf> = self
            .documents
            .iter()
            .filter_map(|d| d.path.clone())
            .collect();
        let closed: Vec<PathBuf> = std::mem::replace(&mut self.open_paths, now)
            .into_iter()
            .filter(|p| !self.open_paths.contains(p))
            .collect();
        for path in &closed {
            for plugin in &mut self.builtins {
                plugin.file_closed(ctx, path);
            }
        }
    }
    fn remember(&mut self, path: &Path) {
        self.settings.recent.retain(|p| p != path);
        self.settings.recent.insert(0, path.to_path_buf());
        self.settings.recent.truncate(MAX_RECENT);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        self.settings.opened.insert(path.to_path_buf(), now);
        let recent = &self.settings.recent;
        self.settings.opened.retain(|p, _| recent.contains(p));
    }
    fn switch_project(&mut self, root: PathBuf) {
        self.root = root.canonicalize().unwrap_or(root);
        self.documents.clear();
        self.active = 0;
        self.tree.clear();
        self.tree_selected = None;
        self.tree_multi.clear();
        self.file_index.invalidate();
        self.expanded = BTreeSet::from([self.root.clone()]);
        self.terminals.clear();
        self.terminal_error.clear();
        let root = self.root.clone();
        self.remember(&root);
        (self.plugins, self.plugin_errors) = plugins::load(&self.root);
        self.languages = Arc::new(crate::languages::Service::new(
            &self.root,
            self.repaint.clone(),
        ));
        self.problems = (0, 0);
        self.graph.set_root(&self.root, self.repaint.clone());
        self.status = trf!("project_root", root = self.root.display());
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
            self.status = tr!("file_already_open_another_tab").into();
            return false;
        }
        let result = self.documents[index].save(path.clone(), save_as);
        if self.report(result) {
            for plugin in &mut self.builtins {
                plugin.file_saved(&path);
            }
            history::record(
                &crate::cli::data_dir(),
                &self.root,
                &path,
                &self.documents[index].text,
            );
            self.tree.clear();
            self.graph.file_changed();
            self.status = tr!("saved").into();
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
                    self.languages
                        .close(doc_key(&self.documents[index].path, index));
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
        let root = project::simplify(&self.root);
        let env = if self.settings.disabled_builtins.contains("runtimes") {
            Vec::new()
        } else {
            crate::plugins::builtins::runtimes::environment(&root)
        };
        match Terminal::start(&root, shell, &env) {
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
                self.terminal_error =
                    trf!("cannot_start", name = shell.name, e = format!("{:#}", e));
                None
            }
        }
    }
    /// Opens a new terminal tab running `shell`, or the default shell.
    fn start_terminal(&mut self, shell: Option<Shell>) {
        // A project of a WSL distribution gets a shell inside that distribution.
        let shell = shell.unwrap_or_else(|| {
            crate::wsl::parse(&self.root)
                .map_or_else(Shell::default_shell, |w| Shell::wsl(&w.distro))
        });
        if let Some(pane) = self.new_pane(&shell) {
            self.terminals.push(TerminalTab {
                active: pane.id,
                root: Node::Leaf(pane),
                title: None,
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
        let found = self
            .terminals
            .iter_mut()
            .flat_map(|tab| tab.root.leaves_mut())
            .find(|p| p.id == pane);
        if let Some(pane) = found {
            let _ = pane.term.write(b"\r");
        }
    }

    /// The user started an AI in a terminal: connects it to the MCP server of the IDE if they
    /// said so before, asks if they were never asked, and lets it start in the other cases.
    fn offer_mcp(&mut self, pane: u64, agent: crate::plugins::builtins::mcp::agents::Agent) {
        use crate::plugins::builtins::mcp::agents;
        let key = format!("{}|{}", self.root.display(), agent.id());
        if agents::is_connected(
            agent,
            &self.root,
            crate::plugins::builtins::mcp::address().as_deref(),
        ) {
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
        match crate::plugins::builtins::mcp::agents::connect(
            agent,
            &self.root,
            crate::plugins::builtins::mcp::address().as_deref(),
        ) {
            Ok(_) => self.status = trf!("connected_atelier_mcp", agent = agent.name()),
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
            if tab.root.split_with(
                &|p: &Pane| p.id == target,
                stacked,
                before,
                Node::Leaf(pane),
            ) {
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
        self.active_terminal = self
            .active_terminal
            .min(self.terminals.len().saturating_sub(1));
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
            self.status = tr!("open_file_run").into();
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
                self.status = tr!("error_type_file_cannot_run").into();
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
                self.status = tr!("error_type_file_cannot_run").into();
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
        let from = doc.path.clone().map(|path| NavPoint {
            path,
            offset: doc.view.cursor,
        });
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
        let here = self.documents.get(self.active).and_then(|d| {
            Some(NavPoint {
                path: d.path.clone()?,
                offset: d.view.cursor,
            })
        });
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
            if let Some(doc) = self
                .documents
                .iter_mut()
                .find(|d| d.path.as_ref() == Some(&canonical))
            {
                let new = apply(&doc.text, &edits);
                doc.view.replace_text(&mut doc.text, &new);
                changed += 1;
            } else if let Ok(text) = fs::read_to_string(&path)
                && fs::write(&path, apply(&text, &edits)).is_ok()
            {
                changed += 1;
            }
        }
        self.status = trf!("renamed_other_file_s_changed", changed = changed);
    }

    /// The selection of the active document, for the find bar and the search window.
    fn selected_text(&self) -> Option<String> {
        let doc = self.documents.get(self.active)?;
        let range = doc.view.selection();
        (!range.is_empty() && range.end <= doc.text.len())
            .then(|| doc.text.get(range).map(str::to_string))
            .flatten()
    }

    fn open_palette(&mut self) {
        self.open_palette_as(PaletteMode::Everywhere);
    }

    fn open_palette_as(&mut self, mode: PaletteMode) {
        self.palette_mode = mode;
        self.show_palette = true;
        self.palette_query.clear();
        self.palette_index = 0;
        self.file_index.invalidate();
    }

    fn perform_action(&mut self, index: usize) {
        match index {
            0 => self.choose_project(false),
            1 => self.choose_file(),
            2 => {
                self.save_all();
            }
            3 => self.set_terminal(!self.show_terminal),
            4 => self.open_settings(Page::Appearance),
            5 => self.open_settings(Page::Plugins),
            6 => self.run_file(),
            7 => {
                self.tree.clear();
                (self.plugins, self.plugin_errors) = plugins::load(&self.root);
            }
            8 => self.choose_project(true),
            9 => self.show_new_file = true,
            10 => self.show_notifications = !self.show_notifications,
            11 => self.reformat_code(),
            12 => self.reformat(FormatRequest {
                imports_only: true,
                ..Default::default()
            }),
            13 => self.open_projects_page(),
            14 => self.toggle_bookmark(),
            15 => self.open_palette_as(PaletteMode::Bookmarks),
            16 => self.open_palette_as(PaletteMode::History),
            17 => self.split_to(false),
            18 => self.split_to(true),
            19 => self.split = None,
            _ => self.start_inspection(),
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
        if pressed(command | Modifiers::ALT | Modifiers::SHIFT, Key::L) {
            self.open_reformat_dialog();
        }
        if pressed(command | Modifiers::ALT, Key::L) {
            self.reformat_code();
        }
        if pressed(command | Modifiers::ALT, Key::O) {
            self.reformat(FormatRequest {
                imports_only: true,
                ..Default::default()
            });
        }
        if pressed(command | Modifiers::ALT, Key::S) {
            self.open_settings(self.settings_page);
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
            self.open_palette_as(PaletteMode::Symbols);
        }
        if pressed(Modifiers::ALT, Key::Insert) {
            self.new_document();
        }
        if pressed(command, Key::F4) && !self.documents.is_empty() {
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
        if pressed(Modifiers::SHIFT, Key::F11) {
            self.open_palette_as(PaletteMode::Bookmarks);
        }
        if pressed(Modifiers::NONE, Key::F11) {
            self.toggle_bookmark();
        }
        for (i, key) in [
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
        ]
        .into_iter()
        .enumerate()
        {
            let number = i as u8 + 1;
            if pressed(command | Modifiers::SHIFT, key) {
                self.number_bookmark(number);
            } else if pressed(command, key) {
                self.go_to_bookmark(number);
            }
        }
        if pressed(command | Modifiers::SHIFT, Key::E) {
            self.open_palette_as(PaletteMode::Locations);
        }
        if pressed(command | Modifiers::SHIFT, Key::A) {
            self.open_palette_as(PaletteMode::Actions);
        }
        if pressed(command | Modifiers::SHIFT, Key::Tab) {
            self.switch_key(false);
        }
        if pressed(command, Key::Tab) {
            self.switch_key(true);
        }
        if pressed(command, Key::E) {
            self.open_palette_as(PaletteMode::Recent);
        }
        if pressed(command, Key::G) && !self.documents.is_empty() {
            self.open_palette_as(PaletteMode::Line);
        }
        if pressed(command, Key::Backtick) || pressed(Modifiers::ALT, Key::F12) {
            self.set_terminal(!self.show_terminal);
        }
        if pressed(Modifiers::ALT, Key::Num1) {
            self.toggle_tool(&PROJECT_TOOL);
        }
        if pressed(Modifiers::SHIFT, Key::F10) || pressed(Modifiers::NONE, Key::F5) {
            self.run_selected();
        }
        if pressed(command, Key::F2) {
            self.stop_run();
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
                        if ui::icon_button(ui, "settings", tr!("settings_ctrl_alt_s")).clicked() {
                            self.open_settings(self.settings_page);
                        }
                        if ui::icon_button(ui, "search", tr!("search_everywhere_double_shift"))
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
                                    .add(egui::Button::new(label.get()).shortcut_text(*shortcut))
                                    .clicked()
                                {
                                    self.perform_action(index);
                                    ui.close();
                                }
                            }
                        });
                        ui.add_enabled_ui(false, |ui| {
                            ui::icon_button(ui, "debug", tr!("debugger_not_available"));
                        });
                        if self.run_pane().is_some()
                            && ui::icon_button(ui, "stop", tr!("stop_run")).clicked()
                        {
                            self.stop_run();
                        }
                        let label = self.run_label();
                        if ui::icon_button(ui, "run", &trf!("run_label", label = label)).clicked() {
                            self.run_selected();
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
                ui.menu_button(tr!("file"), |ui| {
                    if item(ui, tr!("new_file"), "Alt+Insert") {
                        self.new_document();
                        ui.close();
                    }
                    if item(ui, tr!("new_file_project"), "") {
                        self.show_new_file = true;
                        ui.close();
                    }
                    ui.separator();
                    if item(ui, tr!("open_file_dialog"), "Ctrl+O") {
                        self.choose_file();
                        ui.close();
                    }
                    if item(ui, tr!("open_project_dialog"), "") {
                        self.choose_project(false);
                        ui.close();
                    }
                    if item(ui, tr!("open_project_new_window"), "") {
                        self.choose_project(true);
                        ui.close();
                    }
                    if item(ui, tr!("projects.all"), "") {
                        self.open_projects_page();
                        ui.close();
                    }
                    ui.menu_button(tr!("recent_projects"), |ui| {
                        for path in self.settings.recent.clone() {
                            if path != self.root && item(ui, &path.display().to_string(), "") {
                                self.open_project(path);
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    if item(ui, tr!("save"), "Ctrl+S") {
                        self.save_document(self.active, false);
                        ui.close();
                    }
                    if item(ui, tr!("save_as_dialog"), "Ctrl+Shift+S") {
                        self.save_document(self.active, true);
                        ui.close();
                    }
                    if item(ui, tr!("save_all"), "") {
                        self.save_all();
                        ui.close();
                    }
                    ui.separator();
                    if item(ui, tr!("settings_dialog"), "Ctrl+Alt+S") {
                        self.open_settings(self.settings_page);
                        ui.close();
                    }
                    ui.separator();
                    if item(ui, tr!("exit"), "") {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        ui.close();
                    }
                });
                ui.menu_button(tr!("edit"), |ui| {
                    if item(ui, tr!("find_dialog"), "Ctrl+F") {
                        let selected = self.selected_text();
                        self.find.show(selected, false);
                        ui.close();
                    }
                    if item(ui, tr!("rename_dialog"), &crate::i18n::shortcut("Shift+F6")) {
                        if let Some(doc) = self.documents.get_mut(self.active) {
                            doc.view.request_rename(&doc.text);
                        }
                        ui.close();
                    }
                    if item(ui, tr!("replace_dialog"), "Ctrl+R") {
                        let selected = self.selected_text();
                        self.find.show(selected, true);
                        ui.close();
                    }
                    if item(ui, tr!("search_everywhere_dialog"), tr!("double_shift")) {
                        self.open_palette();
                        ui.close();
                    }
                });
                ui.menu_button(tr!("code"), |ui| {
                    if item(ui, tr!("reformat_code"), "Ctrl+Alt+L") {
                        self.reformat_code();
                        ui.close();
                    }
                    if item(ui, tr!("reformat_file"), tr!("ctrl_alt_shift_l")) {
                        self.open_reformat_dialog();
                        ui.close();
                    }
                    if item(ui, tr!("optimize_imports"), "Ctrl+Alt+O") {
                        self.reformat(FormatRequest {
                            imports_only: true,
                            ..Default::default()
                        });
                        ui.close();
                    }
                });
                ui.menu_button(tr!("view"), |ui| {
                    ui.menu_button(tr!("tool_windows"), |ui| {
                        for tool in self.all_tools() {
                            if item(ui, tool.title.get(), &crate::i18n::shortcut(tool.shortcut)) {
                                self.open_tool(tool);
                                ui.close();
                            }
                        }
                    });
                    ui.menu_button(tr!("theme"), |ui| {
                        for (index, name) in ui::THEMES.iter().enumerate() {
                            if ui.radio(self.settings.theme == index, *name).clicked() {
                                self.settings.theme = index;
                                self.apply_theme(ui.ctx());
                                ui.close();
                            }
                        }
                    });
                });
                ui.menu_button(tr!("run"), |ui| {
                    if item(ui, tr!("run_current_file"), "Shift+F10") {
                        self.run_file();
                        ui.close();
                    }
                    self.external_commands_menu(ui);
                });
                let menus: Vec<_> = self
                    .builtins
                    .iter()
                    .filter(|p| self.builtin_enabled(p.id()) && p.applies())
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
                ui.menu_button(tr!("window"), |ui| {
                    if item(ui, tr!("new_project_window"), "") {
                        self.choose_project(true);
                        ui.close();
                    }
                    if item(ui, "Notifications", "") {
                        self.show_notifications = !self.show_notifications;
                        ui.close();
                    }
                });
                ui.menu_button(tr!("help"), |ui| {
                    if item(ui, tr!("keymap"), "") {
                        self.open_settings(Page::Keymap);
                        ui.close();
                    }
                    if item(ui, tr!("about"), "") {
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
            ui::section(ui, tr!("recent_projects"));
            for path in self
                .settings
                .recent
                .iter()
                .take(10)
                .cloned()
                .collect::<Vec<_>>()
            {
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
                    if ui.button(tr!("open_window")).clicked() {
                        self.request(Pending::Project(path.clone()));
                        ui.close();
                    }
                    if ui.button(tr!("open_new_window")).clicked() {
                        self.open_window(path.clone());
                        ui.close();
                    }
                    if ui.button(tr!("remove_from_list")).clicked() {
                        self.settings.recent.retain(|p| p != &path);
                        ui.close();
                    }
                });
            }
            if ui.button(tr!("projects.all")).clicked() {
                self.open_projects_page();
                ui.close();
            }
            ui.separator();
            if ui.button(tr!("open_project_dialog")).clicked() {
                self.choose_project(false);
                ui.close();
            }
            if ui.button(tr!("open_new_window_dialog")).clicked() {
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
                if ui.button(tr!("clone_git_repository")).clicked() {
                    self.run_builtin_command(plugin, command.id);
                    ui.close();
                }
            }
        });
    }

    /// Run configuration widget: IntelliJ's "Current File".
    fn run_widget(&mut self, ui: &mut egui::Ui) {
        let label = self.run_label();
        let response = ui::icon_text_button(ui, None, &label, true);
        egui::Popup::menu(&response)
            .id(egui::Id::new("run_menu"))
            .show(|ui| {
                ui.set_min_width(220.0);
                if ui
                    .add(
                        egui::Button::new(trf!("run_label", label = label))
                            .shortcut_text("Shift+F10"),
                    )
                    .clicked()
                {
                    self.run_selected();
                    ui.close();
                }
                if self.run_pane().is_some()
                    && ui
                        .add(egui::Button::new(tr!("stop_run")).shortcut_text("Ctrl+F2"))
                        .clicked()
                {
                    self.stop_run();
                    ui.close();
                }
                ui.separator();
                self.run_configs_menu(ui);
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
        if control(ui, "winClose", true, tr!("close")) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let (icon, tooltip) = if maximized {
            ("winRestore", tr!("restore"))
        } else {
            ("winMaximize", tr!("maximize"))
        };
        if control(ui, icon, false, tooltip) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }
        if control(ui, "winMinimize", false, tr!("minimize")) {
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
            "notifications" => self.show_notifications && self.right_tool.is_none(),
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
            tool.title.get().to_string()
        } else {
            format!(
                "{}  {}",
                tool.title.get(),
                crate::i18n::shortcut(tool.shortcut)
            )
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
            hide = ui::icon_button(ui, "hide", tr!("hide")).clicked();
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
        ui::menu_title(ui, tr!("context_graph"));
        let status = self.graph.status();
        if !self.graph.enabled() {
            if ui::menu_item(
                ui,
                Some("refresh"),
                tr!("generate_context_graph"),
                None,
                false,
            )
            .clicked()
            {
                self.graph.enable(self.repaint.clone());
                ui.close();
            }
            ui.label(
                RichText::new(tr!("ais_read_map_project_instead"))
                    .small()
                    .color(self.theme(ui.ctx()).text_secondary),
            );
            return;
        }
        let state = if status.busy {
            tr!("updating").to_string()
        } else {
            trf!(
                "up_date_files_symbols_groups",
                files = status.files,
                symbols = status.symbols,
                groups = status.groups
            )
        };
        ui.label(
            RichText::new(state)
                .small()
                .color(self.theme(ui.ctx()).text_secondary),
        );
        if let Some(error) = &status.error {
            ui.colored_label(self.theme(ui.ctx()).error, error);
        }
        if ui::menu_item(ui, Some("editorPreview"), tr!("open_report"), None, false).clicked() {
            let report = crate::graph::folder(&self.root).join("GRAPH_REPORT.md");
            if report.exists() {
                self.open_file(report);
            }
            ui.close();
        }
        if ui::menu_item(ui, Some("refresh"), tr!("regenerate"), None, false).clicked() {
            self.graph.regenerate();
            ui.close();
        }
        if ui::menu_item(ui, Some("closeSmall"), tr!("disable"), None, false).clicked() {
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
        egui::Frame::new()
            .fill(t.hover)
            .corner_radius(8)
            .inner_margin(Margin::same(10))
            .outer_margin(Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(tr!("generate_context_graph_question")).font(ui::semibold(12.5)),
                );
                ui.label(
                    RichText::new(tr!("map_project_made_without_ai"))
                        .small()
                        .color(t.text_secondary),
                );
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    if ui::primary_button(ui, tr!("generate")).clicked() {
                        self.graph.enable(self.repaint.clone());
                    }
                    if ui::button(ui, tr!("later")).clicked() {
                        self.graph.postpone();
                    }
                    if ui::button(ui, tr!("don_t_ask_again")).clicked() {
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
                let hide = self.tool_header(ui, tr!("project"), |ide, ui| {
                    if ui::icon_button(ui, "collapseAll", tr!("collapse_all")).clicked() {
                        ide.expanded = BTreeSet::from([ide.root.clone()]);
                    }
                    if ui::icon_button(ui, "refresh", tr!("refresh_files_plugins")).clicked() {
                        ide.tree.clear();
                        (ide.plugins, ide.plugin_errors) = plugins::load(&ide.root);
                    }
                    if ui::icon_button(ui, "add", tr!("new_file")).clicked() {
                        ide.show_new_file = true;
                    }
                    let graph = ui::icon_button(
                        ui,
                        "graph",
                        if ide.graph.enabled() {
                            tr!("context_graph_enabled")
                        } else {
                            tr!("context_graph")
                        },
                    );
                    egui::Popup::menu(&graph)
                        .id(egui::Id::new("graph_menu"))
                        .show(|ui| ide.graph_menu(ui));
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
        if self.tool_header(ui, tool.title.get(), |_, _| {}) {
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
                ui::menu_title(ui, &trf!("items_selected", count = selection.len()));
                if ui::menu_item(
                    ui,
                    Some("delete"),
                    tr!("delete_dialog"),
                    Some(tr!("del")),
                    true,
                )
                .clicked()
                {
                    self.tree_dialog = Some(TreeDialog::Delete(selection));
                    ui.close();
                } else if ui::menu_item(ui, Some("copy"), tr!("copy_paths"), None, false).clicked()
                {
                    let paths: Vec<_> = selection
                        .iter()
                        .map(|p| project::simplify(p).display().to_string())
                        .collect();
                    ui.ctx().copy_text(paths.join("\n"));
                    ui.close();
                }
                return;
            }
            if !is_dir
                && ui::menu_item(
                    ui,
                    Some("openNewTab"),
                    tr!("open"),
                    Some(tr!("enter")),
                    false,
                )
                .clicked()
            {
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
            if ui::menu_item(
                ui,
                Some("add"),
                tr!("new_file_dialog"),
                Some("Alt+Insert"),
                false,
            )
            .clicked()
            {
                self.new_name = prefix.clone();
                self.show_new_file = true;
                ui.close();
            }
            if ui::menu_item(ui, Some("newFolder"), tr!("new_folder_dialog"), None, false).clicked()
            {
                self.tree_dialog = Some(TreeDialog::NewFolder(prefix));
                ui.close();
            }
            if !is_root {
                ui::menu_separator(ui);
                if ui::menu_item(ui, Some("edit"), tr!("rename_dialog"), Some("F2"), false)
                    .clicked()
                {
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    self.tree_dialog = Some(TreeDialog::Rename {
                        path: path.to_path_buf(),
                        name,
                    });
                    ui.close();
                }
                if ui::menu_item(
                    ui,
                    Some("delete"),
                    tr!("delete_dialog"),
                    Some(tr!("del")),
                    true,
                )
                .clicked()
                {
                    self.tree_dialog = Some(TreeDialog::Delete(vec![path.to_path_buf()]));
                    ui.close();
                }
            }
            ui::menu_separator(ui);
            if ui::menu_item(ui, Some("copy"), tr!("copy_path"), None, false).clicked() {
                ui.ctx()
                    .copy_text(project::simplify(path).display().to_string());
                ui.close();
            }
            if ui::menu_item(ui, None, tr!("copy_relative_path"), None, false).clicked() {
                let relative = path.strip_prefix(&self.root).unwrap_or(path);
                ui.ctx().copy_text(relative.display().to_string());
                ui.close();
            }
            ui::menu_separator(ui);
            if ui::menu_item(ui, Some("locate"), tr!("show_explorer"), None, false).clicked() {
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
            if is_dir
                && ui::menu_item(ui, Some("terminal"), tr!("open_terminal"), None, false).clicked()
            {
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
    fn tree_drag_row(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        entry: &Path,
        is_dir: bool,
    ) {
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
            let dir = if is_dir { Some(entry) } else { entry.parent() };
            if let Some(dir) =
                dir.filter(|d| !self.tree_drag_set.iter().any(|from| d.starts_with(from)))
            {
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
                trf!("items", count = self.tree_drag_set.len())
            } else {
                from.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            };
            let t = self.theme(ctx);
            ctx.layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new("tree_drag"),
            ))
            .text(
                pointer + vec2(12.0, 12.0),
                Align2::LEFT_TOP,
                name,
                FontId::proportional(13.0),
                t.text,
            );
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
                            self.status = trf!("error_detail", e = format!("{:#}", e));
                            break;
                        }
                    }
                }
                if let [only] = &moved[..] {
                    self.status = trf!("moved", only = only.display());
                } else if moved.len() > 1 {
                    self.status = trf!("items_moved", count = moved.len());
                    self.tree_multi = moved.into_iter().collect();
                }
                if self.tree_multi.len() > 1 || !crate::i18n::is_error(&self.status) {
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
        self.tree_multi = self
            .tree_multi
            .iter()
            .map(|p| remap(p).unwrap_or_else(|| p.clone()))
            .collect();
        self.tree_anchor = self.tree_anchor.take().map(|p| remap(&p).unwrap_or(p));
        self.tree.clear();
        self.file_index.invalidate();
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
            Some(Some(p)) => self
                .documents
                .iter()
                .position(|d| d.path.as_ref() == Some(&p)),
            _ => None,
        }
        .unwrap_or_else(|| self.active.min(self.documents.len().saturating_sub(1)));
        self.expanded.retain(|p| !p.starts_with(path));
        if self
            .tree_selected
            .as_deref()
            .is_some_and(|p| p.starts_with(path))
        {
            self.tree_selected = None;
        }
        self.tree_multi.retain(|p| !p.starts_with(path));
        if self.tree_multi.len() < 2 {
            self.tree_multi.clear();
        }
        self.tree.clear();
        self.file_index.invalidate();
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
        let anchor = self
            .tree_anchor
            .clone()
            .or_else(|| self.tree_selected.clone());
        let from = anchor
            .as_ref()
            .and_then(|a| rows.iter().position(|(p, _)| p == a));
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
                self.tree_selected = self
                    .tree_multi
                    .iter()
                    .next()
                    .cloned()
                    .or(self.tree_selected.take());
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
            select = current
                .and_then(|i| i.checked_sub(1))
                .and_then(|i| rows.get(i))
                .map(|r| r.0.clone());
        }
        if let Some((path, is_dir)) = current.map(|i| rows[i].clone()) {
            let expanded = self.expanded.contains(&path);
            if key(Key::ArrowRight) && is_dir {
                if expanded {
                    select = rows
                        .get(current.unwrap() + 1)
                        .filter(|r| r.0.parent() == Some(&path))
                        .map(|r| r.0.clone());
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
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                self.tree_dialog = Some(TreeDialog::Rename {
                    path: path.clone(),
                    name,
                });
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
        let Some(dialog) = self.tree_dialog.take() else {
            return;
        };
        let t = self.theme(ctx);
        let mut keep = None;
        match dialog {
            TreeDialog::NewFolder(mut name) => {
                let modal = egui::Modal::new(egui::Id::new("new_folder")).show(ctx, |ui| {
                    ui.set_width(420.0);
                    ui::dialog_title(ui, tr!("new_folder"));
                    ui.add(
                        egui::TextEdit::singleline(&mut name)
                            .hint_text("src/module")
                            .desired_width(f32::INFINITY),
                    )
                    .request_focus();
                    ui.label(
                        RichText::new(tr!("path_relative_project"))
                            .small()
                            .color(t.text_secondary),
                    );
                    ui::dialog_buttons(ui, tr!("create"))
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
                            self.status = trf!("error_detail", e = format!("{:#}", e));
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
                    ui::dialog_title(ui, tr!("rename"));
                    ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY))
                        .request_focus();
                    ui::dialog_buttons(ui, tr!("rename"))
                });
                let (ok, cancel) = modal.inner;
                if ok {
                    match project::rename_path(&path, &name) {
                        Ok(new) => {
                            self.path_changed(&path, &new);
                            self.reveal(&new);
                        }
                        Err(e) => {
                            self.status = trf!("error_detail", e = format!("{:#}", e));
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
                    ui::dialog_title(ui, tr!("delete"));
                    if let [path] = &paths[..] {
                        let what = if path.is_dir() {
                            tr!("folder_content")
                        } else {
                            tr!("this_file")
                        };
                        ui.label(trf!(
                            "delete_permanently_deletes",
                            path = path.file_name().unwrap_or_default().to_string_lossy(),
                            what = what
                        ));
                    } else {
                        ui.label(trf!(
                            "delete_these_items_content_folders",
                            count = paths.len()
                        ));
                    }
                    ui::dialog_buttons(ui, tr!("delete"))
                });
                let (ok, cancel) = modal.inner;
                if ok {
                    for path in &paths {
                        // Inside a folder that is deleted too: already gone.
                        if paths
                            .iter()
                            .any(|other| other != path && path.starts_with(other))
                        {
                            continue;
                        }
                        match project::delete_path(path) {
                            Ok(()) => self.path_deleted(path),
                            Err(e) => {
                                self.status = trf!("error_detail", e = format!("{:#}", e));
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

    /// The ignore file that ignores a path, as the plugins say it: the first answer.
    pub(super) fn ignored_by(&self, path: &Path) -> Option<String> {
        self.builtins
            .iter()
            .filter(|p| self.builtin_enabled(p.id()))
            .find_map(|p| p.ignored_by(path))
    }

    fn tree_ui(&mut self, ui: &mut egui::Ui, path: &Path, depth: usize) {
        if depth > 16 {
            ui.weak(tr!("maximum_depth_reached"));
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
                    self.tree_stamps
                        .insert(path.into(), project::modified(path));
                    self.tree.insert(path.into(), entries);
                }
                Err(e) => {
                    ui.colored_label(self.theme(ui.ctx()).error, trf!("cannot_read", e = e));
                    return;
                }
            }
        }
        let mut entries = self.tree.get(path).cloned().unwrap_or_default();
        // What an ignore file ignores can be left out, with the eye of the status bar.
        if self.settings.hide_ignored {
            entries.retain(|(entry, _)| self.ignored_by(entry).is_none());
        }
        for (entry, is_dir) in entries {
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
        let (rect, response) =
            ui.allocate_exact_size(vec2(width, ui::ROW), Sense::click_and_drag());
        #[cfg(test)]
        self.row_rects.push((path.to_path_buf(), rect));
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let selected = self.tree_is_selected(path);
        if self.tree_selected.as_deref() == Some(path) && self.scroll_to_selected {
            response.scroll_to_me(None);
        }
        ui::paint_row(
            ui,
            rect,
            selected,
            response.hovered() && self.tree_drag.is_none(),
        );
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
        // Which ignore file ignores it.
        match self.ignored_by(path) {
            Some(by) => response.on_hover_text(trf!("ignored_by_file", file = by)),
            None => response,
        }
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
            if ui::icon_button(ui, "rerun", tr!("restart_shell")).clicked() {
                restart = true;
            }
            if ui::icon_button(ui, "stop", tr!("interrupt_ctrl_c")).clicked()
                && let Some(pane) = ide
                    .terminals
                    .get_mut(ide.active_terminal)
                    .and_then(|tab| tab.pane())
            {
                let result = pane.term.write(&[3]);
                ide.report(result);
            }
            if ui::icon_button(ui, "unified", tr!("split_down")).clicked() {
                split = Some(true);
            }
            if ui::icon_button(ui, "sideBySide", tr!("split_right")).clicked() {
                split = Some(false);
            }
            let plus = ui::icon_button(ui, "add", tr!("new_terminal"));
            egui::Popup::menu(&plus)
                .id(egui::Id::new("terminal_shells"))
                .show(|ui| {
                    ui::menu_begin(ui);
                    ui::menu_title(ui, tr!("new_terminal"));
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
            let galley = ui.painter().layout_no_wrap(
                tab.name().to_string(),
                FontId::proportional(12.5),
                t.text,
            );
            // What the shells of the tab and their programs use, in a cell of fixed width so that
            // the values changing never move the tabs.
            let usage = tab_usage(tab);
            let usage_width = ui
                .painter()
                .layout_no_wrap(
                    crate::stats::WIDEST_USAGE.into(),
                    FontId::proportional(11.0),
                    t.text_secondary,
                )
                .size()
                .x;
            let width = galley.size().x + 44.0 + usage_width + 8.0;
            if x + width > limit {
                break;
            }
            let rect = Rect::from_min_size(pos2(x, top), vec2(width, 26.0));
            tab_rects.push(rect);
            let active = index == self.active_terminal;
            let response = ui.interact(
                rect,
                egui::Id::new(("terminal_tab", index)),
                Sense::click_and_drag(),
            );
            if response.dragged() && ui.input(|i| i.pointer.is_decidedly_dragging()) {
                self.drag = Some(Drag::Tab(index));
            }
            if active || response.hovered() {
                ui.painter().rect(
                    rect,
                    6.0,
                    if active { t.frame } else { t.hover },
                    Stroke::new(
                        1.0_f32,
                        if active {
                            t.border
                        } else {
                            Color32::TRANSPARENT
                        },
                    ),
                    StrokeKind::Inside,
                );
            }
            ui.painter().galley(
                pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0),
                galley,
                t.text,
            );
            if let Some(text) = usage {
                ui.painter().text(
                    pos2(rect.right() - 30.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    text,
                    FontId::proportional(11.0),
                    t.text_secondary,
                );
            }
            let cross = Rect::from_center_size(
                pos2(rect.right() - 14.0, rect.center().y),
                vec2(18.0, 18.0),
            );
            let cross_response = ui.interact(
                cross,
                egui::Id::new(("terminal_close", index)),
                Sense::click(),
            );
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
                if ui::menu_item(ui, Some("sideBySide"), tr!("split_right"), None, false).clicked()
                {
                    split = Some(false);
                    ui.close();
                }
                if ui::menu_item(ui, Some("unified"), tr!("split_down"), None, false).clicked() {
                    split = Some(true);
                    ui.close();
                }
                ui::menu_separator(ui);
                if tab.count() > 1
                    && ui::menu_item(ui, None, tr!("close_pane"), None, false).clicked()
                {
                    close_pane = true;
                    ui.close();
                }
                if ui::menu_item(ui, Some("closeSmall"), tr!("close_tab"), None, false).clicked() {
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
        // Files clicked in the output, opened once the panes are drawn.
        let mut file_links: Vec<(PathBuf, Option<usize>, Option<usize>)> = Vec::new();
        let root = self.root.clone();
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
                if tab.count() == 1 && tab.root.leaves().first().is_some_and(|p| p.term.exited) {
                    ui.colored_label(t.warning, tr!("session_ended_restart_open_new"));
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
                        let response = ui.interact(
                            head,
                            egui::Id::new(("pane_head", pane.id)),
                            Sense::click_and_drag(),
                        );
                        ui.painter().rect_filled(
                            head,
                            0.0,
                            if pane.id == active_id {
                                t.hover
                            } else {
                                t.frame
                            },
                        );
                        if let Some(usage) = pane.term.pid().and_then(crate::stats::watch) {
                            ui.painter().text(
                                pos2(head.right() - 8.0, head.center().y),
                                Align2::RIGHT_CENTER,
                                crate::stats::format_usage(usage.0, usage.1),
                                FontId::proportional(11.0),
                                t.text_secondary,
                            );
                        }
                        ui.painter().text(
                            pos2(head.left() + 8.0, head.center().y),
                            Align2::LEFT_CENTER,
                            &pane.term.shell.name,
                            FontId::proportional(12.0),
                            if pane.id == active_id {
                                t.text
                            } else {
                                t.text_secondary
                            },
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
                    let area = crate::terminal_ui::show(
                        &mut child,
                        &mut pane.term,
                        &mut pane.selection,
                        &t,
                        &root,
                    );
                    if let Some(agent) = pane.term.pending_ai.take() {
                        mcp_offers.push((pane.id, agent));
                    }
                    if let Some(place) = pane.term.open_file.take() {
                        file_links.push(place);
                    }
                    if area.secondary_clicked() {
                        focused = Some(pane.id);
                    }
                    area.context_menu(|ui| {
                        ui::menu_begin(ui);
                        if ui::menu_item(ui, Some("sideBySide"), tr!("split_right"), None, false)
                            .clicked()
                        {
                            menu_split = Some(false);
                            ui.close();
                        }
                        if ui::menu_item(ui, Some("unified"), tr!("split_down"), None, false)
                            .clicked()
                        {
                            menu_split = Some(true);
                            ui.close();
                        }
                        ui::menu_separator(ui);
                        if split_view
                            && ui::menu_item(ui, None, tr!("close_pane"), None, false).clicked()
                        {
                            menu_close_pane = true;
                            ui.close();
                        }
                        if ui::menu_item(ui, Some("closeSmall"), tr!("close_tab"), None, false)
                            .clicked()
                        {
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
                    let grab = divider
                        .rect
                        .expand(if divider.stacked { 0.0 } else { 3.0 })
                        .expand2(if divider.stacked {
                            vec2(0.0, 3.0)
                        } else {
                            vec2(0.0, 0.0)
                        });
                    let response = ui
                        .interact(
                            grab,
                            egui::Id::new(("terminal_divider", index)),
                            Sense::drag(),
                        )
                        .on_hover_cursor(if divider.stacked {
                            egui::CursorIcon::ResizeVertical
                        } else {
                            egui::CursorIcon::ResizeHorizontal
                        });
                    let line = if divider.stacked {
                        Rect::from_center_size(
                            divider.rect.center(),
                            vec2(divider.rect.width(), 1.0),
                        )
                    } else {
                        Rect::from_center_size(
                            divider.rect.center(),
                            vec2(1.0, divider.rect.height()),
                        )
                    };
                    let color = if response.hovered() || response.dragged() {
                        t.border
                    } else {
                        t.separator
                    };
                    ui.painter().rect_filled(line, 0.0, color);
                    if let Some(pointer) = response.interact_pointer_pos()
                        && response.dragged()
                    {
                        let at = if divider.stacked {
                            pointer.y
                        } else {
                            pointer.x
                        };
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
                    ui.painter()
                        .rect_filled(zone, 4.0, t.accent.gamma_multiply(0.3));
                    ui.painter().rect_stroke(
                        zone,
                        4.0,
                        Stroke::new(1.5_f32, t.accent),
                        StrokeKind::Inside,
                    );
                    drop = Some((pane.id, stacked, before));
                }
                ui.allocate_rect(full, Sense::hover());
            });
        for (pane, agent) in mcp_offers {
            self.offer_mcp(pane, agent);
        }
        for (path, line, column) in file_links {
            let (line, column) = (line.unwrap_or(1).max(1) - 1, column.unwrap_or(1).max(1) - 1);
            builtin::Host::open_at(self, path, line, column, 0);
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
                ui.painter().rect_stroke(
                    bar.shrink(2.0),
                    4.0,
                    Stroke::new(1.5_f32, t.accent),
                    StrokeKind::Inside,
                );
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
                ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::new("terminal_drag"),
                ))
                .text(
                    pointer + vec2(12.0, 12.0),
                    Align2::LEFT_TOP,
                    name,
                    FontId::proportional(12.5),
                    t.text,
                );
            }
        }
        if ctx.input(|i| i.pointer.any_released())
            && let Some(drag) = self.drag.take()
        {
            match (drag, drop) {
                (Drag::Tab(from), Some((target, stacked, before)))
                    if from == self.active_terminal =>
                {
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
            tab.root.split_with(
                &|p: &Pane| p.id == target,
                stacked,
                before,
                Node::Leaf(pane),
            );
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
            title: None,
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
        if tab
            .root
            .split_with(&|p: &Pane| p.id == target, stacked, before, moved.root)
        {
            tab.active = id;
            if let Some(pane) = tab.pane() {
                pane.term.want_focus = true;
            }
        }
        self.active_terminal = active;
    }

    fn right_panel(&mut self, ctx: &egui::Context) {
        if let Some(tool) = self.right_tool {
            let response = egui::SidePanel::right("tool_right")
                .default_width(340.0)
                .width_range(220.0..=760.0)
                .frame(self.island(ctx))
                .show_separator_line(false)
                .show(ctx, |ui| self.tool_panel(ui, tool));
            self.panel_focus(Some(tool.id), response.response.rect);
            return;
        }
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
                    if ui::icon_button(ui, "delete", tr!("clear_all")).clicked() {
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
                                    RichText::new(tr!("no_notifications")).color(t.text_secondary),
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
                                        if ui::icon_button(ui, "closeSmall", tr!("close")).clicked()
                                        {
                                            close = Some(index);
                                        }
                                    },
                                );
                            });
                            let text: String = note.text.chars().take(600).collect();
                            ui.label(text);
                            if note.error && ui.link(tr!("show_notifications")).clicked() {
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
        // The space of the bottom corners stays empty so that the code, painted in a
        // rectangle, does not hide their rounding.
        let response = egui::CentralPanel::default()
            .frame(self.island(ctx).inner_margin(Margin {
                bottom: ISLAND_RADIUS as i8,
                ..Margin::ZERO
            }))
            .show(ctx, |ui| {
                if self.documents.is_empty() {
                    self.welcome(ui);
                    return;
                }
                self.tabs(ui);
                let split_other = self.split_other();
                let (doc, mut other_doc) = match split_other {
                    Some(other) => match self.documents.get_disjoint_mut([self.active, other]) {
                        Ok([doc, other]) => (doc, Some(other)),
                        Err(_) => return,
                    },
                    None => match self.documents.get_mut(self.active) {
                        Some(doc) => (doc, None),
                        None => return,
                    },
                };
                if let Some(loading) = &doc.loading {
                    ui.add(
                        egui::ProgressBar::new(loading.progress())
                            .desired_height(3.0)
                            .corner_radius(0),
                    );
                    return;
                }
                let mut find_action = self
                    .find_request
                    .take()
                    .unwrap_or(crate::find::Action::None);
                if self.find.open {
                    self.find
                        .refresh(self.active, doc.view.revision(), &doc.text);
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
                    self.find
                        .refresh(self.active, doc.view.revision(), &doc.text);
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
                                                (
                                                    EditorLayout::Text,
                                                    "editorOnly",
                                                    tr!("editor_only"),
                                                ),
                                                (
                                                    EditorLayout::Split,
                                                    "editorPreview",
                                                    tr!("editor_preview"),
                                                ),
                                                (
                                                    EditorLayout::Preview,
                                                    "previewOnly",
                                                    tr!("preview_only"),
                                                ),
                                            ] {
                                                if ui::icon_toggle(
                                                    ui,
                                                    icon,
                                                    *layout == choice,
                                                    tooltip,
                                                )
                                                .clicked()
                                                {
                                                    new_layout =
                                                        Some((*plugin, path.clone(), choice));
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
                            pos2(
                                ui.max_rect().right() - galley.size().x - 40.0,
                                ui.max_rect().top() + 14.0,
                            ),
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
                // The second editor, on the right.
                if let Some(pane) = self.split.as_mut() {
                    let down = pane.down;
                    let mut close = false;
                    let mut content = |ui: &mut egui::Ui| {
                        let target: &mut Document = match other_doc.as_mut() {
                            Some(other) => other,
                            None => &mut *doc,
                        };
                        let separate = split_other.is_some();
                        split::show(
                            ui,
                            pane,
                            target,
                            separate.then_some(&self.languages),
                            &self.settings,
                            t,
                            font.clone(),
                        )
                    };
                    if down {
                        let half = (ui.available_height() / 2.0).max(120.0);
                        egui::TopBottomPanel::bottom(egui::Id::new("split_editor"))
                            .resizable(true)
                            .default_height(half)
                            .height_range(80.0..=(ui.available_height() - 80.0).max(100.0))
                            .frame(egui::Frame::NONE)
                            .show_inside(ui, |ui| close = content(ui));
                    } else {
                        let half = (ui.available_width() / 2.0).max(240.0);
                        egui::SidePanel::right(egui::Id::new("split_editor"))
                            .resizable(true)
                            .default_width(half)
                            .width_range(160.0..=(ui.available_width() - 160.0).max(180.0))
                            .frame(egui::Frame::NONE)
                            .show_inside(ui, |ui| close = content(ui));
                    }
                    if close {
                        self.split = None;
                    }
                }
                let annotations = gutter.annotations.as_deref();
                let assist = self
                    .languages
                    .formats(&doc.language)
                    .then(|| crate::editor::Assist {
                        service: &self.languages,
                        doc: crate::languages::DocRef {
                            key: doc_key(&doc.path, self.active),
                            path: doc.path.clone(),
                            language: doc.language.clone(),
                        },
                    });
                let bookmark_lines = Ide::bookmark_lines(
                    &mut self.bookmarks,
                    &mut self.bookmark_rev,
                    self.active,
                    &self.root,
                    &doc.path,
                    &doc.text,
                    doc.view.revision(),
                );
                let options = crate::editor::Options {
                    language: &doc.language,
                    font,
                    theme: t,
                    marks: &gutter.marks,
                    annotations,
                    gutter_commands: &gutter.commands,
                    assist,
                    search: if self.find.open {
                        self.find.matches()
                    } else {
                        &[]
                    },
                    read_only: doc.read_only,
                    inlay_hints: !self.settings.inlay_hints_off,
                    bookmarks: &bookmark_lines,
                    sticky_lines: !self.settings.sticky_lines_off,
                    breadcrumbs: !self.settings.breadcrumbs_off,
                    spellcheck: !self.settings.spellcheck_off,
                };
                let editor_id = egui::Id::new(("source", doc.path.clone(), self.active));
                let output = doc.view.show(ui, editor_id, &mut doc.text, &options);
                self.cursor = (output.line, output.column);
                self.problems = (output.errors, output.warnings);
                if let Some(target) = output.navigate {
                    navigate = Some((target, doc.path.clone(), doc.view.cursor));
                }
                file_edits = output.file_edits;
                if let Some(message) = output.message {
                    self.status = message;
                }
                if output.command.is_some() {
                    gutter_command = output.command;
                }
                {
                    use crate::find::Action;
                    let selection = doc.view.selection();
                    match find_action {
                        Action::Next | Action::Previous => {
                            if let Some(found) =
                                self.find.neighbor(&selection, find_action == Action::Next)
                            {
                                doc.view.select(found.start, found.end);
                            }
                        }
                        Action::Replace => {
                            // The selected match is replaced, then the next one is selected.
                            if self.find.position(&selection).is_some()
                                && let Some(re) = self.find.regex().cloned()
                            {
                                let new = crate::search::replace_all(
                                    &doc.text[selection.clone()],
                                    &re,
                                    &self.find.replace,
                                    self.find.options,
                                );
                                doc.view
                                    .replace_range(&mut doc.text, selection.clone(), &new);
                                self.find
                                    .refresh(self.active, doc.view.revision(), &doc.text);
                                let caret = doc.view.selection();
                                if let Some(next) =
                                    self.find.neighbor(&(caret.start..caret.start), true)
                                {
                                    doc.view.select(next.start, next.end);
                                }
                            } else if let Some(found) = self.find.neighbor(&selection, true) {
                                doc.view.select(found.start, found.end);
                            }
                        }
                        Action::ReplaceAll => {
                            if let Some(re) = self.find.regex().cloned() {
                                let new = crate::search::replace_all(
                                    &doc.text,
                                    &re,
                                    &self.find.replace,
                                    self.find.options,
                                );
                                if new != doc.text {
                                    let count = self.find.matches().len();
                                    doc.view.replace_text(&mut doc.text, &new);
                                    self.status = trf!("replacement_s", count = count);
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
            let more = ui::icon_button(ui, "moreVertical", tr!("tab_options"));
            egui::Popup::menu(&more)
                .id(egui::Id::new("tabs_more"))
                .show(|ui| {
                    ui::menu_begin(ui);
                    if ui::menu_item(ui, Some("closeSmall"), tr!("close_tab"), None, false)
                        .clicked()
                    {
                        close = Some(self.active);
                        ui.close();
                    }
                    if ui::menu_item(ui, None, tr!("close_other_tabs"), None, false).clicked() {
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
                    if ui::menu_item(ui, None, tr!("close_all"), None, false).clicked() {
                        self.documents.retain(Document::dirty);
                        self.active = 0;
                        ui.close();
                    }
                    ui.separator();
                    if ui::menu_item(ui, None, tr!("split_right"), None, false).clicked() {
                        self.split_to(false);
                        ui.close();
                    }
                    if ui::menu_item(ui, None, tr!("split_down"), None, false).clicked() {
                        self.split_to(true);
                        ui.close();
                    }
                    if self.split.is_some()
                        && ui::menu_item(ui, None, tr!("close_split"), None, false).clicked()
                    {
                        self.split = None;
                        ui.close();
                    }
                });
            let list = ui::icon_button(ui, "chevronDown", tr!("open_tabs"));
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
                            let (rect, response) =
                                ui.allocate_exact_size(size, Sense::click_and_drag());
                            tab_rects.push(rect);
                            if response.dragged() && ui.input(|i| i.pointer.is_decidedly_dragging())
                            {
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
                            let tooltip = doc.path.as_deref().map_or(tr!("untitled").into(), |p| {
                                project::simplify(p).display().to_string()
                            });
                            let has_path = doc.path.is_some();
                            response.context_menu(|ui| {
                                ui::menu_begin(ui);
                                if ui::menu_item(
                                    ui,
                                    Some("closeSmall"),
                                    tr!("close"),
                                    Some("Ctrl+F4"),
                                    false,
                                )
                                .clicked()
                                {
                                    close = Some(i);
                                    ui.close();
                                }
                                if ui::menu_item(ui, None, tr!("close_others"), None, false)
                                    .clicked()
                                {
                                    action = Some(TabAction::CloseOthers(i));
                                    ui.close();
                                }
                                if ui::menu_item(ui, None, tr!("close_tabs_right"), None, false)
                                    .clicked()
                                {
                                    action = Some(TabAction::CloseRight(i));
                                    ui.close();
                                }
                                if ui::menu_item(ui, None, tr!("close_all"), None, false).clicked()
                                {
                                    action = Some(TabAction::CloseAll);
                                    ui.close();
                                }
                                if has_path {
                                    ui::menu_separator(ui);
                                    if ui::menu_item(
                                        ui,
                                        Some("copy"),
                                        tr!("copy_path"),
                                        None,
                                        false,
                                    )
                                    .clicked()
                                    {
                                        action = Some(TabAction::CopyPath(i));
                                        ui.close();
                                    }
                                    if ui::menu_item(
                                        ui,
                                        Some("project"),
                                        tr!("show_project"),
                                        None,
                                        false,
                                    )
                                    .clicked()
                                    {
                                        action = Some(TabAction::Reveal(i));
                                        ui.close();
                                    }
                                    if ui::menu_item(
                                        ui,
                                        Some("locate"),
                                        tr!("show_explorer"),
                                        None,
                                        false,
                                    )
                                    .clicked()
                                    {
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
            (tr!("search_everywhere"), tr!("double_shift")),
            (tr!("go_file"), tr!("ctrl_shift_n")),
            (tr!("open_file"), "Ctrl+O"),
            (tr!("go_symbol"), "Ctrl+N"),
            (tr!("new_file"), "Alt+Insert"),
            ("Terminal", "Alt+F12"),
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
            tr!("drop_files_here_open_them"),
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
                            ui::status_text_fixed(ui, &summary, tr!("ram_9999_mb_cpu_99"))
                                .on_hover_text(trf!(
                                    "memory_ram_processor_used_by",
                                    memory_bytes =
                                        crate::stats::format_memory(crate::stats::memory_bytes()),
                                    cpu_percent = format!("{:.1}", crate::stats::cpu_percent())
                                ));
                            ui.add_space(8.0);
                            ctx.request_repaint_after(Duration::from_secs(1));
                        }
                        // Background tasks, next to it, as IntelliJ shows them.
                        let running = crate::tasks::running();
                        if let Some(first) = running.first() {
                            let more = if running.len() > 1 {
                                format!(" (+{})", running.len() - 1)
                            } else {
                                String::new()
                            };
                            let text = ui
                                .label(
                                    RichText::new(format!("{}{more}…", first.title))
                                        .size(12.5)
                                        .color(t.text_secondary),
                                )
                                .on_hover_ui(|ui| {
                                    for task in &running {
                                        ui.label(format!(
                                            "{} · {} s",
                                            task.title,
                                            task.elapsed.as_secs()
                                        ));
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
                                tr!("context_graph_being_prepared").to_string()
                            } else {
                                trf!(
                                    "context_graph_files_symbols_groups",
                                    files = state.files,
                                    symbols = state.symbols,
                                    groups = state.groups
                                )
                            };
                            ui::status_text(ui, tr!("graph")).on_hover_text(text);
                        }
                        // The eye: what an ignore file ignores is shown, dimmed, or left out.
                        let hidden = self.settings.hide_ignored;
                        if ui::icon_toggle(
                            ui,
                            if hidden { "hide" } else { "show" },
                            hidden,
                            if hidden {
                                tr!("show_ignored_files")
                            } else {
                                tr!("hide_ignored_files")
                            },
                        )
                        .clicked()
                        {
                            self.settings.hide_ignored = !hidden;
                            let _ = self.persist_settings();
                        }
                        self.each_builtin(|plugin, ide| plugin.status_bar(ui, ide));
                        if let Some(doc) = self.documents.get(self.active) {
                            // Sampled: a full scan on every frame freezes big files.
                            let sample = doc.text.lines().take(2000).collect::<Vec<_>>().join("\n");
                            let indent = if sample.contains("\n\t") {
                                tr!("tab").to_string()
                            } else {
                                let width = sample
                                    .lines()
                                    .filter_map(|l| {
                                        let n = l.len() - l.trim_start_matches(' ').len();
                                        (n > 0 && !l.trim().is_empty()).then_some(n)
                                    })
                                    .min()
                                    .unwrap_or(4);
                                trf!("spaces", width = width)
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
                                    parts.push(trf!(
                                        "errors_count",
                                        errors = errors,
                                        plural = if errors > 1 { "s" } else { "" }
                                    ));
                                }
                                if warnings > 0 {
                                    parts.push(trf!(
                                        "warning",
                                        warnings = warnings,
                                        plural = if warnings > 1 { "s" } else { "" }
                                    ));
                                }
                                ui.add_space(10.0);
                                let label = ui
                                    .add(
                                        egui::Label::new(
                                            RichText::new(parts.join(", ")).size(12.5).color(
                                                if errors > 0 {
                                                    Color32::from_rgb(0xF0, 0x56, 0x56)
                                                } else {
                                                    Color32::from_rgb(0xE6, 0xB5, 0x4A)
                                                },
                                            ),
                                        )
                                        .sense(Sense::click()),
                                    )
                                    .on_hover_text(tr!("show_problems_alt_6"))
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
        self.projects_dialog(ctx);
        self.new_project_dialog(ctx);
        self.reformat_dialog(ctx);
        self.run_dialog(ctx);
        self.tree_dialogs(ctx);
        if self.show_new_file {
            let modal = egui::Modal::new(egui::Id::new("new_file")).show(ctx, |ui| {
                ui.set_width(420.0);
                ui::dialog_title(ui, tr!("new_file"));
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.new_name)
                        .hint_text("src/main.rs")
                        .desired_width(f32::INFINITY),
                );
                field.request_focus();
                ui.label(
                    RichText::new(tr!("path_relative_project_missing_folders"))
                        .small()
                        .color(self.theme(ui.ctx()).text_secondary),
                );
                ui::dialog_buttons(ui, tr!("create"))
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
                    Err(e) => self.status = trf!("error_detail", e = format!("{:#}", e)),
                }
            } else if cancel || modal.should_close() {
                self.show_new_file = false;
            }
        }
        self.palette(ctx);
        self.comparison_window(ctx);
        self.switcher(ctx);
        self.each_builtin(|plugin, ide| plugin.windows(ctx, ide));
        if let Some((pane, agent)) = self.mcp_offer {
            let modal = egui::Modal::new(egui::Id::new("mcp_offer")).show(ctx, |ui| {
                ui.set_width(500.0);
                ui::dialog_title(ui, &trf!("connect_atelier", agent = agent.name()));
                ui.label(trf!("atelier_can_give_access_tools", agent = agent.name()));
                ui.add_space(6.0);
                ui.label(
                    RichText::new(match agent {
                        crate::plugins::builtins::mcp::agents::Agent::Codex => {
                            tr!("connection_written_codex_config_toml")
                        }
                        _ => tr!("connection_written_configuration_files_project"),
                    })
                    .small()
                    .color(self.theme(ui.ctx()).text_secondary),
                );
                ui.add_space(12.0);
                let mut choice = None;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui::primary_button(ui, tr!("connect")).clicked() {
                        choice = Some(1);
                    }
                    if ui::button(ui, tr!("not_now")).clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        choice = Some(0);
                    }
                    if ui::button(ui, tr!("don_t_ask_again")).clicked() {
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
                ui::dialog_title(ui, tr!("open_project"));
                ui.label(trf!(
                    "open_window_new_window",
                    path = path.file_name().unwrap_or_default().to_string_lossy()
                ));
                ui.add_space(12.0);
                let mut choice = None;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui::button(ui, tr!("cancel")).clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        choice = Some(0);
                    }
                    if ui::button(ui, tr!("this_window")).clicked() {
                        choice = Some(1);
                    }
                    if ui::primary_button(ui, tr!("new_window")).clicked() {
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
                ui::dialog_title(ui, tr!("unsaved_changes"));
                ui.label(tr!("save_modified_files_before_continuing"));
                ui.add_space(4.0);
                for (i, doc) in self.documents.iter().enumerate() {
                    if doc.dirty() && (!matches!(action, Pending::Close(index) if index != i)) {
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::Image::new(self.file_icon(
                                    ui.ctx(),
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
                    if ui::button(ui, tr!("cancel")).clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        self.pending = None;
                    }
                    if ui::button(ui, tr!("don_t_save")).clicked() {
                        self.pending = None;
                        self.perform(action.clone());
                    }
                    if ui::primary_button(ui, tr!("save")).clicked() {
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

    fn plugin_key(&self, id: &str) -> String {
        format!("{}::{id}", self.root.display())
    }

    /// Go to Symbol: the definitions of the project matching what is typed.
    fn symbol_hits(&mut self) -> Vec<Hit> {
        self.symbol_index.poll();
        let root = self.root.clone();
        let repaint = self.repaint.clone();
        let listed = root.clone();
        self.symbol_index
            .refresh(&root, move || file_index::list(&listed), move || repaint());
        let query = self.palette_query.trim().to_string();
        if query.is_empty() {
            return Vec::new();
        }
        let key = PaletteKey {
            mode: PaletteMode::Symbols,
            query: query.clone(),
            version: (
                Arc::as_ptr(&self.symbol_index.symbols) as usize,
                self.symbol_index.symbols.len() as u64,
            ),
        };
        if let Some((kept, hits)) = &self.palette_cache
            && *kept == key
        {
            return hits.clone();
        }
        let hits: Vec<Hit> = symbols::search(&self.symbol_index.symbols, &query)
            .into_iter()
            .map(|symbol| {
                let relative = symbol
                    .path
                    .strip_prefix(&self.root)
                    .unwrap_or(&symbol.path)
                    .to_string_lossy()
                    .replace('\\', "/");
                Hit {
                    label: symbol.name.clone(),
                    hint: format!("{} · {relative}:{}", symbol.kind, symbol.line + 1),
                    icon: None,
                    file: Some(symbol.path.clone()),
                    target: Target::Place(symbol.path.clone(), symbol.line),
                }
            })
            .collect();
        self.palette_cache = Some((key, hits.clone()));
        hits
    }

    /// Ctrl+F2: stops what runs in the active terminal, as Ctrl+C there would.
    fn stop_run(&mut self) {
        // The terminal of the selected configuration, else the active one.
        let run = self.run_pane();
        let pane = match run {
            Some(id) => self
                .terminals
                .iter_mut()
                .flat_map(|tab| tab.root.leaves_mut())
                .find(|p| p.id == id),
            None => self
                .terminals
                .get_mut(self.active_terminal)
                .and_then(|tab| tab.pane()),
        }
        .filter(|pane| !pane.term.exited);
        match pane {
            Some(pane) => {
                let result = pane.term.write(b"\x03");
                self.report(result);
            }
            None => self.status = tr!("nothing_running").into(),
        }
    }

    /// The files used, most recent first: the open ones and the ones closed since.
    fn recent_hits(&self, query: &str) -> Vec<Hit> {
        self.recent_files
            .iter()
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().to_lowercase().contains(query))
            })
            .map(|path| Hit {
                label: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                hint: path
                    .parent()
                    .map(|p| {
                        p.strip_prefix(&self.root)
                            .unwrap_or(p)
                            .display()
                            .to_string()
                    })
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
        let project = self
            .root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
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

    /// The files whose name holds `query` (in lower case), the closest names first. The search is
    /// kept until the query or the list of files changes.
    fn file_hits(&mut self, query: &str) -> Vec<Hit> {
        let key = PaletteKey {
            mode: PaletteMode::Everywhere,
            query: query.to_string(),
            version: (0, self.file_index.generation),
        };
        if let Some((kept, hits)) = &self.palette_cache
            && *kept == key
        {
            return hits.clone();
        }
        let mut matches: Vec<_> = self
            .file_index
            .entries()
            .iter()
            .filter(|e| e.name.contains(query))
            .collect();
        // Names starting with the query first, then the shortest.
        matches.sort_by_key(|e| (!e.name.starts_with(query), e.name.len()));
        let hits: Vec<Hit> = matches
            .into_iter()
            .take(60)
            .map(|entry| {
                let path = &entry.path;
                let relative = path
                    .parent()
                    .and_then(|p| p.strip_prefix(&self.root).ok())
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                Hit {
                    label: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    hint: relative,
                    icon: None,
                    file: Some(path.clone()),
                    target: Target::File(path.clone()),
                }
            })
            .collect();
        self.palette_cache = Some((key, hits.clone()));
        hits
    }

    fn palette_hits(&mut self) -> Vec<Hit> {
        let query = self.palette_query.to_lowercase();
        match self.palette_mode {
            PaletteMode::Recent => return self.recent_hits(&query),
            PaletteMode::Line => return line_hit(&query).into_iter().collect(),
            PaletteMode::Symbols => return self.symbol_hits(),
            PaletteMode::Locations => return self.location_hits(),
            PaletteMode::Bookmarks => return self.bookmark_hits(&query),
            PaletteMode::History => return self.history_hits(),
            PaletteMode::Everywhere | PaletteMode::Actions => {}
        }
        let everywhere = self.palette_mode == PaletteMode::Everywhere;
        let mut hits = Vec::new();
        for (i, doc) in self.documents.iter().enumerate().filter(|_| everywhere) {
            if doc.title().to_lowercase().contains(&query) {
                hits.push(Hit {
                    label: doc.title(),
                    hint: tr!("open_tab").into(),
                    icon: None,
                    file: Some(doc.path.clone().unwrap_or_default()),
                    target: Target::Tab(i),
                });
            }
        }
        for (i, (label, shortcut, icon)) in ACTIONS.iter().enumerate() {
            if label.get().to_lowercase().contains(&query) {
                hits.push(Hit {
                    label: label.get().to_string(),
                    hint: shortcut.to_string(),
                    icon: Some(*icon),
                    file: None,
                    target: Target::Action(i),
                });
            }
        }
        for plugin in &self.builtins {
            if !self.builtin_enabled(plugin.id()) || !plugin.applies() {
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
        if everywhere && !query.is_empty() {
            let repaint = self.repaint.clone();
            self.file_index.poll();
            self.file_index.refresh(&self.root, move || repaint());
            hits.extend(self.file_hits(&query));
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
                                        PaletteMode::Everywhere => {
                                            (tr!("search_everywhere"), tr!("files_actions_tabs"))
                                        }
                                        PaletteMode::Recent => {
                                            (tr!("recent_files"), tr!("order_use"))
                                        }
                                        PaletteMode::Actions => {
                                            (tr!("find_action"), tr!("actions_commands"))
                                        }
                                        PaletteMode::Locations => {
                                            (tr!("recent_locations"), tr!("recent_locations_hint"))
                                        }
                                        PaletteMode::Bookmarks => {
                                            (tr!("bookmarks"), tr!("bookmarks_hint"))
                                        }
                                        PaletteMode::History => {
                                            (tr!("local_history"), tr!("local_history_hint"))
                                        }
                                        PaletteMode::Line => {
                                            (tr!("go_line"), tr!("line_line_column"))
                                        }
                                        PaletteMode::Symbols if self.symbol_index.busy() => {
                                            (tr!("go_symbol"), tr!("reading_symbols"))
                                        }
                                        PaletteMode::Symbols => {
                                            (tr!("go_symbol"), tr!("classes_functions_project"))
                                        }
                                    };
                                    ui.label(RichText::new(title).font(ui::semibold(13.0)));
                                    ui.label(
                                        RichText::new(subtitle).small().color(t.text_secondary),
                                    );
                                });
                                ui.add_space(6.0);
                                let field = ui::search_field(
                                    ui,
                                    &mut self.palette_query,
                                    if self.palette_mode == PaletteMode::Line {
                                        tr!("line_number")
                                    } else {
                                        tr!("type_search")
                                    },
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
                                            RichText::new(tr!("no_results"))
                                                .color(t.text_secondary),
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
                                            egui::Image::new(self.file_icon(ui.ctx(), path))
                                                .paint_at(ui, icon)
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
                Target::Place(path, line) => Host::open_at(self, path, line, 0, 0),
                Target::Version(path) => self.compare_version(&path),
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
        self.builtins.iter().any(|p| p.id() == id && p.core())
            || !self.settings.disabled_builtins.contains(id)
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
        if owned(self.right_tool) {
            self.right_tool = None;
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
            .filter(|p| self.builtin_enabled(p.id()) && p.applies())
            .flat_map(|p| p.tool_windows())
            .collect()
    }

    /// Closes the tool windows of plugins that no longer apply (another project was opened).
    fn close_missing_tools(&mut self) {
        let tools = self.tool_windows();
        let gone = |tool: Option<&'static ToolWindow>| {
            tool.is_some_and(|t| !tools.iter().any(|w| w.id == t.id))
        };
        if gone(self.left_tool) {
            self.left_tool = None;
        }
        if gone(self.right_tool) {
            self.right_tool = None;
        }
        if gone(self.bottom_tool) {
            self.bottom_tool = None;
        }
    }

    fn tool_open(&self, tool: &ToolWindow) -> bool {
        let open = match tool.dock {
            Dock::Left => self.left_tool,
            Dock::Bottom => self.bottom_tool,
            Dock::Right => self.right_tool,
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
                _ if tool.dock == Dock::Right => self.right_tool = None,
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
            ("notifications", _) => {
                self.show_notifications = true;
                self.right_tool = None;
            }
            (_, Dock::Left) => self.left_tool = Some(tool),
            (_, Dock::Bottom) => {
                self.bottom_tool = Some(tool);
                self.show_terminal = false;
            }
            (_, Dock::Right) => {
                self.right_tool = Some(tool);
                self.show_notifications = false;
            }
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
    /// The run configuration the tab is for, shown in place of the shell.
    title: Option<String>,
    /// Id of the pane that has the keyboard.
    active: u64,
}

impl TerminalTab {
    fn pane(&mut self) -> Option<&mut Pane> {
        let active = self.active;
        self.root.leaves_mut().into_iter().find(|p| p.id == active)
    }
    fn name(&self) -> &str {
        if let Some(title) = &self.title {
            return title;
        }
        self.root
            .leaves()
            .first()
            .map_or("", |p| p.term.shell.name.as_str())
    }
    fn count(&self) -> usize {
        self.root.leaves().len()
    }
}

/// The memory and processor use of a terminal tab: its shells and what runs in them.
fn tab_usage(tab: &TerminalTab) -> Option<String> {
    let mut total: Option<(u64, f32)> = None;
    for pane in tab.root.leaves() {
        if let Some((memory, cpu)) = pane.term.pid().and_then(crate::stats::watch) {
            let sum = total.unwrap_or((0, 0.0));
            total = Some((sum.0 + memory, sum.1 + cpu));
        }
    }
    total.map(|(memory, cpu)| crate::stats::format_usage(memory, cpu))
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
        (
            false,
            false,
            Rect::from_min_size(pos2(rect.center().x, rect.top()), half_w),
        )
    } else if y < 0.5 {
        (true, true, Rect::from_min_size(top, half_h))
    } else {
        (
            true,
            false,
            Rect::from_min_size(pos2(rect.left(), rect.center().y), half_h),
        )
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
    fn inspect_project(&mut self) {
        self.start_inspection();
    }
    fn inspection_progress(&self) -> Option<(usize, usize)> {
        Ide::inspection_progress(self)
    }
    fn inspect_files(&self, files: &[PathBuf]) -> Vec<builtin::Problem> {
        self.check_before_commit(files)
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
    fn run_configuration(&mut self, name: String, command: String) {
        self.run_temporary(&name, &command);
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
                let start = doc.text[..d.range.start.min(doc.text.len())]
                    .rfind('\n')
                    .map_or(0, |i| i + 1);
                let end = d
                    .range
                    .end
                    .min(doc.text.len())
                    .max(d.range.start.min(doc.text.len()));
                out.push(builtin::Problem {
                    path: path.clone(),
                    line,
                    column: doc.text[start..d.range.start.min(doc.text.len())]
                        .chars()
                        .count(),
                    length: doc.text[d.range.start.min(doc.text.len())..end]
                        .chars()
                        .count()
                        .max(1),
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
        // What the language servers found in the other files of the project.
        out.extend(self.server_problems());
        // The files of the project that Inspect Code looked at, unless they are open.
        if let Some(inspection) = &self.inspection {
            out.extend(
                inspection
                    .problems
                    .iter()
                    .filter(|p| {
                        !self
                            .documents
                            .iter()
                            .any(|d| d.path.as_ref() == Some(&p.path))
                    })
                    .cloned(),
            );
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
            self.status = trf!("error_reloading", error = error);
        }
        self.tree.clear();
        self.file_index.invalidate();
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
            && let Some(page) = std::env::var("ATELIER_SHOT_PAGE")
                .ok()
                .and_then(|p| p.parse().ok())
        {
            let page: usize = page;
            self.open_settings(Page::TREE[page.min(Page::TREE.len() - 1)].0);
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
            if std::env::var_os("ATELIER_SHOT_RUN").is_some() {
                self.run_configs().temporary("gradle build", "gradle build");
                self.open_run_dialog();
            }
            if let Ok(query) = std::env::var("ATELIER_SHOT_SYMBOLS") {
                self.open_palette_as(PaletteMode::Symbols);
                self.palette_query = query;
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
            if std::env::var_os("ATELIER_SHOT_PROJECTS").is_some() {
                self.open_projects_page();
            }
            // Any tool window, by its id.
            if let Ok(id) = std::env::var("ATELIER_SHOT_TOOL")
                && let Some(tool) = self.all_tools().into_iter().find(|t| t.id == id)
            {
                self.open_tool(tool);
            }
            if let Ok(query) = std::env::var("ATELIER_SHOT_FIND") {
                self.find.show(
                    Some(query),
                    std::env::var_os("ATELIER_SHOT_REPLACE").is_some(),
                );
            }
        }
        // A language server needs a few seconds before it can answer.
        let slow = std::env::var_os("ATELIER_SHOT_CURSOR").is_some();
        if pass == if slow { 150 } else { 8 }
            && let Some(offset) = std::env::var("ATELIER_SHOT_CURSOR")
                .ok()
                .and_then(|o| o.parse::<usize>().ok())
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
        if pass
            == if slow {
                300
            } else if late {
                60
            } else {
                20
            }
        {
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
            let button = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            match pass {
                6..=9 => raw_input.events.push(egui::Event::PointerMoved(a)),
                10 => raw_input.events.push(button(a, true)),
                11..=14 => {
                    let f = (pass - 10) as f32 / 4.0;
                    raw_input
                        .events
                        .push(egui::Event::PointerMoved(a + (b - a) * f));
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
        self.poll_inspection();
        self.track_recent_files();
        self.update_window_title(ctx);
        self.serve_command_line(ctx);
        if let Some(project) = self.pending_start.take() {
            self.leave_start_screen(ctx, project);
        }
        self.track_window(ctx);
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
        if self.start_screen {
            Self::resize_edges(ctx);
            self.start_screen_ui(ctx);
            self.dialogs(ctx);
            self.balloons(ctx);
            return;
        }
        if self.pending.is_none()
            && self.project_choice.is_none()
            && !self.show_palette
            && !self.show_settings
            && !self.show_projects
            && !self.terminals.get(self.active_terminal).is_some_and(|tab| {
                tab.root.leaves().iter().any(|p| p.term.focused) && self.show_terminal
            })
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
                let Some(id) = tab
                    .root
                    .leaves()
                    .iter()
                    .find(|p| p.term.exited)
                    .map(|p| p.id)
                else {
                    break;
                };
                tab.root.remove(&|p: &Pane| p.id == id);
                if let Some(first) = tab.root.leaves_mut().into_iter().next()
                    && tab.active == id
                {
                    first.term.want_focus = true;
                    tab.active = first.id;
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
        self.close_missing_tools();
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
        self.release_closed_files(ctx);
    }
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        ui::theme_by_index(self.settings.theme)
            .frame
            .to_normalized_gamma_f32()
    }
    /// eframe calls it every 30 seconds and on exit; the window's own state stays with eframe.
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        if let Err(e) = self.persist_settings() {
            eprintln!("settings: {e:#}");
        }
    }
}

#[cfg(test)]
#[path = "../test/app/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../test/app/perf_probe.rs"]
mod perf_probe;
