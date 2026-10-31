//! Built-in plugins: compiled into the IDE, available in every project, enabled by default and
//! switchable in the Plugins window.
//!
//! External plugins (`external.rs`) are JSON manifests stored in a project that can only run shell
//! commands. A built-in plugin extends the interface itself through the hooks of
//! [`BuiltinPlugin`]: tool windows, toolbar and status bar widgets, editor gutter, project tree
//! colors, shortcuts and palette commands. It talks back to the IDE through [`Host`].

use eframe::egui::{self, Color32};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dock {
    /// Shares the left side with the project view, one tool window at a time.
    Left,
    /// Shares the bottom with the terminal, one tool window at a time.
    Bottom,
    /// Right side, as notifications.
    Right,
}

pub struct ToolWindow {
    /// Unique across all built-in plugins.
    pub id: &'static str,
    pub title: crate::i18n::Text,
    /// Name of an icon of the IntelliJ set (`ui::icon_source`).
    pub icon: &'static str,
    pub dock: Dock,
    /// With the English names of the keys: `i18n::shortcut` shows it.
    pub shortcut: &'static str,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MarkKind {
    Added,
    Modified,
    /// Lines removed just before `line`.
    Deleted,
}

/// Change marker drawn next to the line numbers (0-based line).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineMark {
    pub line: usize,
    pub kind: MarkKind,
}

impl MarkKind {
    pub fn color(self) -> Color32 {
        match self {
            Self::Added => Color32::from_rgb(98, 181, 67),
            Self::Modified => Color32::from_rgb(108, 153, 235),
            Self::Deleted => Color32::from_rgb(140, 145, 155),
        }
    }
}

/// How the editor shows a document: source, source and preview side by side, or preview only.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EditorLayout {
    Text,
    #[default]
    Split,
    Preview,
}

/// Entry of the command palette or of the editor gutter menu.
pub struct Command {
    pub id: &'static str,
    pub label: String,
}

/// An error or warning found in a document, for the Problems window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub path: PathBuf,
    /// 0-based line and column, and length in characters.
    pub line: usize,
    pub column: usize,
    pub length: usize,
    /// 1 error, 2 warning, 3 information, 4 hint.
    pub severity: u8,
    pub message: String,
    pub source: String,
}

/// A tab of the editor, as plugins and the MCP server see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenDocument {
    pub path: Option<PathBuf>,
    pub title: String,
    pub dirty: bool,
    pub active: bool,
}

/// The active editor: where the caret is and what is selected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorSnapshot {
    pub path: Option<PathBuf>,
    pub language: String,
    /// 1-based line and column of the caret.
    pub line: usize,
    pub column: usize,
    pub selection: Option<String>,
}

