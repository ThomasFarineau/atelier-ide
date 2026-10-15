//! Language assistance in the editor: the completion popup, the wavy underline of problems
//! and the tooltip shown when the pointer rests on code. The analysis itself comes from
//! [`crate::languages`]; this module is how the editor asks for it and draws the answers.

use super::{EditorState, Options, column_of, is_word};
use crate::{
    languages::{
        Completion, CompletionItem, Diagnostic, DocRef, FileEdit, Hover, ItemKind, Location,
        Oneshot, QuickFix, Service, Severity, Symbol, TextEdit, fuzzy_match,
    },
    plugins::builtin::{LineMark, MarkKind},
    ui::Theme,
};
use eframe::egui::{
    self, Align2, Color32, FontId, Key, Modifiers, Order, Painter, Pos2, Rect, RichText, Stroke,
    StrokeKind, Ui, pos2, text::CCursor, vec2,
};
use std::{
    ops::Range,
    sync::Arc,
    time::{Duration, Instant},
};

/// What the editor needs to ask for language assistance.
pub struct Assist<'a> {
    pub service: &'a Service,
    pub doc: DocRef,
}

pub(super) const ROWS: usize = 12;
const ROW_HEIGHT: f32 = 22.0;
const POPUP_WIDTH: f32 = 480.0;
const HOVER_DELAY: Duration = Duration::from_millis(450);

pub(super) struct Popup {
    /// Where the word being completed starts.
    from: usize,
    items: Vec<CompletionItem>,
    /// Indexes of `items` that match what is typed, best first, with the matched characters.
    filtered: Vec<(usize, Vec<usize>)>,
    selected: usize,
    scroll: usize,
    /// Where it was drawn last frame, to take clicks.
    rect: Option<Rect>,
    /// The item whose documentation is being asked for.
    resolving: Option<(usize, Oneshot<Option<CompletionItem>>)>,
}

struct Pending {
    answer: Oneshot<Option<Completion>>,
    explicit: bool,
}

#[derive(Default)]
struct HoverState {
    word: Option<Range<usize>>,
    since: Option<Instant>,
    requested: bool,
    answer: Option<Oneshot<Option<Hover>>>,
    shown: Option<Hover>,
}

/// A jump asked for with Ctrl+click or Ctrl+B, waiting for the answers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Nav {
    /// Ctrl+click, Ctrl+B: the declaration, or the usages when already on it.
    Definition,
    /// Ctrl+Shift+B.
    Type,
    /// Ctrl+Alt+B.
    Implementation,
    /// Alt+F7.
    Usages,
}

struct Job {
    /// The word the jump started from.
    word: Range<usize>,
    offset: usize,
    kind: Nav,
    /// False while asking for the declaration, true while asking for the usages.
    usages: bool,
    answer: Oneshot<Vec<Location>>,
}

/// What choosing an entry does.
pub(super) enum Choice {
    Go(Location),
    Fix(QuickFix),
    /// A word to add to the dictionary of the user.
    Dictionary(String),
    /// The refactorings of the selection (Ctrl+Alt+Shift+T).
    Refactor,
    /// The ways to surround the selection (Ctrl+Alt+T).
    Surround,
}

pub(super) struct Entry {
    /// The line of code, or the title of the action.
    pub(super) label: String,
    /// Where it is, shown at the right.
    pub(super) detail: String,
    /// The kind of symbol, drawn as a badge.
    pub(super) kind: Option<ItemKind>,
    /// Nesting, for the structure of a file.
    pub(super) indent: usize,
    pub(super) choice: Choice,
}

/// A list to choose from: the places of a name with several declarations or usages, or the
/// actions offered at the caret.
pub(super) struct Chooser {
    pub(super) title: &'static str,
    pub(super) entries: Vec<Entry>,
    /// Indexes of the entries that match the filter, in the order shown.
    pub(super) shown: Vec<usize>,
    /// What was typed, when the list can be filtered.
    pub(super) filter: Option<String>,
    /// Index in `shown`.
    pub(super) selected: usize,
    pub(super) scroll: usize,
    pub(super) anchor: Pos2,
    /// Opened at the caret (actions) rather than at the pointer (places).
    pub(super) at_caret: bool,
    /// Opened at the top of the editor (structure).
    pub(super) centered: bool,
    pub(super) rect: Option<Rect>,
}

impl Chooser {
    pub(super) fn new(title: &'static str, entries: Vec<Entry>, at_caret: bool) -> Self {
        Self {
            title,
            shown: (0..entries.len()).collect(),
            entries,
            filter: None,
            selected: 0,
            scroll: 0,
            anchor: Pos2::ZERO,
            at_caret,
            centered: false,
            rect: None,
        }
    }

    /// Keeps the entries that match what was typed, best first.
    fn refilter(&mut self) {
        let Some(query) = &self.filter else { return };
        if query.is_empty() {
            self.shown = (0..self.entries.len()).collect();
        } else {
            let mut scored: Vec<(i32, usize)> = self
                .entries
                .iter()
                .enumerate()
                .filter_map(|(i, e)| fuzzy_match(query, &e.label).map(|(score, _)| (score, i)))
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            self.shown = scored.into_iter().map(|(_, i)| i).collect();
        }
        self.selected = 0;
        self.scroll = 0;
    }
}

/// A rename in progress: the name being typed, and the answer of the language.
struct Rename {
    word: Range<usize>,
    name: String,
    /// The field takes the focus, with its text selected, at the next frame.
    focus: bool,
    submit: bool,
    error: Option<String>,
    answer: Option<Oneshot<Result<Vec<FileEdit>, String>>>,
}

/// The highlight of the name under the caret: where else it appears in the document.
#[derive(Default)]
struct UsageState {
    word: Option<Range<usize>>,
    rev: u64,
    since: Option<Instant>,
    requested: bool,
    answer: Option<Oneshot<Vec<Range<usize>>>>,
    ranges: Vec<Range<usize>>,
}

/// Scrolls the rows of a list: `lines` is the wheel movement, negative going down.
fn scroll_chooser(chooser: &mut Chooser, lines: f32) {
    let max = chooser.shown.len().saturating_sub(ROWS);
    let step = lines.abs().ceil() as usize;
    chooser.scroll = if lines < 0.0 {
        (chooser.scroll + step).min(max)
    } else {
        chooser.scroll.saturating_sub(step)
    };
}

fn place_entries(locations: Vec<Location>) -> Vec<Entry> {
    locations
        .into_iter()
        .map(|location| Entry {
            detail: match &location.path {
                Some(path) => format!(
                    "{}:{}",
                    path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    location.line + 1
                ),
                None => trf!("line", line = location.line + 1),
            },
            label: location.preview.clone(),
            kind: None,
            indent: 0,
            choice: Choice::Go(location),
        })
        .collect()
}

#[derive(Default)]
struct GotoHover {
    /// The word under the pointer while Ctrl is held, and the answer about it.
    word: Option<Range<usize>>,
    answer: Option<Oneshot<Vec<Location>>>,
    /// The word leads somewhere: it is drawn as a link.
    link: bool,
    /// Where it leads, for the preview.
    places: Vec<Location>,
}

/// The edits of a step of Reformat Code, or why there are none.
type FormatAnswer = Oneshot<Result<Vec<TextEdit>, String>>;

/// What Reformat Code does, in order.
#[derive(Clone, Debug, PartialEq)]
enum FormatStep {
    Imports,
    /// The code, in these lines only (0-based, end excluded) or in the whole file.
    Code(Option<Vec<Range<usize>>>),
}

/// Reformat Code, as asked from the shortcut or from the Reformat File dialog.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormatRequest {
    pub organize_imports: bool,
    /// Only these lines (0-based, end excluded); the whole file when None.
    pub lines: Option<Vec<Range<usize>>>,
    /// Only the imports, not the code (Optimize Imports).
    pub imports_only: bool,
}

#[derive(Default)]
pub struct AssistState {
    pub(super) popup: Option<Popup>,
    pending: Option<Pending>,
    goto_hover: GotoHover,
    job: Option<Job>,
    pub(super) chooser: Option<Chooser>,
    /// Ctrl+B was pressed: go to the declaration of the word at the caret.
    goto_here: bool,
    /// Alt+Enter was pressed: ask for the actions at the caret.
    pub(super) want_fixes: bool,
    fixes: Option<Oneshot<Vec<QuickFix>>>,
    /// The mouse button is down for a Ctrl+click: the caret must not follow.
    pub ctrl_press: bool,
    /// Where to jump to, taken by the editor's caller.
    pub navigate: Option<Location>,
    /// A completion to ask for at the next frame: whether the user asked for it.
    want: Option<bool>,
    hover: HoverState,
    goto_pending: Option<(usize, Nav)>,
    rename: Option<Rename>,
    /// Changes to this document that the editor applies at its next frame.
    pub apply: Option<Vec<TextEdit>>,
    /// Changes to other files, for the caller.
    pub file_edits: Vec<(std::path::PathBuf, Vec<TextEdit>)>,
    usages: UsageState,
    /// Ctrl+F12 was pressed: show the structure of the file.
    want_symbols: bool,
    symbols: Option<Oneshot<Vec<Symbol>>>,
    /// Reformat Code: the steps left, and the answer awaited with the version it was asked for.
    format_steps: Vec<FormatStep>,
    format_answer: Option<(FormatStep, u64, FormatAnswer)>,
    /// A message for the status bar, taken by the editor's caller.
    pub message: Option<String>,
    had_focus: bool,
    pub diagnostics: Arc<Vec<Diagnostic>>,
}

impl AssistState {
    pub fn focus_changed(&mut self, focused: bool) {
        if focused {
            self.had_focus = true;
        } else if self.had_focus {
            self.had_focus = false;
            self.popup = None;
            self.hover = HoverState::default();
        }
    }

    pub fn counts(&self) -> (usize, usize) {
        let count = |s| self.diagnostics.iter().filter(|d| d.severity == s).count();
        (count(Severity::Error), count(Severity::Warning))
    }
}

pub(super) fn severity_color(severity: Severity) -> Color32 {
    match severity {
        Severity::Error => Color32::from_rgb(0xF0, 0x56, 0x56),
        Severity::Warning => Color32::from_rgb(0xE6, 0xB5, 0x4A),
        Severity::Info => Color32::from_rgb(0x6C, 0x9B, 0xEB),
        Severity::Hint => Color32::from_rgb(0x8C, 0x91, 0x9B),
    }
}

fn kind_style(kind: ItemKind) -> (&'static str, Color32) {
    match kind {
        ItemKind::Function => ("f", Color32::from_rgb(0xA5, 0x7B, 0xDB)),
        ItemKind::Method => ("m", Color32::from_rgb(0xA5, 0x7B, 0xDB)),
        ItemKind::Variable => ("v", Color32::from_rgb(0x5C, 0x9C, 0xE6)),
        ItemKind::Constant => ("c", Color32::from_rgb(0x5C, 0x9C, 0xE6)),
        ItemKind::Parameter => ("p", Color32::from_rgb(0x7F, 0xB6, 0xC8)),
        ItemKind::Property => ("p", Color32::from_rgb(0x65, 0xB5, 0x65)),
        ItemKind::Class => ("C", Color32::from_rgb(0xE8, 0xA3, 0x3D)),
        ItemKind::Interface => ("I", Color32::from_rgb(0xE8, 0xA3, 0x3D)),
        ItemKind::Enum => ("E", Color32::from_rgb(0xE8, 0xA3, 0x3D)),
        ItemKind::Keyword => ("k", Color32::from_rgb(0xCF, 0x8E, 0x6D)),
        ItemKind::Module => ("M", Color32::from_rgb(0x9A, 0xA7, 0xB0)),
        ItemKind::File => ("F", Color32::from_rgb(0x9A, 0xA7, 0xB0)),
        ItemKind::Folder => ("D", Color32::from_rgb(0xD2, 0xB4, 0x6E)),
        ItemKind::Snippet => ("s", Color32::from_rgb(0xC7, 0x7D, 0xBB)),
        ItemKind::Text => ("t", Color32::from_rgb(0x8C, 0x8C, 0x8C)),
    }
}