/// What a built-in plugin may ask the IDE.
pub trait Host {
    fn root(&self) -> &Path;
    fn active_file(&self) -> Option<PathBuf>;
    fn open_file(&mut self, path: PathBuf);
    /// Runs a command in a terminal of its own (a new tab), e.g. to start an AI.
    fn run_in_new_terminal(&mut self, _command: String) {}
    /// Runs a command of the project as a run configuration named `name`: a temporary one the
    /// run widget offers again, in a terminal tab of its own.
    fn run_configuration(&mut self, _name: String, command: String) {
        self.run_in_new_terminal(command);
    }
    /// The tabs of the editor.
    fn open_documents(&self) -> Vec<OpenDocument> {
        Vec::new()
    }
    /// The caret and selection of the active editor.
    fn editor_state(&self) -> Option<EditorSnapshot> {
        None
    }
    /// The problems of the open documents, and of the project once it was inspected.
    fn problems(&self) -> Vec<Problem> {
        Vec::new()
    }
    /// Looks for the syntax errors of every source file of the project, in the background.
    fn inspect_project(&mut self) {}
    /// Files read so far and files to read, while the project is being inspected.
    fn inspection_progress(&self) -> Option<(usize, usize)> {
        None
    }
    /// The syntax errors of these files, read now (the check before a commit).
    fn inspect_files(&self, _files: &[PathBuf]) -> Vec<Problem> {
        Vec::new()
    }
    /// The selected text of the active document.
    fn selected_text(&self) -> Option<String> {
        None
    }
    /// Opens a file with a range selected: 0-based line, column and length in characters.
    fn open_at(&mut self, path: PathBuf, _line: usize, _column: usize, _length: usize) {
        self.open_file(path);
    }
    /// Asks to switch projects; unsaved documents are confirmed first.
    fn open_project(&mut self, path: PathBuf);
    /// Saves the modified documents that have a path. False if one could not be saved.
    fn save_files(&mut self) -> bool;
    /// Reloads unmodified documents changed on disk and refreshes the project tree.
    fn reload_files(&mut self);
    fn show_tool_window(&mut self, tool: &'static ToolWindow);
    /// Status message, kept in the notification log. Messages starting with "Erreur : " (or
    /// "Error: ", see `i18n::error_message`) also show an error balloon.
    fn notify(&mut self, message: String);
    /// Balloon notification at the bottom right, as IntelliJ shows after a push or a failure.
    fn balloon(&mut self, title: &str, text: String, error: bool) {
        let prefix = match error {
            true => tr!("error_prefix"),
            false => "",
        };
        let colon = tr!("text");
        self.notify(format!("{prefix}{title}{colon}{text}"));
    }
}

#[allow(unused_variables)]
pub trait BuiltinPlugin {
    fn id(&self) -> &'static str;
    /// A tool window of the IDE itself rather than a plugin: not listed in the plugins page, and
    /// always on.
    fn core(&self) -> bool {
        false
    }
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    /// Whether the plugin has something to do in the open project (a `pom.xml` for Maven…).
    /// When it has not, its tool windows and its commands are hidden, as in IntelliJ; it still
    /// gets `update`, to notice when the project changes.
    fn applies(&self) -> bool {
        true
    }
    fn version(&self) -> &'static str {
        "0.0.1"
    }
    fn tool_windows(&self) -> &'static [ToolWindow] {
        &[]
    }
    /// Called every frame before drawing: background work, project changes.
    fn update(&mut self, ctx: &egui::Context, host: &mut dyn Host) {}
    fn shortcuts(&mut self, ctx: &egui::Context, host: &mut dyn Host) {}
    fn file_saved(&mut self, path: &Path) {}
    /// The last tab of `path` closed: what the plugin kept for it (pictures, copies of the
    /// text) goes, and its memory with it.
    fn file_closed(&mut self, ctx: &egui::Context, path: &Path) {}
    /// Widgets after the project menu of the title bar.
    fn toolbar(&mut self, ui: &mut egui::Ui, host: &mut dyn Host) {}
    /// Widgets of the status bar, laid out right to left.
    fn status_bar(&mut self, ui: &mut egui::Ui, host: &mut dyn Host) {}
    fn tool_window(&mut self, id: &str, ui: &mut egui::Ui, host: &mut dyn Host) {}
    /// Floating windows and dialogs.
    fn windows(&mut self, ctx: &egui::Context, host: &mut dyn Host) {}
    fn commands(&self) -> Vec<Command> {
        Vec::new()
    }
    fn run_command(&mut self, id: &str, host: &mut dyn Host) {}
    fn file_color(&self, path: &Path) -> Option<Color32> {
        None
    }
    /// The ignore file that ignores `path` (`.gitignore`, `.dockerignore`…), when a plugin knows
    /// one does. The project tree shows what is ignored dimmed, with the name of the file that
    /// ignores it in a tooltip, and hides it when the user turns the eye off. Any plugin that
    /// reads an ignore file can say so: they are all asked, the first answer is shown.
    fn ignored_by(&self, path: &Path) -> Option<String> {
        None
    }
    /// Folder of the plugin whose `extensions` icons apply to every file of that extension
    /// (`msoffice` for `msoffice/extensions/docx.svg`).
    fn extensions_dir(&self) -> Option<&'static str> {
        None
    }
    /// Icon of the files this plugin handles, in the project tree and the tabs: the key of an
    /// icon of the plugin (`icon_set`), e.g. `"mermaid/icon"`.
    fn file_icon(&self, path: &Path) -> Option<&'static str> {
        None
    }
    /// The icon of a file for this plugin: `file_icon`, else the one of its extension.
    fn file_icon_for(&self, path: &Path) -> Option<&'static str> {
        self.file_icon(path).or_else(|| {
            self.extensions_dir()
                .and_then(|dir| crate::icon_set::extension_icon(dir, path))
        })
    }
    fn line_marks(&mut self, path: &Path, text: &str) -> Vec<LineMark> {
        Vec::new()
    }
    /// Text shown left of the line numbers, one line per editor line (e.g. blame annotations).
    fn annotations(&mut self, path: &Path) -> Option<String> {
        None
    }
    fn gutter_commands(&self, path: &Path) -> Vec<Command> {
        Vec::new()
    }
    /// Icon of the plugin in the settings, from the IntelliJ set.
    fn icon(&self) -> &'static str {
        "settings"
    }
    /// Some when the plugin previews this document; the IDE then shows the layout switch
    /// at the top right of the editor, as IntelliJ does for Markdown.
    fn editor_layout(&self, path: &Path) -> Option<EditorLayout> {
        None
    }
    /// False when the layout switch makes no sense for this document (e.g. an image).
    fn layout_switch(&self, path: &Path) -> bool {
        true
    }
    /// True when the plugin displays this file itself: it is opened without being read as text.
    fn opens_binary(&self, path: &Path) -> bool {
        false
    }
    fn set_editor_layout(&mut self, path: &Path, layout: EditorLayout) {}
    /// Draws the preview of a document. `scroll` is the position of the source editor
    /// (0 to 1) when it just moved, for synchronized scrolling. The preview may edit the
    /// text, e.g. to tick a task list item.
    fn preview(&mut self, ui: &mut egui::Ui, path: &Path, text: &mut String, scroll: Option<f32>) {}
}