impl EditorState {
    /// Sends the text to the language service when it changed, takes its answers and asks for
    /// what the editor wanted since the last frame. Returns whether a repaint is needed soon.
    pub(super) fn assist_frame(&mut self, ui: &Ui, text: &str, opt: &Options) {
        let Some(assist) = &opt.assist else {
            self.assist = AssistState::default();
            return;
        };
        assist.service.update(&assist.doc, text, self.rev);
        self.assist.diagnostics = assist.service.diagnostics(&assist.doc);

        if let Some(mut rename) = self.assist.rename.take() {
            if std::mem::take(&mut rename.submit) {
                rename.error = None;
                rename.answer = Some(assist.service.rename(
                    &assist.doc,
                    text,
                    rename.word.start,
                    rename.name.trim(),
                ));
            }
            let mut keep = true;
            if let Some(answer) = &rename.answer {
                if let Some(result) = answer.take() {
                    rename.answer = None;
                    match result {
                        Ok(plan) => {
                            keep = false;
                            for file in plan {
                                match file.path {
                                    None => self.assist.apply = Some(file.edits),
                                    Some(path) => self.assist.file_edits.push((path, file.edits)),
                                }
                            }
                        }
                        Err(message) => rename.error = Some(message),
                    }
                } else {
                    ui.ctx().request_repaint_after(Duration::from_millis(30));
                }
            }
            if keep {
                self.assist.rename = Some(rename);
            }
        }
        if std::mem::take(&mut self.assist.goto_here) {
            self.assist_goto(self.cursor);
        }
        if std::mem::take(&mut self.assist.want_fixes) {
            self.assist.popup = None;
            self.assist.chooser = None;
            self.assist.fixes = Some(assist.service.quick_fixes(&assist.doc, text, self.cursor));
        }
        if std::mem::take(&mut self.assist.want_symbols) {
            self.assist.popup = None;
            self.assist.chooser = None;
            self.assist.symbols = Some(assist.service.symbols(&assist.doc, text));
        }
        if let Some(answer) = &self.assist.symbols {
            if let Some(list) = answer.take() {
                self.assist.symbols = None;
                self.open_structure(text, list);
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(30));
            }
        }
        self.format_frame(ui, text, assist);
        self.usage_frame(ui, text, opt);
        if let Some(answer) = &self.assist.fixes {
            if let Some(fixes) = answer.take() {
                self.assist.fixes = None;
                // A typo under the caret comes first.
                let mut entries = self.spelling_entries(text);
                entries.extend(fixes.into_iter().map(|fix| Entry {
                    label: fix.title.clone(),
                    detail: String::new(),
                    kind: None,
                    indent: 0,
                    choice: Choice::Fix(fix),
                }));
                // The actions of the IDE on the code, as IntelliJ's intentions list them.
                for (label, shortcut, choice) in [
                    (tr!("refactor_this"), "Ctrl+Alt+Shift+T", Choice::Refactor),
                    (tr!("surround_with"), "Ctrl+Alt+T", Choice::Surround),
                ] {
                    entries.push(Entry {
                        label: label.to_string(),
                        detail: shortcut.to_string(),
                        kind: None,
                        indent: 0,
                        choice,
                    });
                }
                if !entries.is_empty() {
                    self.assist.chooser = Some(Chooser::new(tr!("quick_fixes"), entries, true));
                }
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(30));
            }
        }
        self.poll_goto(ui, text, opt);
        if let Some(explicit) = self.assist.want.take() {
            let answer = assist
                .service
                .complete(&assist.doc, text, self.cursor, explicit);
            self.assist.pending = Some(Pending { answer, explicit });
        }
        if let Some(pending) = &self.assist.pending {
            if let Some(result) = pending.answer.take() {
                let explicit = pending.explicit;
                self.assist.pending = None;
                let result = self.with_templates(text, result, opt.language, explicit);
                self.open_popup(text, result, explicit);
            } else {
                // The server has not answered yet.
                ui.ctx().request_repaint_after(Duration::from_millis(30));
            }
        }
        // The documentation of the selected item, when the server has more to say.
        if let Some(popup) = &mut self.assist.popup {
            if let Some((index, answer)) = &popup.resolving
                && let Some(result) = answer.take()
            {
                let index = *index;
                popup.resolving = None;
                match result {
                    Some(updated) => popup.items[index] = updated,
                    None => popup.items[index].raw = None,
                }
            } else if popup.resolving.is_none()
                && let Some((index, _)) = popup.filtered.get(popup.selected)
                && popup.items[*index].raw.is_some()
            {
                let index = *index;
                popup.resolving = Some((
                    index,
                    assist.service.resolve(&assist.doc, &popup.items[index]),
                ));
            }
            if popup.resolving.is_some() {
                ui.ctx().request_repaint_after(Duration::from_millis(30));
            }
        }
        if let Some(answer) = &self.assist.hover.answer
            && let Some(result) = answer.take()
        {
            self.assist.hover.answer = None;
            self.assist.hover.shown = result;
        }
    }

    fn open_popup(&mut self, text: &str, completion: Option<Completion>, explicit: bool) {
        let Some(completion) = completion.filter(|c| !c.items.is_empty()) else {
            if explicit {
                self.assist.popup = None;
            }
            return;
        };
        // The caret may have moved on since the request.
        if self.cursor < completion.from || completion.from > text.len() || self.cursor > text.len()
        {
            return;
        }
        let mut popup = Popup {
            from: completion.from,
            items: completion.items,
            filtered: Vec::new(),
            selected: 0,
            scroll: 0,
            rect: None,
            resolving: None,
        };
        let query = &text[popup.from..self.cursor];
        if query.contains(|c: char| !is_path_char(c)) {
            return;
        }
        refilter(&mut popup, query);
        if popup.filtered.is_empty() {
            return;
        }
        self.assist.popup = Some(popup);
    }

    /// The text changed: keep, update or open the popup. `typed` is the character just typed.
    pub(super) fn assist_after_edit(&mut self, text: &str, typed: Option<char>, erased: bool) {
        self.assist.hover = HoverState::default();
        self.assist.usages = UsageState::default();
        // The signature of a call opens by itself after its parenthesis and its commas.
        if matches!(typed, Some('(' | ',')) {
            self.intel.want = Some(super::intel::Want::Signature);
        }
        if let Some(popup) = &mut self.assist.popup {
            let ok = self.cursor >= popup.from
                && self.cursor <= text.len()
                && text.is_char_boundary(popup.from)
                && !text[popup.from..self.cursor].contains(|c: char| !is_path_char(c));
            if !ok {
                self.assist.popup = None;
            } else {
                let query = &text[popup.from..self.cursor];
                refilter(popup, query);
                if popup.filtered.is_empty() {
                    self.assist.popup = None;
                }
            }
        }
        if erased || self.assist.popup.is_some() {
            return;
        }
        let Some(c) = typed else { return };
        let word_start = word_start_of(text, self.cursor);
        let trigger =
            c == '.' || c == '/' || (is_word(c) && self.cursor - word_start >= 1 && c != ' ');
        if trigger {
            self.assist.want = Some(false);
        }
    }

    /// Keys while the popup is open, and the shortcut that opens it. Some(changed) when the
    /// key was taken.
    pub(super) fn assist_key(&mut self, text: &mut String, key: Key, m: Modifiers) -> Option<bool> {
        if key == Key::Space && m.command {
            self.assist.want = Some(true);
            return Some(false);
        }
        if key == Key::Enter && m.alt {
            self.assist.want_fixes = true;
            return Some(false);
        }
        if key == Key::F6 && m.shift {
            self.request_rename(text);
            return Some(false);
        }
        if let Some(rename) = &self.assist.rename
            && rename.answer.is_none()
            && key == Key::Escape
        {
            self.assist.rename = None;
            return Some(false);
        }
        if key == Key::F12 && m.command {
            self.assist.want_symbols = true;
            return Some(false);
        }
        if key == Key::F2 && !m.command && !m.alt {
            self.next_problem(!m.shift);
            return Some(false);
        }
        if key == Key::F7 && m.alt {
            self.assist_goto_kind(self.cursor, Nav::Usages);
            return Some(false);
        }
        if key == Key::B && m.command {
            let kind = if m.alt {
                Nav::Implementation
            } else if m.shift {
                Nav::Type
            } else {
                Nav::Definition
            };
            self.assist_goto_kind(self.cursor, kind);
            return Some(false);
        }
        if let Some(chooser) = &mut self.assist.chooser {
            let count = chooser.shown.len();
            if count == 0 && key != Key::Backspace {
                self.assist.chooser = None;
                return if key == Key::Escape {
                    Some(false)
                } else {
                    None
                };
            }
            if matches!(key, Key::ArrowRight | Key::ArrowLeft | Key::Tab) && self.intel_chooser_key(key) {
                return Some(false);
            }
            let Some(chooser) = &mut self.assist.chooser else {
                return Some(false);
            };
            match key {
                Key::Backspace if chooser.filter.is_some() => {
                    if let Some(filter) = &mut chooser.filter {
                        filter.pop();
                    }
                    chooser.refilter();
                    return Some(false);
                }
                Key::ArrowDown => chooser.selected = (chooser.selected + 1) % count,
                Key::ArrowUp => chooser.selected = (chooser.selected + count - 1) % count,
                Key::PageDown => chooser.selected = (chooser.selected + ROWS - 1).min(count - 1),
                Key::PageUp => chooser.selected = chooser.selected.saturating_sub(ROWS - 1),
                Key::Enter | Key::Tab => {
                    let index = chooser.selected;
                    return Some(self.choose(text, index));
                }
                _ => {
                    self.assist.chooser = None;
                    return if key == Key::Escape {
                        Some(false)
                    } else {
                        None
                    };
                }
            }
            if chooser.selected < chooser.scroll {
                chooser.scroll = chooser.selected;
            } else if chooser.selected >= chooser.scroll + ROWS {
                chooser.scroll = chooser.selected + 1 - ROWS;
            }
            return Some(false);
        }
        let popup = self.assist.popup.as_mut()?;
        let count = popup.filtered.len();
        match key {
            Key::ArrowDown => {
                popup.selected = (popup.selected + 1) % count;
                scroll_to_selection(popup);
                Some(false)
            }
            Key::ArrowUp => {
                popup.selected = (popup.selected + count - 1) % count;
                scroll_to_selection(popup);
                Some(false)
            }
            Key::PageDown => {
                popup.selected = (popup.selected + ROWS - 1).min(count - 1);
                scroll_to_selection(popup);
                Some(false)
            }
            Key::PageUp => {
                popup.selected = popup.selected.saturating_sub(ROWS - 1);
                scroll_to_selection(popup);
                Some(false)
            }
            Key::Enter | Key::Tab if !m.shift && !m.command => {
                Some(self.accept_completion(text, key == Key::Tab))
            }
            Key::Escape => {
                self.assist.popup = None;
                Some(false)
            }
            Key::ArrowLeft | Key::ArrowRight | Key::Home | Key::End => {
                self.assist.popup = None;
                None
            }
            _ => None,
        }
    }

    /// Inserts the selected item. With `replace_tail` the rest of the word after the caret is
    /// replaced too.
    pub(super) fn accept_completion(&mut self, text: &mut String, replace_tail: bool) -> bool {
        let Some(popup) = self.assist.popup.take() else {
            return false;
        };
        let Some((index, _)) = popup.filtered.get(popup.selected) else {
            return false;
        };
        let item = popup.items[*index].clone();
        let from = popup.from.min(text.len());
        let mut to = self.cursor.min(text.len());
        if replace_tail {
            while let Some(c) = text[to..].chars().next() {
                if !is_word(c) {
                    break;
                }
                to += c.len_utf8();
            }
        }
        // Lines after the first keep the indentation of the current one.
        let line_start = self.lines[self.line_of(from)];
        let indent: String = text[line_start..from]
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let insert = item.insert.replace('\n', &format!("\n{indent}"));
        let caret = item.caret.map(|c| {
            let newlines = item.insert[..c.min(item.insert.len())]
                .matches('\n')
                .count();
            c + newlines * indent.len()
        });
        self.replace(text, from..to, &insert, false);
        if let Some(caret) = caret {
            let pos = self.clamp(text, from + caret);
            self.cursor = pos;
            self.anchor = pos;
        }
        // A folder continues the path.
        if matches!(item.kind, ItemKind::Folder) {
            self.assist.want = Some(true);
        }
        true
    }

    /// Clicks on the popup. True when the click was for it.
    pub(super) fn assist_click(&mut self, text: &mut String, pos: Pos2) -> Option<bool> {
        if let Some(chooser) = &self.assist.chooser {
            let inside = chooser.rect.filter(|r| r.contains(pos));
            let Some(rect) = inside else {
                self.assist.chooser = None;
                return None;
            };
            let row = ((pos.y - rect.top() - 26.0 - 4.0) / ROW_HEIGHT).floor();
            if row >= 0.0 {
                let index = chooser.scroll + row as usize;
                if index < chooser.shown.len() {
                    return Some(self.choose(text, index));
                }
            }
            return Some(false);
        }
        let popup = self.assist.popup.as_mut()?;
        let rect = popup.rect?;
        if !rect.contains(pos) {
            self.assist.popup = None;
            return None;
        }
        let row = ((pos.y - rect.top() - 4.0) / ROW_HEIGHT).floor();
        if row >= 0.0 {
            let index = popup.scroll + row as usize;
            if index < popup.filtered.len() {
                popup.selected = index;
                return Some(self.accept_completion(text, false));
            }
        }
        Some(false)
    }

    /// Does what a shortcut does, for screenshots: `complete`, `structure`, `fixes`, `usages`.
    #[cfg(debug_assertions)]
    pub fn debug_action(&mut self, action: &str) {
        match action {
            "fold" => self.fold_caret(true),
            "structure" => self.assist.want_symbols = true,
            "fixes" => self.assist.want_fixes = true,
            "usages" => self.assist_goto_kind(self.cursor, Nav::Usages),
            "definition" => self.assist_goto_kind(self.cursor, Nav::Definition),
            _ => self.assist.want = Some(true),
        }
    }

    /// Carries out the chosen entry of the chooser. True when the text changed.
    fn choose(&mut self, text: &mut String, index: usize) -> bool {
        let Some(chooser) = self.assist.chooser.take() else {
            return false;
        };
        let Some(&entry_index) = chooser.shown.get(index) else {
            return false;
        };
        let Some(entry) = chooser.entries.into_iter().nth(entry_index) else {
            return false;
        };
        match entry.choice {
            Choice::Go(location) => {
                self.assist.navigate = Some(location);
                false
            }
            Choice::Fix(fix) => {
                self.apply_fix(text, &fix);
                true
            }
            Choice::Dictionary(word) => {
                self.word_added(text, &word);
                false
            }
            Choice::Refactor => {
                self.intel.want = Some(super::intel::Want::Refactor);
                false
            }
            Choice::Surround => {
                let language = self.language.clone();
                self.open_surround(text, &language);
                false
            }
        }
    }

    /// Applies the edits of a fix, the caret staying on the same code.
    fn apply_fix(&mut self, text: &mut String, fix: &QuickFix) {
        self.apply_edits(text, fix.edits.clone());
    }

    /// Applies edits as one change that is undone in one step.
    pub(super) fn apply_edits(&mut self, text: &mut String, mut edits: Vec<TextEdit>) {
        let caret = self.cursor;
        edits.sort_by_key(|e| std::cmp::Reverse(e.range.start));
        let mut new = text.clone();
        let mut shift: isize = 0;
        for edit in edits {
            let end = edit.range.end.min(new.len());
            let start = edit.range.start.min(end);
            if !new.is_char_boundary(start) || !new.is_char_boundary(end) {
                continue;
            }
            if end <= caret {
                shift += edit.new_text.len() as isize - (end - start) as isize;
            }
            new.replace_range(start..end, &edit.new_text);
        }
        if new == *text {
            return;
        }
        let len = text.len();
        self.replace(text, 0..len, &new, false);
        let pos = self.clamp(text, (caret as isize + shift).max(0) as usize);
        self.cursor = pos;
        self.anchor = pos;
    }

    /// Starts Reformat Code: the imports first when asked, then the code.
    pub fn request_format(&mut self, request: FormatRequest) {
        let mut steps = Vec::new();
        if request.organize_imports || request.imports_only {
            steps.push(FormatStep::Imports);
        }
        if !request.imports_only {
            steps.push(FormatStep::Code(request.lines));
        }
        self.assist.format_steps = steps;
        self.assist.format_answer = None;
    }

    /// Asks for the next step of a reformat, and applies its answer.
    fn format_frame(&mut self, ui: &Ui, text: &str, assist: &Assist) {
        if self.assist.format_answer.is_none()
            && self.assist.apply.is_none()
            && !self.assist.format_steps.is_empty()
        {
            let step = self.assist.format_steps.remove(0);
            let answer = match step {
                FormatStep::Imports => assist.service.organize_imports(&assist.doc, text),
                FormatStep::Code(_) => assist.service.format(&assist.doc, text),
            };
            self.assist.format_answer = Some((step, self.rev, answer));
        }
        let Some((step, rev, answer)) = &self.assist.format_answer else {
            return;
        };
        let Some(result) = answer.take() else {
            ui.ctx().request_repaint_after(Duration::from_millis(30));
            return;
        };
        let (step, rev) = (step.clone(), *rev);
        self.assist.format_answer = None;
        // The text changed while the formatter worked: its edits are for another text.
        if rev != self.rev {
            self.assist.format_steps.clear();
            return;
        }
        match result {
            Ok(edits) => {
                let edits = match &step {
                    FormatStep::Code(Some(lines)) => {
                        crate::languages::format::within_lines(edits, text, lines)
                    }
                    _ => edits,
                };
                if let FormatStep::Code(_) = step {
                    let count = crate::languages::format::changed_lines(text, &edits);
                    self.assist.message = Some(if edits.is_empty() {
                        tr!("no_lines_changed_content_already").to_string()
                    } else {
                        trf!("reformatted_line_s", count = count)
                    });
                }
                if !edits.is_empty() {
                    self.assist.apply = Some(edits);
                    ui.ctx().request_repaint();
                }
            }
            Err(error) => {
                self.assist.format_steps.clear();
                self.assist.message = Some(format!("{}{error}", tr!("error_prefix")));
            }
        }
    }

    /// Starts renaming the name at the caret (Shift+F6).
    pub fn request_rename(&mut self, text: &str) {
        let word = self.word_at(text, self.cursor);
        if word.is_empty() {
            return;
        }
        self.assist.popup = None;
        self.assist.chooser = None;
        self.assist.rename = Some(Rename {
            name: text[word.clone()].to_string(),
            word,
            focus: true,
            submit: false,
            error: None,
            answer: None,
        });
    }

    pub(super) fn popup_rect(&self) -> Option<Rect> {
        self.assist
            .popup
            .as_ref()
            .and_then(|p| p.rect)
            .or_else(|| self.assist.chooser.as_ref().and_then(|c| c.rect))
    }

    /// Starts a jump to the declaration of the word at `offset`; with nothing to go to, to
    /// its usages.
    pub(super) fn assist_goto(&mut self, offset: usize) {
        self.assist_goto_kind(offset, Nav::Definition);
    }

    pub(super) fn assist_goto_kind(&mut self, offset: usize, kind: Nav) {
        self.assist.popup = None;
        self.assist.chooser = None;
        self.assist.goto_pending = Some((offset, kind));
    }

    /// Asks for the places of a jump started, and acts on the answers.
    fn poll_goto(&mut self, ui: &Ui, text: &str, opt: &Options) {
        let Some(assist) = &opt.assist else { return };
        if let Some((offset, kind)) = self.assist.goto_pending.take() {
            let offset = offset.min(text.len());
            let word = self.word_at(text, offset);
            if word.is_empty() {
                return;
            }
            let usages = kind == Nav::Usages;
            let answer = match kind {
                Nav::Definition => assist.service.definition(&assist.doc, text, offset),
                Nav::Type => assist.service.type_definition(&assist.doc, text, offset),
                Nav::Implementation => assist.service.implementation(&assist.doc, text, offset),
                Nav::Usages => assist.service.references(&assist.doc, text, offset),
            };
            self.assist.job = Some(Job {
                word,
                offset,
                kind,
                usages,
                answer,
            });
        }
        let Some(job) = &self.assist.job else { return };
        let Some(places) = job.answer.take() else {
            ui.ctx().request_repaint_after(Duration::from_millis(30));
            return;
        };
        let (word, offset, usages, kind) = (job.word.clone(), job.offset, job.usages, job.kind);
        self.assist.job = None;
        // The place the jump started from is not a destination.
        let places: Vec<Location> = places
            .into_iter()
            .filter(|l| {
                !(l.path.is_none() && {
                    let range = l.range_in(text);
                    range.start < word.end && word.start < range.end.max(range.start + 1)
                })
            })
            .collect();
        if places.is_empty() {
            if kind == Nav::Definition && !usages {
                // On a declaration: where it is used.
                self.assist.job = Some(Job {
                    word,
                    offset,
                    kind,
                    usages: true,
                    answer: assist.service.references(&assist.doc, text, offset),
                });
            }
            return;
        }
        // The usages are always listed, as IntelliJ does; one declaration is simply gone to.
        if places.len() == 1 {
            self.assist.navigate = places.into_iter().next();
        } else {
            let title = match (kind, usages) {
                (_, true) | (Nav::Usages, _) => tr!("usages"),
                (Nav::Type, _) => tr!("types"),
                (Nav::Implementation, _) => tr!("implementations"),
                _ => tr!("declarations"),
            };
            self.assist.chooser = Some(Chooser::new(title, place_entries(places), false));
        }
    }

    /// The structure of the file, as a list to filter by typing.
    fn open_structure(&mut self, text: &str, symbols: Vec<Symbol>) {
        if symbols.is_empty() {
            return;
        }
        // The symbol the caret is in comes pre-selected: the last one that contains it.
        let mut current = 0;
        for (i, s) in symbols.iter().enumerate() {
            if s.range.start <= self.cursor {
                current = i;
            }
        }
        let entries = symbols
            .into_iter()
            .map(|s| Entry {
                label: s.name.clone(),
                detail: s.detail.clone(),
                kind: Some(s.kind),
                indent: s.depth,
                choice: Choice::Go(Location::in_text(None, text, s.range)),
            })
            .collect();
        let mut chooser = Chooser::new(tr!("file_structure"), entries, false);
        chooser.filter = Some(String::new());
        chooser.centered = true;
        chooser.selected = current;
        chooser.scroll = current.saturating_sub(ROWS / 2);
        self.assist.chooser = Some(chooser);
    }

    /// Typing while a list is open filters it, when it can be.
    pub(super) fn assist_text(&mut self, typed: &str) -> bool {
        let Some(chooser) = &mut self.assist.chooser else {
            return false;
        };
        let Some(filter) = &mut chooser.filter else {
            return false;
        };
        filter.push_str(typed);
        chooser.refilter();
        true
    }

    /// Follows the name under the caret to highlight where else it appears.
    fn usage_frame(&mut self, ui: &Ui, text: &str, opt: &Options) {
        let Some(assist) = &opt.assist else { return };
        let word = {
            let w = self.word_at(text, self.cursor);
            let starts_with_digit = text[w.clone()]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit());
            (self.selection().is_empty() && !w.is_empty() && !starts_with_digit).then_some(w)
        };
        let rev = self.rev;
        let cursor = self.cursor;
        let state = &mut self.assist.usages;
        if state.word != word || state.rev != rev {
            *state = UsageState {
                word,
                rev,
                since: Some(Instant::now()),
                ..UsageState::default()
            };
        }
        if state.word.is_none() {
            return;
        }
        if let Some(answer) = &state.answer {
            if let Some(ranges) = answer.take() {
                state.ranges = ranges;
                state.answer = None;
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(30));
            }
        }
        if !state.requested {
            if state
                .since
                .is_some_and(|s| s.elapsed() >= Duration::from_millis(150))
            {
                state.requested = true;
                state.answer = Some(assist.service.highlights(&assist.doc, text, cursor));
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(150));
            }
        }
    }

    /// Background of the two brackets of the pair at the caret.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn bracket_highlights(
        &self,
        painter: &Painter,
        text: &str,
        line: Range<usize>,
        galley: &egui::Galley,
        text_left: f32,
        y: f32,
        row: f32,
        color: Color32,
        pair: Option<(usize, usize)>,
    ) {
        let Some((a, b)) = pair else { return };
        for at in [a, b] {
            if at < line.start || at >= line.end {
                continue;
            }
            let text_line = &text[line.clone()];
            let from = column_of(text_line, at - line.start);
            let to = column_of(text_line, at - line.start + 1);
            let x0 = text_left + galley
                .pos_from_cursor(CCursor::new(self.visual_column(
                    text,
                    self.line_of(line.start),
                    from,
                ))).min.x;
            let x1 = text_left + galley
                .pos_from_cursor(CCursor::new(self.visual_column(
                    text,
                    self.line_of(line.start),
                    to,
                ))).min.x;
            painter.rect_filled(
                Rect::from_min_max(pos2(x0, y + 1.0), pos2(x1, y + row - 1.0)),
                2.0,
                color,
            );
        }
    }

    /// Background of the given sorted ranges (the matches of the find bar).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn range_highlights(
        &self,
        painter: &Painter,
        text: &str,
        line: Range<usize>,
        galley: &egui::Galley,
        text_left: f32,
        y: f32,
        row: f32,
        color: Color32,
        ranges: &[Range<usize>],
    ) {
        let first = ranges.partition_point(|r| r.end < line.start);
        for range in ranges[first..].iter().take_while(|r| r.start <= line.end) {
            if range.is_empty() {
                continue;
            }
            let text_line = &text[line.clone()];
            let from = column_of(text_line, range.start.max(line.start) - line.start);
            let to = column_of(text_line, range.end.min(line.end) - line.start);
            let x0 = text_left + galley
                .pos_from_cursor(CCursor::new(self.visual_column(
                    text,
                    self.line_of(line.start),
                    from,
                ))).min.x;
            let x1 = text_left + galley
                .pos_from_cursor(CCursor::new(self.visual_column(
                    text,
                    self.line_of(line.start),
                    to,
                ))).min.x;
            painter.rect_filled(
                Rect::from_min_max(pos2(x0 - 1.0, y + 1.0), pos2(x1 + 1.0, y + row - 1.0)),
                2.0,
                color,
            );
        }
    }

    /// Background of the places where the name under the caret appears.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn usage_highlights(
        &self,
        painter: &Painter,
        text: &str,
        line: Range<usize>,
        galley: &egui::Galley,
        text_left: f32,
        y: f32,
        row: f32,
        color: Color32,
    ) {
        for range in &self.assist.usages.ranges {
            if range.end < line.start || range.start > line.end || range.is_empty() {
                continue;
            }
            let text_line = &text[line.clone()];
            let from = column_of(text_line, range.start.max(line.start) - line.start);
            let to = column_of(text_line, range.end.min(line.end) - line.start);
            let x0 = text_left + galley
                .pos_from_cursor(CCursor::new(self.visual_column(
                    text,
                    self.line_of(line.start),
                    from,
                ))).min.x;
            let x1 = text_left + galley
                .pos_from_cursor(CCursor::new(self.visual_column(
                    text,
                    self.line_of(line.start),
                    to,
                ))).min.x;
            painter.rect_filled(
                Rect::from_min_max(pos2(x0 - 1.0, y + 1.0), pos2(x1 + 1.0, y + row - 1.0)),
                2.0,
                color,
            );
        }
    }

    /// Underlines the word that Ctrl+click would follow.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn link_underline(
        &self,
        painter: &Painter,
        text: &str,
        line: Range<usize>,
        galley: &egui::Galley,
        text_left: f32,
        y: f32,
        row: f32,
        color: Color32,
    ) {
        let hover = &self.assist.goto_hover;
        let Some(word) = hover.word.as_ref().filter(|_| hover.link) else {
            return;
        };
        if word.start < line.start || word.end > line.end {
            return;
        }
        let text_line = &text[line.clone()];
        let from = column_of(text_line, word.start - line.start);
        let to = column_of(text_line, word.end - line.start);
        let x0 = text_left + galley
                .pos_from_cursor(CCursor::new(self.visual_column(
                    text,
                    self.line_of(line.start),
                    from,
                ))).min.x;
        let x1 = text_left + galley
                .pos_from_cursor(CCursor::new(self.visual_column(
                    text,
                    self.line_of(line.start),
                    to,
                ))).min.x;
        painter.line_segment(
            [pos2(x0, y + row - 3.0), pos2(x1, y + row - 3.0)],
            Stroke::new(1.2_f32, color),
        );
    }

    /// Wavy underline of the problems of one line, under the glyphs of `galley`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn squiggles(
        &self,
        painter: &Painter,
        text: &str,
        line: Range<usize>,
        galley: &egui::Galley,
        text_left: f32,
        y: f32,
        row: f32,
        char_width: f32,
    ) {
        for diagnostic in self.assist.diagnostics.iter() {
            let range = &diagnostic.range;
            if range.start > line.end || range.end < line.start {
                continue;
            }
            let start = range.start.max(line.start);
            let end = range.end.min(line.end).max(start);
            let text_line = &text[line.clone()];
            let from = column_of(text_line, start - line.start);
            let to = column_of(text_line, end - line.start);
            let x0 = text_left + galley
                .pos_from_cursor(CCursor::new(self.visual_column(
                    text,
                    self.line_of(line.start),
                    from,
                ))).min.x;
            let mut x1 = text_left + galley
                .pos_from_cursor(CCursor::new(self.visual_column(
                    text,
                    self.line_of(line.start),
                    to,
                ))).min.x;
            if x1 - x0 < char_width * 0.5 {
                x1 = x0 + char_width;
            }
            // Hints (unused names…) are for the tooltip only.
            if diagnostic.severity != Severity::Hint {
                wave(
                    painter,
                    x0,
                    x1,
                    y + row - 3.0,
                    severity_color(diagnostic.severity),
                );
            }
        }
        // The misspelled words, in green.
        let first = self.spell.typos.partition_point(|r| r.end < line.start);
        for typo in self.spell.typos[first..]
            .iter()
            .take_while(|r| r.start <= line.end)
            .filter(|r| line.start <= r.start && r.end <= line.end)
        {
            let text_line = &text[line.clone()];
            let from = column_of(text_line, typo.start - line.start);
            let to = column_of(text_line, typo.end - line.start);
            let x0 = text_left
                + galley
                    .pos_from_cursor(CCursor::new(self.visual_column(
                        text,
                        self.line_of(line.start),
                        from,
                    )))
                    .min
                    .x;
            let x1 = text_left
                + galley
                    .pos_from_cursor(CCursor::new(self.visual_column(
                        text,
                        self.line_of(line.start),
                        to,
                    )))
                    .min
                    .x;
            wave(painter, x0, x1, y + row - 3.0, Color32::from_rgb(0x62, 0xB5, 0x43));
        }
    }

    /// Problems under a byte offset.
    /// F2 / Shift+F2: the caret to the next or previous problem of the file, the errors first
    /// when there are, round the end; its message goes to the status bar.
    pub(super) fn next_problem(&mut self, forward: bool) {
        let errors = self
            .assist
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error);
        let mut problems: Vec<&Diagnostic> = self
            .assist
            .diagnostics
            .iter()
            .filter(|d| !errors || d.severity == Severity::Error)
            .filter(|d| matches!(d.severity, Severity::Error | Severity::Warning))
            .collect();
        problems.sort_by_key(|d| d.range.start);
        problems.dedup_by_key(|d| d.range.start);
        let at = self.cursor;
        let found = if forward {
            problems
                .iter()
                .find(|d| d.range.start > at)
                .or(problems.first())
        } else {
            problems
                .iter()
                .rev()
                .find(|d| d.range.start < at)
                .or(problems.last())
        };
        let Some(problem) = found else {
            self.assist.message = Some(tr!("no_problems_file").into());
            return;
        };
        let (start, message) = (problem.range.start, problem.message.clone());
        self.select(start, start);
        self.assist.message = Some(message);
    }

    fn diagnostics_at(&self, offset: usize) -> Vec<&Diagnostic> {
        self.assist
            .diagnostics
            .iter()
            .filter(|d| d.range.start <= offset && offset < d.range.end.max(d.range.start + 1))
            .collect()
    }

    /// Follows the pointer: after a rest over a word, asks what it is. `offset` is the byte
    /// under the pointer, None when the pointer is not over code.
    pub(super) fn assist_hover(
        &mut self,
        ui: &Ui,
        text: &str,
        opt: &Options,
        offset: Option<usize>,
        ctrl: bool,
    ) {
        let Some(assist) = &opt.assist else { return };
        // With Ctrl held, the word under the pointer is a link when it leads somewhere.
        let word_here = offset
            .map(|o| (o, self.word_at(text, o)))
            .filter(|(_, w)| ctrl && !w.is_empty());
        match word_here {
            Some((offset, word)) => {
                if self.assist.goto_hover.word.as_ref() != Some(&word) {
                    self.assist.goto_hover = GotoHover {
                        word: Some(word.clone()),
                        answer: Some(assist.service.definition(&assist.doc, text, offset)),
                        link: false,
                        places: Vec::new(),
                    };
                }
                if let Some(answer) = &self.assist.goto_hover.answer {
                    if let Some(places) = answer.take() {
                        let elsewhere: Vec<Location> = places
                            .into_iter()
                            .filter(|l| {
                                l.path.is_some() || {
                                    let range = l.range_in(text);
                                    !(range.start < word.end
                                        && word.start < range.end.max(range.start + 1))
                                }
                            })
                            .collect();
                        self.assist.goto_hover.link = !elsewhere.is_empty();
                        self.assist.goto_hover.places = elsewhere;
                        self.assist.goto_hover.answer = None;
                    } else {
                        ui.ctx().request_repaint_after(Duration::from_millis(30));
                    }
                }
                if self.assist.goto_hover.link {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                self.assist.hover = HoverState::default();
                return;
            }
            None => self.assist.goto_hover = GotoHover::default(),
        }
        let Some(offset) = offset else {
            self.assist.hover = HoverState::default();
            return;
        };
        let word = self.word_at(text, offset);
        let on_word = !word.is_empty() || !self.diagnostics_at(offset).is_empty();
        let same = self.assist.hover.word.as_ref().is_some_and(|w| *w == word);
        if !same || !on_word {
            self.assist.hover = HoverState {
                word: on_word.then_some(word),
                since: Some(Instant::now()),
                ..HoverState::default()
            };
        }
        let state = &mut self.assist.hover;
        if !on_word {
            return;
        }
        match state.since {
            Some(since) if !state.requested && since.elapsed() >= HOVER_DELAY => {
                state.requested = true;
                state.answer = Some(assist.service.hover(&assist.doc, text, offset));
            }
            Some(_) if !state.requested => {
                ui.ctx().request_repaint_after(HOVER_DELAY);
            }
            _ => {}
        }
    }

    /// Marks of the problems and of the Git changes of the document along the right edge of
    /// `rect`. Returns the line that was clicked.
    pub(super) fn problem_stripe(
        &self,
        ui: &mut Ui,
        rect: Rect,
        id: egui::Id,
        row: f32,
        changes: &[LineMark],
    ) -> Option<usize> {
        // A file shorter than the window keeps its marks at the place of their lines.
        let lines = self
            .lines
            .len()
            .max((rect.height() / row.max(1.0)) as usize)
            .max(1);
        let strip = Rect::from_min_max(pos2(rect.right() - 9.0, rect.top()), rect.right_bottom());
        let problems = || {
            self.assist
                .diagnostics
                .iter()
                .filter(|d| d.severity != Severity::Hint)
        };
        if changes.is_empty() && problems().next().is_none() {
            return None;
        }
        let y_of = |line: f32| strip.top() + line / lines as f32 * strip.height();
        let painter = ui.painter();
        // Git changes on the left of the stripe, a bar per block of consecutive changed lines,
        // under the problems.
        let mut blocks: Vec<(usize, usize, MarkKind)> = Vec::new();
        for mark in changes {
            match blocks.last_mut() {
                Some((_, end, kind)) if *end == mark.line && *kind == mark.kind => *end += 1,
                _ => blocks.push((mark.line, mark.line + 1, mark.kind)),
            }
        }
        for (start, end, kind) in blocks {
            let (top, bottom) = if kind == MarkKind::Deleted {
                (y_of(start as f32) - 1.0, y_of(start as f32) + 1.0)
            } else {
                let (top, bottom) = (y_of(start as f32), y_of(end as f32));
                // At least 3 points high to stay visible in a long file.
                let grow = ((3.0 - (bottom - top)) / 2.0).max(0.0);
                (top - grow, bottom + grow)
            };
            painter.rect_filled(
                Rect::from_x_y_ranges(strip.left()..=strip.left() + 3.0, top..=bottom),
                0.0,
                kind.color(),
            );
        }
        // Errors are drawn last, on top of warnings.
        let mut marks: Vec<(usize, Severity)> = problems()
            .map(|d| (self.line_of(d.range.start), d.severity))
            .collect();
        marks.sort_by_key(|(_, s)| std::cmp::Reverse(*s));
        for (line, severity) in marks {
            let y = y_of(line as f32 + 0.5);
            let color = severity_color(severity);
            painter.rect_filled(
                Rect::from_center_size(pos2(strip.center().x + 1.0, y), vec2(6.0, 3.0)),
                1.0,
                color,
            );
        }
        let response = ui.interact(strip, id.with("problem_stripe"), egui::Sense::click());
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            // The mark nearest to the click (a problem, else a change) within a few points, or
            // the line at that height.
            let height = (pos.y - strip.top()) / strip.height().max(1.0);
            let distance = |l: &usize| (y_of(*l as f32 + 0.5) - pos.y).abs();
            let nearest = |lines: Vec<usize>| {
                lines
                    .into_iter()
                    .filter(|l| distance(l) < 6.0)
                    .min_by(|a, b| distance(a).total_cmp(&distance(b)))
            };
            let nearest = nearest(problems().map(|d| self.line_of(d.range.start)).collect())
                .or_else(|| nearest(changes.iter().map(|m| m.line).collect()));
            return nearest.or(Some((height * lines as f32) as usize));
        }
        None
    }

    /// The mouse over a list: the wheel scrolls it and the row under the pointer is selected.
    pub(super) fn assist_list_mouse(&mut self, ui: &Ui, pointer: Option<Pos2>) {
        let Some(pos) = pointer else { return };
        let wheel = ui.input(|i| i.smooth_scroll_delta.y + i.raw_scroll_delta.y);
        if let Some(chooser) = &mut self.assist.chooser
            && let Some(rect) = chooser.rect.filter(|r| r.contains(pos))
        {
            if wheel.abs() > 0.5 {
                scroll_chooser(chooser, (wheel / ROW_HEIGHT).clamp(-3.0, 3.0));
                ui.ctx().input_mut(|i| {
                    i.smooth_scroll_delta = egui::Vec2::ZERO;
                    i.raw_scroll_delta = egui::Vec2::ZERO;
                });
            }
            if ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                let row = ((pos.y - rect.top() - 26.0 - 4.0) / ROW_HEIGHT).floor();
                if row >= 0.0 && chooser.scroll + (row as usize) < chooser.shown.len() {
                    chooser.selected = chooser.scroll + row as usize;
                }
            }
            return;
        }
        if let Some(popup) = &mut self.assist.popup
            && popup.rect.is_some_and(|r| r.contains(pos))
            && wheel.abs() > 0.5
        {
            let max = popup.filtered.len().saturating_sub(ROWS);
            let step = (wheel.abs() / ROW_HEIGHT).ceil().clamp(1.0, 3.0) as usize;
            popup.scroll = if wheel < 0.0 {
                (popup.scroll + step).min(max)
            } else {
                popup.scroll.saturating_sub(step)
            };
            ui.ctx().input_mut(|i| {
                i.smooth_scroll_delta = egui::Vec2::ZERO;
                i.raw_scroll_delta = egui::Vec2::ZERO;
            });
        }
    }

    /// The field where the new name is typed.
    fn rename_popup(&mut self, ui: &Ui, t: &Theme, at: Pos2, occurrences: usize) {
        let Some(rename) = &mut self.assist.rename else {
            return;
        };
        let busy = rename.answer.is_some();
        let mut cancel = false;
        egui::Area::new(egui::Id::new("rename_popup"))
            .order(Order::Foreground)
            .fixed_pos(pos2(at.x, at.y + 4.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(t.popup)
                    .stroke(Stroke::new(1.0_f32, t.border))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width(320.0);
                        ui.label(
                            RichText::new(tr!("rename"))
                                .font(crate::ui::semibold(12.5))
                                .color(t.text_secondary),
                        );
                        let id = egui::Id::new("rename_field");
                        let mut output = egui::TextEdit::singleline(&mut rename.name)
                            .id(id)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .interactive(!busy)
                            .show(ui);
                        if rename.focus {
                            rename.focus = false;
                            output.response.request_focus();
                            let end = egui::text::CCursor::new(rename.name.chars().count());
                            output.state.cursor.set_char_range(Some(
                                egui::text::CCursorRange::two(egui::text::CCursor::new(0), end),
                            ));
                            output.state.store(ui.ctx(), id);
                        }
                        // A single-line field gives up the focus on Enter, in the same frame.
                        let enter = ui.input(|i| i.key_pressed(Key::Enter));
                        let focused = output.response.has_focus() || output.response.lost_focus();
                        if focused && enter && !busy {
                            rename.submit = true;
                        }
                        if ui.input(|i| i.key_pressed(Key::Escape)) {
                            cancel = true;
                        }
                        let hint = if busy {
                            tr!("renaming").to_string()
                        } else if occurrences > 1 {
                            trf!(
                                "occurrences_file_files_import_enter",
                                occurrences = occurrences
                            )
                        } else {
                            tr!("enter_apply_esc_cancel").to_string()
                        };
                        ui.label(RichText::new(hint).small().color(t.text_secondary));
                        if let Some(error) = &rename.error {
                            ui.label(RichText::new(error).small().color(t.error));
                        }
                    });
            });
        if cancel {
            self.assist.rename = None;
        }
    }

    /// Draws the popup and the tooltip above everything else.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn assist_paint(
        &mut self,
        ui: &Ui,
        text: &str,
        opt: &Options,
        origin: Pos2,
        text_left: f32,
        row: f32,
        pointer: Option<Pos2>,
    ) {
        let t = opt.theme;
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            Order::Foreground,
            egui::Id::new("editor_assist"),
        ));
        let screen = ui.ctx().content_rect();
        let line = self.line_of(self.cursor);
        let caret_y = origin.y + (self.row_of(line) + 1) as f32 * row;
        let galley = |s: &mut Self, offset: usize| -> f32 {
            let range = s.line_range(text, s.line_of(offset));
            let column = column_of(
                &text[range.clone()],
                offset.saturating_sub(range.start).min(range.len()),
            );
            let g = s.galley(ui, text, s.line_of(offset), opt);
            g.pos_from_cursor(CCursor::new(s.visual_column(text, s.line_of(offset), column)))
                .min
                .x
        };
        if self.assist.rename.is_some() {
            let x = text_left + galley(self, self.cursor);
            let occurrences = self.assist.usages.ranges.len();
            self.rename_popup(ui, &t, pos2(x, caret_y), occurrences);
            return;
        }
        if self.assist.chooser.is_some() {
            let caret_x = text_left + galley(self, self.cursor);
            if let Some(chooser) = &mut self.assist.chooser {
                if chooser.anchor == Pos2::ZERO {
                    chooser.anchor = if chooser.centered {
                        pos2(ui.clip_rect().left() + 120.0, ui.clip_rect().top() + 24.0)
                    } else if chooser.at_caret {
                        pos2(caret_x, caret_y)
                    } else {
                        pointer.unwrap_or(pos2(text_left, caret_y))
                    };
                }
                chooser.rect = Some(draw_chooser(&painter, chooser, &t, chooser.anchor, screen));
            }
            return;
        }
        if let Some(from) = self.assist.popup.as_ref().map(|p| p.from) {
            let x = text_left + galley(self, from.min(self.cursor));
            if let Some(popup) = &mut self.assist.popup {
                popup.rect = Some(draw_popup(
                    &painter,
                    popup,
                    &t,
                    &opt.font,
                    pos2(x, caret_y),
                    row,
                    screen,
                ));
            }
            return;
        }
        // Ctrl held over a link: the declaration it leads to, as IntelliJ's quick definition.
        if self.assist.goto_hover.link
            && let (Some(pointer), Some(place)) = (pointer, self.assist.goto_hover.places.first())
        {
            let source = match &place.path {
                Some(path) => format!(
                    "{}:{}",
                    path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    place.line + 1
                ),
                None => trf!("line", line = place.line + 1),
            };
            let hover = Hover {
                range: 0..0,
                title: place.preview.clone(),
                body: source,
            };
            draw_hover(&painter, &t, &opt.font, &[], Some(&hover), pointer, screen);
            return;
        }
        let hover = &self.assist.hover;
        let (Some(pointer), Some(since)) = (pointer, hover.since) else {
            return;
        };
        if !hover.requested || since.elapsed() < HOVER_DELAY {
            return;
        }
        let offset = hover.word.as_ref().map_or(0, |w| w.start);
        let mut problems: Vec<(Severity, String)> = self
            .diagnostics_at(offset)
            .into_iter()
            .map(|d| (d.severity, d.message.clone()))
            .collect();
        if let Some(typo) = self.typo_at(offset) {
            problems.push((
                Severity::Hint,
                trf!("spelling_typo", word = text.get(typo).unwrap_or_default()),
            ));
        }
        if problems.is_empty() && hover.shown.is_none() {
            return;
        }
        draw_hover(
            &painter,
            &t,
            &opt.font,
            &problems,
            hover.shown.as_ref(),
            pointer,
            screen,
        );
    }
}

fn word_start_of(text: &str, pos: usize) -> usize {
    let mut start = pos;
    for (i, c) in text[..pos].char_indices().rev() {
        if !is_word(c) {
            break;
        }
        start = i;
    }
    start
}

fn is_path_char(c: char) -> bool {
    is_word(c) || matches!(c, '$' | '-' | '.' | '@' | '/')
}

fn scroll_to_selection(popup: &mut Popup) {
    if popup.selected < popup.scroll {
        popup.scroll = popup.selected;
    } else if popup.selected >= popup.scroll + ROWS {
        popup.scroll = popup.selected + 1 - ROWS;
    }
}

/// Keeps the items that match `query`, best first.
fn refilter(popup: &mut Popup, query: &str) {
    let mut scored: Vec<(i32, u32, usize, Vec<usize>)> = popup
        .items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| {
            let (score, positions) = fuzzy_match(query, &item.label)?;
            Some((score, item.rank, i, positions))
        })
        .collect();
    // A better match first; among equals the lower rank, then the label.
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then_with(|| {
            popup.items[a.2]
                .label
                .to_lowercase()
                .cmp(&popup.items[b.2].label.to_lowercase())
        })
    });
    popup.filtered = scored.into_iter().map(|(_, _, i, p)| (i, p)).collect();
    popup.selected = 0;
    popup.scroll = 0;
}