pub fn all() -> Vec<Box<dyn BuiltinPlugin>> {
    vec![
        Box::new(crate::plugins::builtins::git::GitPlugin::default()),
        Box::new(crate::plugins::builtins::markdown::MarkdownPlugin::default()),
        Box::new(crate::plugins::builtins::mermaid::MermaidPlugin::default()),
        Box::new(crate::plugins::builtins::mcp::McpPlugin::default()),
        Box::new(crate::plugins::builtins::svg::SvgPlugin::default()),
        Box::new(crate::plugins::builtins::csv::CsvPlugin::default()),
        Box::new(crate::languages::javascript::JavaScriptPlugin),
        Box::new(crate::languages::rust::RustPlugin),
        Box::new(crate::languages::java::JavaPlugin),
        Box::new(crate::features::search::SearchPlugin::default()),
        Box::new(crate::features::problems::ProblemsPlugin::default()),
        Box::new(crate::plugins::builtins::msoffice::MsOfficeViewerPlugin::default()),
        Box::new(crate::plugins::builtins::image::ImagePlugin::default()),
        Box::new(crate::plugins::builtins::k8s::K8sPlugin::default()),
        Box::new(crate::plugins::builtins::node::NodePlugin::default()),
        Box::new(crate::plugins::builtins::runtimes::RuntimesPlugin::default()),
        Box::new(crate::plugins::builtins::maven::MavenPlugin::default()),
        Box::new(crate::plugins::builtins::gradle::GradlePlugin::default()),
        Box::new(crate::plugins::builtins::translations::TranslationsPlugin::default()),
    ]
}

#[cfg(test)]
#[path = "../../test/plugins/builtin/tests.rs"]
mod tests;