fn wave(painter: &Painter, x0: f32, x1: f32, y: f32, color: Color32) {
    let mut points = Vec::new();
    let mut x = x0;
    let mut up = false;
    while x < x1 {
        points.push(pos2(x, y + if up { -1.5 } else { 1.0 }));
        x += 2.0;
        up = !up;
    }
    points.push(pos2(x1, y + if up { -1.5 } else { 1.0 }));
    painter.add(egui::Shape::line(points, Stroke::new(1.2_f32, color)));
}

fn draw_popup(
    painter: &Painter,
    popup: &Popup,
    t: &Theme,
    font: &FontId,
    anchor: Pos2,
    row: f32,
    screen: Rect,
) -> Rect {
    let rows = popup.filtered.len().min(ROWS);
    let doc = popup
        .filtered
        .get(popup.selected)
        .and_then(|(i, _)| popup.items[*i].doc.as_deref())
        .filter(|d| !d.is_empty());
    let doc_galley = doc.map(|d| {
        painter.layout(
            d.to_string(),
            FontId::proportional(12.5),
            t.text_secondary,
            POPUP_WIDTH - 20.0,
        )
    });
    let doc_height = doc_galley
        .as_ref()
        .map_or(0.0, |g| g.size().y.min(90.0) + 14.0);
    let list_height = rows as f32 * ROW_HEIGHT + 8.0;
    let height = list_height + doc_height;
    let width = POPUP_WIDTH.min(screen.width() - 16.0);
    let mut x = anchor
        .x
        .min(screen.right() - width - 8.0)
        .max(screen.left() + 8.0);
    let below = screen.bottom() - anchor.y - 8.0;
    let y = if below >= height || below >= anchor.y - row - screen.top() {
        anchor.y + 2.0
    } else {
        anchor.y - row - height - 2.0
    };
    x = x.max(0.0);
    let rect = Rect::from_min_size(pos2(x, y), vec2(width, height));
    painter.rect(
        rect,
        5.0,
        t.popup,
        Stroke::new(1.0_f32, t.border),
        StrokeKind::Inside,
    );
    let mono = FontId::proportional(font.size.clamp(12.0, 14.0));
    for (visible, (index, positions)) in popup
        .filtered
        .iter()
        .skip(popup.scroll)
        .take(rows)
        .enumerate()
    {
        let item = &popup.items[*index];
        let top = rect.top() + 4.0 + visible as f32 * ROW_HEIGHT;
        let row_rect =
            Rect::from_min_size(pos2(rect.left() + 3.0, top), vec2(width - 6.0, ROW_HEIGHT));
        if popup.scroll + visible == popup.selected {
            painter.rect_filled(row_rect, 4.0, t.selection);
        }
        let (letter, color) = kind_style(item.kind);
        let badge = Rect::from_center_size(
            pos2(row_rect.left() + 14.0, row_rect.center().y),
            vec2(15.0, 15.0),
        );
        painter.rect_filled(badge, 4.0, color.gamma_multiply(0.22));
        painter.text(
            badge.center(),
            Align2::CENTER_CENTER,
            letter,
            FontId::monospace(11.0),
            color,
        );
        // The label, with the characters of the match in the accent color.
        let mut x = row_rect.left() + 29.0;
        let mut run = String::new();
        let mut matched_run = false;
        let flush = |x: &mut f32, run: &mut String, matched: bool| {
            if run.is_empty() {
                return;
            }
            let galley = painter.layout_no_wrap(
                std::mem::take(run),
                mono.clone(),
                if matched { t.accent_hover } else { t.text },
            );
            let at = pos2(*x, row_rect.center().y - galley.size().y / 2.0);
            let w = galley.size().x;
            painter.galley(at, galley, t.text);
            *x += w;
        };
        for (i, c) in item.label.chars().enumerate() {
            let is_match = positions.contains(&i);
            if is_match != matched_run {
                flush(&mut x, &mut run, matched_run);
                matched_run = is_match;
            }
            run.push(c);
        }
        flush(&mut x, &mut run, matched_run);
        if let Some(detail) = &item.detail {
            let galley = painter.layout_no_wrap(
                detail.clone(),
                FontId::proportional(12.0),
                t.text_secondary,
            );
            let max = (row_rect.right() - 10.0 - (x + 12.0)).max(0.0);
            if max > 30.0 {
                let w = galley.size().x.min(max);
                let clip = Rect::from_min_size(
                    pos2(row_rect.right() - 8.0 - w, row_rect.top()),
                    vec2(w, ROW_HEIGHT),
                );
                painter.with_clip_rect(clip).galley(
                    pos2(clip.left(), row_rect.center().y - galley.size().y / 2.0),
                    galley,
                    t.text_secondary,
                );
            }
        }
    }
    if popup.filtered.len() > rows {
        // A thin scroll bar at the right edge.
        let track = Rect::from_min_size(
            pos2(rect.right() - 5.0, rect.top() + 4.0),
            vec2(3.0, rows as f32 * ROW_HEIGHT),
        );
        let share = rows as f32 / popup.filtered.len() as f32;
        let offset = popup.scroll as f32 / popup.filtered.len() as f32;
        painter.rect_filled(
            Rect::from_min_size(
                pos2(track.left(), track.top() + offset * track.height()),
                vec2(3.0, (share * track.height()).max(14.0)),
            ),
            1.5,
            t.border,
        );
    }
    if let Some(galley) = doc_galley {
        let sep_y = rect.top() + list_height;
        painter.line_segment(
            [
                pos2(rect.left() + 1.0, sep_y),
                pos2(rect.right() - 1.0, sep_y),
            ],
            Stroke::new(1.0_f32, t.separator),
        );
        painter
            .with_clip_rect(Rect::from_min_size(
                pos2(rect.left(), sep_y),
                vec2(width, doc_height - 2.0),
            ))
            .galley(
                pos2(rect.left() + 10.0, sep_y + 7.0),
                galley,
                t.text_secondary,
            );
    }
    rect
}

fn draw_chooser(
    painter: &Painter,
    chooser: &Chooser,
    t: &Theme,
    anchor: Pos2,
    screen: Rect,
) -> Rect {
    let rows = chooser.shown.len().clamp(1, ROWS);
    let title_height = 26.0;
    let height = rows as f32 * ROW_HEIGHT + 8.0 + title_height;
    let width = 560.0_f32.min(screen.width() - 16.0);
    let x = anchor
        .x
        .min(screen.right() - width - 8.0)
        .max(screen.left() + 8.0);
    let y = if screen.bottom() - anchor.y - 8.0 >= height {
        anchor.y + 6.0
    } else {
        (anchor.y - height - 6.0).max(screen.top())
    };
    let rect = Rect::from_min_size(pos2(x, y), vec2(width, height));
    painter.rect(
        rect,
        5.0,
        t.popup,
        Stroke::new(1.0_f32, t.border),
        StrokeKind::Inside,
    );
    painter.text(
        pos2(rect.left() + 12.0, rect.top() + 14.0),
        Align2::LEFT_CENTER,
        match &chooser.filter {
            Some(filter) => format!("{}   {filter}|", chooser.title),
            None => format!("{} ({})", chooser.title, chooser.entries.len()),
        },
        crate::ui::semibold(12.5),
        t.text_secondary,
    );
    if chooser.shown.is_empty() {
        painter.text(
            pos2(
                rect.left() + 12.0,
                rect.top() + title_height + 4.0 + ROW_HEIGHT / 2.0,
            ),
            Align2::LEFT_CENTER,
            tr!("no_results"),
            FontId::proportional(13.0),
            t.text_secondary,
        );
    }
    let list_top = rect.top() + title_height;
    painter.line_segment(
        [
            pos2(rect.left() + 1.0, list_top - 2.0),
            pos2(rect.right() - 1.0, list_top - 2.0),
        ],
        Stroke::new(1.0_f32, t.separator),
    );
    for (visible, entry) in chooser
        .shown
        .iter()
        .filter_map(|i| chooser.entries.get(*i))
        .skip(chooser.scroll)
        .take(rows)
        .enumerate()
    {
        let top = list_top + 4.0 + visible as f32 * ROW_HEIGHT;
        let row_rect =
            Rect::from_min_size(pos2(rect.left() + 3.0, top), vec2(width - 6.0, ROW_HEIGHT));
        if chooser.scroll + visible == chooser.selected {
            painter.rect_filled(row_rect, 4.0, t.selection);
        }
        let detail = painter.layout_no_wrap(
            entry.detail.clone(),
            FontId::proportional(12.0),
            t.text_secondary,
        );
        let detail_width = detail.size().x;
        painter.galley(
            pos2(
                row_rect.right() - 10.0 - detail_width,
                row_rect.center().y - detail.size().y / 2.0,
            ),
            detail,
            t.text_secondary,
        );
        let mut left = row_rect.left() + 10.0 + entry.indent as f32 * 14.0;
        if let Some(kind) = entry.kind {
            let (letter, color) = kind_style(kind);
            let badge =
                Rect::from_center_size(pos2(left + 7.5, row_rect.center().y), vec2(15.0, 15.0));
            painter.rect_filled(badge, 4.0, color.gamma_multiply(0.22));
            painter.text(
                badge.center(),
                Align2::CENTER_CENTER,
                letter,
                FontId::monospace(11.0),
                color,
            );
            left += 22.0;
        }
        let font = if matches!(entry.choice, Choice::Go(_)) && entry.kind.is_none() {
            FontId::monospace(12.5)
        } else {
            FontId::proportional(13.0)
        };
        let label = painter.layout_no_wrap(entry.label.clone(), font, t.text);
        let clip = Rect::from_min_max(
            pos2(left, row_rect.top()),
            pos2(row_rect.right() - 20.0 - detail_width, row_rect.bottom()),
        );
        painter.with_clip_rect(clip).galley(
            pos2(clip.left(), row_rect.center().y - label.size().y / 2.0),
            label,
            t.text,
        );
    }
    rect
}

pub(super) fn draw_hover(
    painter: &Painter,
    t: &Theme,
    font: &FontId,
    problems: &[(Severity, String)],
    hover: Option<&Hover>,
    pointer: Pos2,
    screen: Rect,
) {
    let width = 460.0_f32.min(screen.width() - 16.0);
    let mut parts: Vec<(Arc<egui::Galley>, Option<Color32>)> = Vec::new();
    for (severity, message) in problems {
        parts.push((
            painter.layout(
                message.clone(),
                FontId::proportional(12.5),
                severity_color(*severity),
                width - 20.0,
            ),
            Some(severity_color(*severity)),
        ));
    }
    if let Some(hover) = hover {
        parts.push((
            painter.layout(
                hover.title.clone(),
                FontId::monospace((font.size - 1.0).max(11.0)),
                t.text,
                width - 20.0,
            ),
            None,
        ));
        if !hover.body.is_empty() {
            parts.push((
                painter.layout(
                    hover.body.clone(),
                    FontId::proportional(12.5),
                    t.text_secondary,
                    width - 20.0,
                ),
                None,
            ));
        }
    }
    if parts.is_empty() {
        return;
    }
    let content: f32 = parts.iter().map(|(g, _)| g.size().y + 6.0).sum();
    let widest = parts.iter().map(|(g, _)| g.size().x).fold(0.0, f32::max);
    let size = vec2((widest + 20.0).max(120.0), content + 8.0);
    let mut at = pos2(pointer.x + 12.0, pointer.y + 18.0);
    if at.x + size.x > screen.right() - 8.0 {
        at.x = (screen.right() - size.x - 8.0).max(screen.left());
    }
    if at.y + size.y > screen.bottom() - 8.0 {
        at.y = (pointer.y - size.y - 10.0).max(screen.top());
    }
    let rect = Rect::from_min_size(at, size);
    painter.rect(
        rect,
        5.0,
        t.popup,
        Stroke::new(1.0_f32, t.border),
        StrokeKind::Inside,
    );
    let mut y = rect.top() + 7.0;
    for (galley, color) in parts {
        let height = galley.size().y;
        painter.galley(pos2(rect.left() + 10.0, y), galley, color.unwrap_or(t.text));
        y += height + 6.0;
    }
}

#[cfg(test)]
#[path = "../../test/editor/assist/tests.rs"]
mod tests;
