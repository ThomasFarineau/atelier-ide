//! Code editor drawn line by line, as in IntelliJ: only the visible lines are laid out and
//! painted, so the cost of a frame does not depend on the size of the file.
//!
//! The text stays a `String` owned by the document; this module keeps the view state next to
//! it: the index of line starts, caret and selection, undo history and incremental
//! highlighting. Positions are byte offsets on char boundaries.

mod actions;
mod assist;
mod context;
mod fold;
mod intel;
mod intentions;
mod multi;
mod refactor;
mod spell;
mod templates;

use crate::{
    highlight::Incremental,
    languages::lsp::extras::{HintKind, InlayHint},
    plugins::builtin::{Command, LineMark, MarkKind},
    ui::Theme,
};
use assist::AssistState;
pub use assist::{Assist, FormatRequest};
use eframe::egui::{
    self, Color32, FontId, Id, Key, Modifiers, Pos2, Rect, Sense, Stroke, Ui, pos2, text::CCursor,
    vec2,
};
use std::{
    ops::Range,
    time::{Duration, Instant},
};

const INDENT: &str = "    ";
/// Display width of a tab, in columns.
const TAB: usize = 4;
/// Typing within this delay merges into one undo step.
const MERGE_DELAY: Duration = Duration::from_millis(800);
const HIGHLIGHT_BUDGET: Duration = Duration::from_millis(8);

#[derive(Clone)]
struct Change {
    at: usize,
    removed: String,
    inserted: String,
    before: (usize, usize),
    after: (usize, usize),
    /// The changes of one keystroke at several carets share a group, other than 0: they undo
    /// and redo together.
    group: u64,
}

/// View state of a document in the editor.
pub struct EditorState {
    pub cursor: usize,
    pub anchor: usize,
    /// Display column kept while moving up and down.
    preferred_column: Option<usize>,
    /// Byte offset of the start of each line.
    lines: Vec<usize>,
    /// Text length the index was built for; `usize::MAX` forces a rebuild.
    indexed_len: usize,
    /// Length in columns of the longest line, for the horizontal scroll range.
    longest: usize,
    undo: Vec<Change>,
    redo: Vec<Change>,
    last_typing: Option<Instant>,
    highlight: Option<Incremental>,
    scroll_to_cursor: bool,
    /// Last caret or text change, for the blinking caret.
    activity: Instant,
    pub scroll_fraction: f32,
    /// Grows with every change of the text, for the language service.
    rev: u64,
    assist: AssistState,
    intel: intel::IntelState,
    crumbs: context::CrumbState,
    spell: spell::SpellState,
    /// The language of the document, as of the last frame.
    language: String,
    /// The hints of the language server, sorted, and the request that brings the next ones.
    hints: Vec<InlayHint>,
    hints_state: intel::HintsState,
    /// Foldable regions of the text, and the ones that are collapsed (by opening offset).
    regions: Vec<fold::Region>,
    folded: Vec<usize>,
    /// Revision and language the regions were found for.
    fold_rev: u64,
    fold_language: String,
    /// The lines that are shown, when some are hidden by a collapsed region.
    visible: Vec<usize>,
    /// The mouse press that is on a fold marker: it does not select.
    fold_press: bool,
    /// The mouse press began in this editor. A press begun elsewhere, such as the Ctrl + click
    /// that opened this file, does not select while the button is still down.
    own_press: bool,
    /// The selections Extend Selection went from, and to, for Shrink Selection.
    expanded: Vec<((usize, usize), Range<usize>)>,
    /// The carets besides the primary one (`cursor` and `anchor`), the last added last.
    extras: Vec<multi::Caret>,
    /// The group of the changes being made at several carets, 0 outside of them.
    group: u64,
    next_group: u64,
    /// The mouse selects columns (Alt+Shift+Insert).
    pub column_mode: bool,
    /// Line and display column where a column selection began.
    column_anchor: Option<(usize, usize)>,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            cursor: 0,
            anchor: 0,
            preferred_column: None,
            lines: vec![0],
            indexed_len: usize::MAX,
            longest: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            last_typing: None,
            highlight: None,
            scroll_to_cursor: false,
            activity: Instant::now(),
            scroll_fraction: 0.0,
            rev: 0,
            assist: AssistState::default(),
            intel: Default::default(),
            crumbs: Default::default(),
            spell: Default::default(),
            language: String::new(),
            hints: Vec::new(),
            hints_state: Default::default(),
            regions: Vec::new(),
            folded: Vec::new(),
            fold_rev: u64::MAX,
            fold_language: String::new(),
            visible: Vec::new(),
            fold_press: false,
            own_press: false,
            expanded: Vec::new(),
            extras: Vec::new(),
            group: 0,
            next_group: 0,
            column_mode: false,
            column_anchor: None,
        }
    }
}

/// A hint of the language server on a line, for the layout.
struct HintCol {
    /// Display column of the character it comes before.
    column: usize,
    label: String,
    /// The caret at that column is before the hint (a type) rather than after it (an argument).
    before_caret: bool,
}

/// Puts the hints of a line in the layout between the pieces of its text.
struct Hints {
    hints: std::vec::IntoIter<HintCol>,
    next: Option<HintCol>,
    column: usize,
    format: egui::TextFormat,
}

impl Hints {
    fn new(hints: Vec<HintCol>, opt: &Options) -> Self {
        let mut hints = hints.into_iter();
        let next = hints.next();
        Self {
            hints,
            next,
            column: 0,
            format: egui::TextFormat {
                font_id: FontId::new(opt.font.size * 0.85, opt.font.family.clone()),
                color: opt.theme.text_secondary,
                background: opt.theme.hover,
                ..egui::TextFormat::default()
            },
        }
    }

    fn put(&mut self, job: &mut egui::text::LayoutJob) {
        if let Some(hint) = self.next.take() {
            job.append(&hint.label, 0.0, self.format.clone());
            self.next = self.hints.next();
        }
    }

    /// Appends a piece of the line, with the hints that fall inside it.
    fn append(&mut self, job: &mut egui::text::LayoutJob, piece: &str, format: egui::TextFormat) {
        let mut rest = piece;
        loop {
            let length = rest.chars().count();
            match &self.next {
                Some(hint) if hint.column <= self.column + length => {
                    let take = hint.column.saturating_sub(self.column);
                    let at = rest.char_indices().nth(take).map_or(rest.len(), |(i, _)| i);
                    if at > 0 {
                        job.append(&rest[..at], 0.0, format.clone());
                    }
                    self.column += take;
                    rest = &rest[at..];
                    self.put(job);
                }
                _ => {
                    if !rest.is_empty() {
                        job.append(rest, 0.0, format);
                    }
                    self.column += length;
                    return;
                }
            }
        }
    }

    /// The hints at the end of the line.
    fn finish(&mut self, job: &mut egui::text::LayoutJob) {
        while self.next.is_some() {
            self.put(job);
        }
    }
}

/// What the editor needs from the IDE for one frame.
pub struct Options<'a> {
    pub language: &'a str,
    pub font: FontId,
    pub theme: Theme,
    pub marks: &'a [LineMark],
    /// One annotation per line (Git blame), shown left of the line numbers.
    pub annotations: Option<&'a str>,
    pub gutter_commands: &'a [(&'static str, Command)],
    /// Completion, problems and tooltips, for the languages that have them.
    pub assist: Option<Assist<'a>>,
    /// The matches of the find bar, sorted.
    pub search: &'a [Range<usize>],
    /// The text can be read, selected and copied, not changed.
    pub read_only: bool,
    /// Hints of the language server in the code (types, names of arguments).
    pub inlay_hints: bool,
    /// The lines that open the blocks around the top of the window stay on top.
    pub sticky_lines: bool,
    /// The path of the code at the caret above the editor.
    pub breadcrumbs: bool,
    /// The lines (0-based) that have a bookmark, and its number when it has one.
    pub bookmarks: &'a [(usize, Option<u8>)],
    /// The misspelled words of the comments and strings are underlined.
    pub spellcheck: bool,
}

#[derive(Default)]
pub struct Output {
    pub changed: bool,
    /// 1-based line and column of the caret.
    pub line: usize,
    pub column: usize,
    /// A gutter menu command chosen by the user: (plugin, command).
    pub command: Option<(&'static str, &'static str)>,
    /// The vertical scroll position moved this frame.
    pub scrolled: bool,
    /// Problems found in the document.
    pub errors: usize,
    pub warnings: usize,
    /// A place to go to, chosen with Ctrl+click or Ctrl+B.
    pub navigate: Option<crate::languages::Location>,
    /// Changes to other files, from a rename.
    pub file_edits: Vec<(std::path::PathBuf, Vec<crate::languages::TextEdit>)>,
    /// A message for the status bar (after Reformat Code).
    pub message: Option<String>,
}

/// What a read-only editor does with an event: it moves, selects, folds and copies, nothing more.
fn allowed_when_read_only(event: &egui::Event) -> bool {
    match event {
        egui::Event::Copy => true,
        egui::Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } => match key {
            Key::ArrowLeft
            | Key::ArrowRight
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::PageUp
            | Key::PageDown
            | Key::Home
            | Key::End
            | Key::Escape => {
                !(modifiers.shift && (modifiers.alt || modifiers.command)) || *key == Key::Escape
            }
            Key::A | Key::W | Key::M | Key::Period | Key::Minus | Key::Equals | Key::Plus => {
                modifiers.command
            }
            _ => false,
        },
        _ => false,
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Line text with tabs expanded, as painted.
fn display(line: &str) -> String {
    line.replace('\t', &" ".repeat(TAB))
}

/// Display column of byte `offset` in `line`.
fn column_of(line: &str, offset: usize) -> usize {
    line[..offset.min(line.len())]
        .chars()
        .map(|c| if c == '\t' { TAB } else { 1 })
        .sum()
}

/// Byte offset in `line` closest to display `column`.
fn offset_of(line: &str, column: usize) -> usize {
    let mut current = 0;
    for (offset, c) in line.char_indices() {
        let width = if c == '\t' { TAB } else { 1 };
        if current + width / 2 >= column && current >= column.saturating_sub(width / 2) {
            return offset;
        }
        if current + width > column {
            return if column - current > width / 2 {
                offset + c.len_utf8()
            } else {
                offset
            };
        }
        current += width;
    }
    line.len()
}

pub fn comment_prefix(language: &str) -> Option<&'static str> {
    match language {
        "rs" | "js" | "ts" | "java" | "c" | "cpp" | "cs" | "go" | "php" | "css" => Some("//"),
        "py" | "sh" | "yaml" | "toml" | "rb" => Some("#"),
        "sql" => Some("--"),
        _ => None,
    }
}

impl EditorState {
    /// Forces the line index and highlighting to be rebuilt, after the text changed
    /// outside the editor (reload from disk, preview…).
    pub fn invalidate(&mut self) {
        self.indexed_len = usize::MAX;
        self.rev += 1;
        // Offsets in the history no longer match the new text.
        self.undo.clear();
        self.redo.clear();
    }

    fn sync(&mut self, text: &str) {
        if self.indexed_len == text.len() {
            return;
        }
        self.lines = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        self.longest = self
            .lines
            .windows(2)
            .map(|w| w[1] - w[0])
            .chain(std::iter::once(
                text.len() - self.lines[self.lines.len() - 1],
            ))
            .max()
            .unwrap_or(0);
        self.indexed_len = text.len();
        self.rev += 1;
        // The text changed outside of the editor: the other carets mean nothing any more.
        self.extras.clear();
        self.cursor = self.clamp(text, self.cursor);
        self.anchor = self.clamp(text, self.anchor);
        if let Some(highlight) = &mut self.highlight {
            highlight.invalidate_from(0);
        }
    }

    fn clamp(&self, text: &str, mut pos: usize) -> usize {
        pos = pos.min(text.len());
        while !text.is_char_boundary(pos) {
            pos -= 1;
        }
        pos
    }

    #[cfg(test)]
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line_of(&self, pos: usize) -> usize {
        self.lines.partition_point(|&start| start <= pos) - 1
    }

    // ---- Folding ------------------------------------------------------------------------

    /// Number of rows shown: the lines, less the ones hidden by collapsed regions.
    fn row_count(&self) -> usize {
        if self.visible.is_empty() {
            self.lines.len()
        } else {
            self.visible.len()
        }
    }

    /// Row where `line` is shown (a hidden line is shown on the row of its fold).
    fn row_of(&self, line: usize) -> usize {
        if self.visible.is_empty() {
            line
        } else {
            self.visible
                .partition_point(|&l| l <= line)
                .saturating_sub(1)
        }
    }

    /// Line shown on `row`.
    fn line_at(&self, row: usize) -> usize {
        if self.visible.is_empty() {
            row.min(self.lines.len() - 1)
        } else {
            self.visible[row.min(self.visible.len() - 1)]
        }
    }

    /// The region that opens on `line`, collapsed or not.
    fn region_at(&self, line: usize) -> Option<&fold::Region> {
        self.regions
            .binary_search_by_key(&line, |r| r.first)
            .ok()
            .map(|i| &self.regions[i])
    }

    /// The collapsed region shown on `line`.
    fn folded_at(&self, line: usize) -> Option<&fold::Region> {
        if self.folded.is_empty() {
            return None;
        }
        self.region_at(line)
            .filter(|r| self.folded.contains(&r.open))
    }

    fn rebuild_visible(&mut self) {
        let hidden = fold::hidden(&self.regions, &self.folded);
        self.visible.clear();
        if hidden.is_empty() {
            return;
        }
        let mut next = hidden.iter().peekable();
        for line in 0..self.lines.len() {
            while next.peek().is_some_and(|r| r.end <= line) {
                next.next();
            }
            if next.peek().is_none_or(|r| r.start > line) {
                self.visible.push(line);
            }
        }
    }

    /// Finds the regions again when the text changed, and the lines they hide.
    fn refresh_folds(&mut self, text: &str, language: &str) {
        if self.fold_rev != self.rev || self.fold_language != language {
            self.regions = fold::regions(text, &self.lines, language);
            let regions = &self.regions;
            self.folded
                .retain(|open| regions.iter().any(|r| r.open == *open));
            self.fold_rev = self.rev;
            self.fold_language = language.to_string();
        } else if self.folded.is_empty() && self.visible.is_empty() {
            return;
        }
        // Navigating to a hidden place opens what hides it.
        if self.scroll_to_cursor || self.selection().is_empty() {
            self.reveal_cursor();
        }
        self.rebuild_visible();
    }

    /// Opens the collapsed regions that hide the caret. Whether it did.
    fn reveal_cursor(&mut self) -> bool {
        if self.folded.is_empty() {
            return false;
        }
        let line = self.line_of(self.cursor);
        let before = self.folded.len();
        let regions = &self.regions;
        self.folded.retain(|open| {
            !regions
                .iter()
                .any(|r| r.open == *open && r.first < line && line <= r.last)
        });
        self.folded.len() != before
    }

    /// Moves the caret out of the collapsed regions, to the end of the one that hides it.
    fn park_cursor(&mut self) {
        let line = self.line_of(self.cursor);
        let hiding = self
            .regions
            .iter()
            .filter(|r| self.folded.contains(&r.open) && r.first < line && line <= r.last)
            .min_by_key(|r| r.first)
            .map(|r| r.head_end);
        if let Some(at) = hiding {
            self.cursor = at;
            self.anchor = at;
        }
    }

    fn toggle_fold(&mut self, open: usize) {
        match self.folded.iter().position(|&o| o == open) {
            Some(at) => {
                self.folded.remove(at);
            }
            None => self.folded.push(open),
        }
        self.park_cursor();
        self.rebuild_visible();
    }

    /// Collapses the innermost region around the caret, or opens the one on its line.
    fn fold_caret(&mut self, collapse: bool) {
        let line = self.line_of(self.cursor);
        let target = if collapse {
            self.regions
                .iter()
                .filter(|r| r.first <= line && line <= r.last && !self.folded.contains(&r.open))
                .max_by_key(|r| r.first)
                .map(|r| r.open)
        } else {
            self.folded_at(line).map(|r| r.open)
        };
        if let Some(open) = target {
            self.toggle_fold(open);
        }
    }

    fn fold_all(&mut self, collapse: bool) {
        self.folded = if collapse {
            self.regions.iter().map(|r| r.open).collect()
        } else {
            Vec::new()
        };
        self.park_cursor();
        self.rebuild_visible();
    }

    /// Byte offset of a 0-based line and column, both clamped to the text.
    pub fn offset_at(&self, text: &str, line: usize, column: usize) -> usize {
        let line = line.min(self.lines.len() - 1);
        self.lines[line] + offset_of(self.line(text, line), column)
    }

    /// Byte range of line `index`, without its line break.
    fn line_range(&self, text: &str, index: usize) -> Range<usize> {
        let start = self.lines[index];
        let mut end = self
            .lines
            .get(index + 1)
            .map_or(text.len(), |&next| next - 1);
        if end > start && text.as_bytes()[end - 1] == b'\r' {
            end -= 1;
        }
        start..end
    }

    fn line<'t>(&self, text: &'t str, index: usize) -> &'t str {
        &text[self.line_range(text, index)]
    }

    /// 1-based line and column of the caret.
    pub fn position(&self, text: &str) -> (usize, usize) {
        let line = self.line_of(self.cursor);
        let start = self.lines[line];
        (line + 1, text[start..self.cursor].chars().count() + 1)
    }

    pub fn selection(&self) -> Range<usize> {
        self.cursor.min(self.anchor)..self.cursor.max(self.anchor)
    }

    /// Grows with every change of the text.
    pub fn revision(&self) -> u64 {
        self.rev
    }

    /// Replaces a range of the text, as an edit that can be undone.
    pub fn replace_range(&mut self, text: &mut String, range: Range<usize>, insert: &str) {
        self.sync(text);
        let end = range.end.min(text.len());
        let start = range.start.min(end);
        self.replace(text, start..end, insert, false);
    }

    /// Replaces the whole text in one edit that can be undone, the caret staying in place.
    pub fn replace_text(&mut self, text: &mut String, new: &str) {
        self.sync(text);
        let caret = self.cursor;
        let len = text.len();
        self.replace(text, 0..len, new, false);
        let caret = self.clamp(text, caret);
        self.cursor = caret;
        self.anchor = caret;
    }

    /// Selects a range and scrolls to it, e.g. a search result.
    pub fn select(&mut self, start: usize, end: usize) {
        self.anchor = start;
        self.cursor = end;
        self.preferred_column = None;
        self.scroll_to_cursor = true;
        self.activity = Instant::now();
    }

    fn move_to(&mut self, pos: usize, extend: bool) {
        self.cursor = pos;
        if !extend {
            self.anchor = pos;
        }
        self.scroll_to_cursor = true;
        self.activity = Instant::now();
    }

    // ---- Editing ------------------------------------------------------------------------

    /// Replaces `range` by `insert`, records the change and updates the line index.
    fn replace(&mut self, text: &mut String, range: Range<usize>, insert: &str, typing: bool) {
        let before = (self.cursor, self.anchor);
        let removed = text[range.clone()].to_string();
        text.replace_range(range.clone(), insert);
        // Line index: drop the starts inside the range, add the new ones, shift the rest.
        let low = self.lines.partition_point(|&s| s <= range.start);
        let high = self.lines.partition_point(|&s| s <= range.end);
        let added: Vec<usize> = insert
            .match_indices('\n')
            .map(|(i, _)| range.start + i + 1)
            .collect();
        let delta = insert.len() as isize - removed.len() as isize;
        let count = added.len();
        self.lines.splice(low..high, added);
        for start in &mut self.lines[low + count..] {
            *start = (*start as isize + delta) as usize;
        }
        self.indexed_len = text.len();
        self.rev += 1;
        // Collapsed regions after the edit move with the text; the ones it touches open.
        self.folded.retain_mut(|open| {
            if *open >= range.end {
                *open = (*open as isize + delta) as usize;
                true
            } else {
                *open < range.start
            }
        });
        // The typos after the edit move with the text; the ones it touches go.
        if !self.spell.typos.is_empty() {
            self.spell.typos.retain_mut(|typo| {
                if typo.start >= range.end {
                    typo.start = (typo.start as isize + delta) as usize;
                    typo.end = (typo.end as isize + delta) as usize;
                    true
                } else {
                    typo.end <= range.start
                }
            });
        }
        // The hints after the edit move with the text; the ones inside it go.
        if !self.hints.is_empty() {
            self.hints.retain_mut(|hint| {
                if hint.offset >= range.end {
                    hint.offset = (hint.offset as isize + delta) as usize;
                    true
                } else {
                    hint.offset <= range.start
                }
            });
        }
        let first = self.line_of(range.start);
        let last = self.line_of(range.start + insert.len());
        for index in first..=last {
            self.longest = self.longest.max(self.line_range(text, index).len());
        }
        if let Some(highlight) = &mut self.highlight {
            highlight.invalidate_from(first);
        }
        let end = range.start + insert.len();
        self.cursor = end;
        self.anchor = end;
        let change = Change {
            at: range.start,
            removed,
            inserted: insert.to_string(),
            before,
            after: (end, end),
            group: self.group,
        };
        let merge = typing
            && self.group == 0
            && self.last_typing.is_some_and(|t| t.elapsed() < MERGE_DELAY)
            && change.removed.is_empty()
            && !insert.contains('\n')
            && self.undo.last().is_some_and(|last| {
                last.at + last.inserted.len() == change.at && last.removed.is_empty()
            });
        match self.undo.last_mut() {
            Some(last) if merge => {
                last.inserted.push_str(insert);
                last.after = change.after;
            }
            _ => self.undo.push(change),
        }
        self.last_typing = typing.then(Instant::now);
        self.redo.clear();
        self.preferred_column = None;
        self.scroll_to_cursor = true;
        self.activity = Instant::now();
    }

    fn insert(&mut self, text: &mut String, insert: &str, typing: bool) {
        self.replace(text, self.selection(), insert, typing);
    }

    fn apply(&mut self, text: &mut String, change: &Change, undo: bool) {
        let (remove, insert, caret) = if undo {
            (&change.inserted, &change.removed, change.before)
        } else {
            (&change.removed, &change.inserted, change.after)
        };
        let range = change.at..change.at + remove.len();
        let (undo_stack, redo_stack) = (
            std::mem::take(&mut self.undo),
            std::mem::take(&mut self.redo),
        );
        self.replace(text, range, insert, false);
        self.undo = undo_stack;
        self.redo = redo_stack;
        self.cursor = self.clamp(text, caret.0);
        self.anchor = self.clamp(text, caret.1);
        self.last_typing = None;
    }

    pub fn undo(&mut self, text: &mut String) -> bool {
        let Some(change) = self.undo.pop() else {
            return false;
        };
        self.extras.clear();
        let group = change.group;
        self.apply(text, &change, true);
        self.redo.push(change);
        // The other changes of the keystroke go with it.
        while group != 0 && self.undo.last().is_some_and(|c| c.group == group) {
            let change = self.undo.pop().expect("checked");
            self.apply(text, &change, true);
            self.redo.push(change);
        }
        true
    }

    pub fn redo(&mut self, text: &mut String) -> bool {
        let Some(change) = self.redo.pop() else {
            return false;
        };
        self.extras.clear();
        let group = change.group;
        self.apply(text, &change, false);
        self.undo.push(change);
        while group != 0 && self.redo.last().is_some_and(|c| c.group == group) {
            let change = self.redo.pop().expect("checked");
            self.apply(text, &change, false);
            self.undo.push(change);
        }
        true
    }

    /// Byte range of the whole lines touched by the selection, line breaks excluded.
    fn selected_lines(&self, text: &str) -> Range<usize> {
        let range = self.selection();
        let first = self.line_of(range.start);
        let mut last = self.line_of(range.end);
        if last > first && range.end == self.lines[last] {
            last -= 1;
        }
        self.lines[first]..self.line_range(text, last).end
    }

    /// Rewrites each selected line, keeping the whole block selected.
    fn edit_lines(&mut self, text: &mut String, edit: impl Fn(&str) -> String) {
        let block = self.selected_lines(text);
        let new: Vec<String> = text[block.clone()].split('\n').map(&edit).collect();
        let new = new.join("\n");
        let start = block.start;
        let single = !text[block.clone()].contains('\n');
        let column = self.cursor - self.lines[self.line_of(self.cursor)];
        let old_len = block.len();
        self.replace(text, block, &new, false);
        if single && self.selection().is_empty() {
            // Keep the caret on its line, moved by the inserted or removed indent.
            let delta = new.len() as isize - old_len as isize;
            let pos = (start as isize + column as isize + delta).max(start as isize) as usize;
            let pos = self.clamp(text, pos.min(start + new.len()));
            self.cursor = pos;
            self.anchor = pos;
        } else {
            self.anchor = start;
            self.cursor = start + new.len();
        }
    }

    fn word_left(&self, text: &str, pos: usize) -> usize {
        let before: Vec<(usize, char)> = text[..pos].char_indices().rev().collect();
        let mut iter = before.into_iter().peekable();
        let mut result = pos;
        while let Some(&(i, c)) = iter.peek() {
            if c.is_whitespace() && c != '\n' {
                result = i;
                iter.next();
            } else {
                break;
            }
        }
        let Some(&(_, first)) = iter.peek() else {
            return 0;
        };
        while let Some(&(i, c)) = iter.peek() {
            if c == '\n' && result == pos {
                return i;
            }
            if is_word(c) == is_word(first) && !c.is_whitespace() {
                result = i;
                iter.next();
            } else {
                break;
            }
        }
        result
    }

    fn word_right(&self, text: &str, pos: usize) -> usize {
        let mut iter = text[pos..].char_indices().peekable();
        let Some(&(_, first)) = iter.peek() else {
            return pos;
        };
        if first == '\n' {
            return pos + 1;
        }
        let mut result = pos;
        while let Some(&(i, c)) = iter.peek() {
            if !c.is_whitespace() && is_word(c) == is_word(first) {
                result = pos + i + c.len_utf8();
                iter.next();
            } else {
                break;
            }
        }
        while let Some(&(i, c)) = iter.peek() {
            if c.is_whitespace() && c != '\n' {
                result = pos + i + c.len_utf8();
                iter.next();
            } else {
                break;
            }
        }
        result
    }

    fn word_at(&self, text: &str, pos: usize) -> Range<usize> {
        let line = self.line_range(text, self.line_of(pos));
        let mut start = pos;
        for (i, c) in text[line.start..pos].char_indices().rev() {
            if !is_word(c) {
                break;
            }
            start = line.start + i;
        }
        let mut end = pos;
        for (i, c) in text[pos..line.end].char_indices() {
            if !is_word(c) {
                break;
            }
            end = pos + i + c.len_utf8();
        }
        start..end
    }

    fn prev_char(&self, text: &str, pos: usize) -> usize {
        text[..pos].char_indices().next_back().map_or(0, |(i, _)| i)
    }

    fn next_char(&self, text: &str, pos: usize) -> usize {
        text[pos..]
            .chars()
            .next()
            .map_or(pos, |c| pos + c.len_utf8())
    }

    fn vertical(&mut self, text: &str, lines: isize, extend: bool) {
        let line = self.line_of(self.cursor);
        let current = self.line(text, line);
        let column = self
            .preferred_column
            .unwrap_or_else(|| column_of(current, self.cursor - self.lines[line]));
        // Hidden lines are skipped: the move counts visible rows.
        let rows = self.row_count() as isize;
        let target_row = (self.row_of(line) as isize + lines).clamp(0, rows - 1) as usize;
        let target = self.line_at(target_row);
        let pos = if target == line && lines < 0 {
            self.lines[line]
        } else if target == line && lines > 0 {
            self.line_range(text, line).end
        } else {
            self.lines[target] + offset_of(self.line(text, target), column)
        };
        self.move_to(pos, extend);
        self.preferred_column = Some(column);
    }

    fn type_text(&mut self, text: &mut String, typed: &str) {
        let pairs = [('(', ')'), ('[', ']'), ('{', '}'), ('"', '"')];
        let mut chars = typed.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            let next = text[self.cursor..].chars().next();
            // Typing a closing bracket over the one inserted automatically.
            if self.selection().is_empty() && next == Some(c) && pairs.iter().any(|p| p.1 == c) {
                self.move_to(self.cursor + c.len_utf8(), false);
                return;
            }
            if let Some(&(open, close)) = pairs.iter().find(|p| p.0 == c)
                && self.selection().is_empty()
                && next.is_none_or(|n| n.is_whitespace() || ")]};,".contains(n))
                && (open != '"' || !text[..self.cursor].ends_with(is_word))
            {
                self.insert(text, &format!("{open}{close}"), true);
                self.move_to(self.cursor - close.len_utf8(), false);
                return;
            }
        }
        self.insert(text, typed, true);
    }

    fn newline(&mut self, text: &mut String) {
        let line = self.line(text, self.line_of(self.cursor));
        let indent: String = line
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let before = text[..self.selection().start]
            .trim_end_matches([' ', '\t'])
            .chars()
            .next_back();
        let after = text[self.selection().end..].chars().next();
        let opens = matches!(before, Some('{' | '(' | '['));
        if opens && matches!(after, Some('}' | ')' | ']')) {
            // Between brackets: the closing one goes to its own line, as in IntelliJ.
            let inserted = format!("\n{indent}{INDENT}\n{indent}");
            self.insert(text, &inserted, false);
            let caret = self.cursor - indent.len() - 1;
            self.move_to(caret, false);
        } else if opens {
            self.insert(text, &format!("\n{indent}{INDENT}"), false);
        } else {
            self.insert(text, &format!("\n{indent}"), false);
        }
    }

    fn backspace(&mut self, text: &mut String, word: bool) {
        if !self.selection().is_empty() {
            self.insert(text, "", false);
            return;
        }
        if self.cursor == 0 {
            return;
        }
        let start = if word {
            self.word_left(text, self.cursor)
        } else {
            let line_start = self.lines[self.line_of(self.cursor)];
            let before = &text[line_start..self.cursor];
            if !before.is_empty() && before.chars().all(|c| c == ' ') {
                // Inside the indentation: remove up to one indent level.
                let remove = (before.len() - 1) % INDENT.len() + 1;
                self.cursor - remove
            } else {
                self.prev_char(text, self.cursor)
            }
        };
        let mut end = self.cursor;
        let pair = (
            text[start..self.cursor].chars().next(),
            text[self.cursor..].chars().next(),
        );
        if matches!(
            pair,
            (Some('('), Some(')'))
                | (Some('['), Some(']'))
                | (Some('{'), Some('}'))
                | (Some('"'), Some('"'))
        ) && self.cursor - start == 1
        {
            end += 1;
        }
        self.replace(text, start..end, "", false);
    }

    fn delete(&mut self, text: &mut String, word: bool) {
        if !self.selection().is_empty() {
            self.insert(text, "", false);
            return;
        }
        let end = if word {
            self.word_right(text, self.cursor)
        } else {
            self.next_char(text, self.cursor)
        };
        self.replace(text, self.cursor..end, "", false);
    }

    fn toggle_comment(&mut self, text: &mut String, language: &str) {
        let Some(prefix) = comment_prefix(language) else {
            return;
        };
        let block = self.selected_lines(text);
        let lines: Vec<&str> = text[block].split('\n').collect();
        let commented = lines
            .iter()
            .filter(|l| !l.trim().is_empty())
            .all(|l| l.trim_start().starts_with(prefix));
        let indent = lines
            .iter()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.len() - l.trim_start().len())
            .min()
            .unwrap_or(0);
        self.edit_lines(text, |line| {
            if line.trim().is_empty() {
                line.to_string()
            } else if commented {
                let start = line.len() - line.trim_start().len();
                let rest = &line[start + prefix.len()..];
                format!(
                    "{}{}",
                    &line[..start],
                    rest.strip_prefix(' ').unwrap_or(rest)
                )
            } else {
                format!("{}{prefix} {}", &line[..indent], &line[indent..])
            }
        });
    }

    fn duplicate(&mut self, text: &mut String) {
        let selection = self.selection();
        if selection.is_empty() {
            let line = self.line_range(text, self.line_of(self.cursor));
            let column = self.cursor - line.start;
            let copy = format!("\n{}", &text[line.clone()]);
            self.replace(text, line.end..line.end, &copy, false);
            let pos = line.end + 1 + column;
            self.move_to(pos, false);
        } else {
            let copy = text[selection.clone()].to_string();
            self.replace(text, selection.end..selection.end, &copy, false);
            self.anchor = selection.end;
            self.cursor = selection.end + copy.len();
        }
    }

    fn delete_line(&mut self, text: &mut String) {
        let block = self.selected_lines(text);
        let range = if block.end < text.len() {
            block.start..(block.end + 1).min(text.len())
        } else {
            block.start.saturating_sub(usize::from(block.start > 0))..block.end
        };
        let range = if text[range.clone()].ends_with("\r\n") || range.end <= text.len() {
            range
        } else {
            range.start..text.len()
        };
        self.replace(text, range, "", false);
    }

    /// Keyboard and clipboard events while the editor has the focus.
    fn handle_events(
        &mut self,
        ui: &Ui,
        text: &mut String,
        language: &str,
        page: isize,
        read_only: bool,
    ) -> bool {
        let events = ui.input(|i| i.events.clone());
        let before = text.len();
        let mut changed = false;
        let mut typed_char = None;
        let mut erased = false;
        for event in events {
            if read_only && !allowed_when_read_only(&event) {
                continue;
            }
            if self.has_extras()
                && let Some(edited) = self.multi_event(ui, text, language, &event, page)
            {
                changed |= edited;
                typed_char = None;
                continue;
            }
            match event {
                egui::Event::Text(typed) => {
                    // A list that can be filtered takes what is typed.
                    if self.assist_text(&typed) {
                        continue;
                    }
                    typed_char = typed.chars().last();
                    self.type_text(text, &typed);
                    changed = true;
                }
                egui::Event::Ime(egui::ImeEvent::Commit(typed)) if !typed.is_empty() => {
                    self.insert(text, &typed, true);
                    changed = true;
                }
                egui::Event::Paste(pasted) => {
                    self.insert(text, &pasted.replace("\r\n", "\n"), false);
                    changed = true;
                }
                egui::Event::Copy | egui::Event::Cut => {
                    let cut = matches!(event, egui::Event::Cut);
                    let selection = self.selection();
                    if selection.is_empty() {
                        // No selection: the whole line, as IntelliJ does.
                        let line = self.line_range(text, self.line_of(self.cursor));
                        ui.ctx().copy_text(format!("{}\n", &text[line]));
                        if cut {
                            self.delete_line(text);
                            changed = true;
                        }
                    } else {
                        ui.ctx().copy_text(text[selection].to_string());
                        if cut {
                            self.insert(text, "", false);
                            changed = true;
                        }
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    erased |= matches!(key, Key::Backspace | Key::Delete);
                    if !matches!(key, Key::Backspace | Key::Delete | Key::Enter | Key::Tab) {
                        typed_char = None;
                    }
                    changed |= self.key(text, language, key, modifiers, page);
                }
                _ => {}
            }
        }
        let changed = changed || text.len() != before;
        if changed {
            self.assist_after_edit(text, typed_char, erased);
        }
        changed
    }

    fn key(
        &mut self,
        text: &mut String,
        language: &str,
        key: Key,
        m: Modifiers,
        page: isize,
    ) -> bool {
        if let Some(changed) = self.assist_key(text, key, m) {
            return changed;
        }
        match key {
            // Alt+J, Ctrl+Alt+Shift+J, Alt+Shift+J: the carets.
            Key::J if m.alt && m.command && m.shift => {
                self.select_all_occurrences(text);
                return false;
            }
            Key::J if m.alt && m.shift => {
                self.unselect_occurrence();
                return false;
            }
            Key::J if m.alt && !m.command => {
                self.add_next_occurrence(text);
                return false;
            }
            Key::Insert if m.alt && m.shift => {
                self.toggle_column_mode();
                return false;
            }
            _ => {}
        }
        // The refactorings of the IDE.
        if m.command && m.alt && !m.shift {
            let native = match key {
                Key::V => Some(refactor::Native::ExtractVariable),
                Key::C => Some(refactor::Native::ExtractConstant),
                Key::M => Some(refactor::Native::ExtractMethod),
                Key::N => Some(refactor::Native::InlineVariable),
                _ => None,
            };
            if let Some(which) = native {
                self.run_native(text, language, which);
                return true;
            }
        }
        if key == Key::T && m.command && m.alt && !m.shift {
            self.open_surround(text, language);
            return false;
        }
        if self.intel_key(key, m) {
            return false;
        }
        self.key_core(text, language, key, m, page)
    }

    /// The keys that act on the text and the caret, without the completion popup.
    fn key_core(
        &mut self,
        text: &mut String,
        language: &str,
        key: Key,
        m: Modifiers,
        page: isize,
    ) -> bool {
        let extend = m.shift;
        let word = m.command || m.alt;
        let line = self.line_of(self.cursor);
        match key {
            Key::ArrowLeft => {
                let selection = self.selection();
                let pos = if !extend && !selection.is_empty() && !word {
                    selection.start
                } else if word {
                    self.word_left(text, self.cursor)
                } else {
                    self.prev_char(text, self.cursor)
                };
                self.move_to(pos, extend);
                self.preferred_column = None;
            }
            Key::ArrowRight => {
                let selection = self.selection();
                let pos = if !extend && !selection.is_empty() && !word {
                    selection.end
                } else if word {
                    self.word_right(text, self.cursor)
                } else {
                    self.next_char(text, self.cursor)
                };
                self.move_to(pos, extend);
                self.preferred_column = None;
            }
            Key::ArrowUp | Key::ArrowDown if m.shift && m.alt => {
                self.move_lines(text, key == Key::ArrowUp);
                return true;
            }
            Key::ArrowUp | Key::ArrowDown if m.shift && m.command => {
                self.move_statement(text, key == Key::ArrowUp);
                return true;
            }
            Key::ArrowUp => self.vertical(text, -1, extend),
            Key::ArrowDown => self.vertical(text, 1, extend),
            Key::PageUp => self.vertical(text, -page, extend),
            Key::PageDown => self.vertical(text, page, extend),
            Key::Home if m.command => self.move_to(0, extend),
            Key::End if m.command => self.move_to(text.len(), extend),
            Key::Home => {
                // Smart home: first non-blank character, then the line start.
                let range = self.line_range(text, line);
                let indent = text[range.clone()].len() - text[range.clone()].trim_start().len();
                let first = range.start + indent;
                let pos = if self.cursor == first {
                    range.start
                } else {
                    first
                };
                self.move_to(pos, extend);
                self.preferred_column = None;
            }
            Key::End => {
                let pos = self.line_range(text, line).end;
                self.move_to(pos, extend);
                self.preferred_column = None;
            }
            Key::A if m.command => {
                self.anchor = 0;
                self.cursor = text.len();
            }
            Key::M if m.command && m.shift => {
                // To the bracket that matches the one at the caret.
                if let Some((a, b)) = self.matching_bracket(text) {
                    let target = if self.cursor == a || self.cursor == a + 1 {
                        b
                    } else {
                        a
                    };
                    self.move_to(target + 1, extend);
                }
            }
            Key::W if m.command && m.shift => self.shrink_selection(),
            Key::W if m.command => self.extend_selection(text),
            Key::J if m.command && m.shift => {
                self.join_lines(text);
                return true;
            }
            Key::U if m.command && m.shift => {
                self.toggle_case(text);
                return true;
            }
            Key::Z if m.command && m.shift => return self.redo(text),
            Key::Z if m.command => return self.undo(text),
            // Redo after an undo (the Windows habit), otherwise IntelliJ's delete line.
            Key::Y if m.command && !self.redo.is_empty() => return self.redo(text),
            Key::Y if m.command => {
                self.delete_line(text);
                return true;
            }
            Key::D if m.command => {
                self.duplicate(text);
                return true;
            }
            Key::Minus if m.command && m.shift => self.fold_all(true),
            Key::Minus if m.command => self.fold_caret(true),
            Key::Equals if m.command && m.shift => self.fold_all(false),
            Key::Equals | Key::Plus if m.command => self.fold_caret(false),
            Key::Period if m.command => {
                let line = self.line_of(self.cursor);
                let collapse = self.folded_at(line).is_none();
                self.fold_caret(collapse);
            }
            Key::Slash if m.command => {
                self.toggle_comment(text, language);
                return true;
            }
            Key::Enter => {
                self.newline(text);
                return true;
            }
            Key::Tab if m.shift => {
                self.edit_lines(text, |l| {
                    let spaces = l.len() - l.trim_start_matches(' ').len();
                    if let Some(rest) = l.strip_prefix('\t') {
                        rest.to_string()
                    } else {
                        l[spaces.min(INDENT.len())..].to_string()
                    }
                });
                return true;
            }
            Key::Tab => {
                if text[self.selection()].contains('\n') {
                    self.edit_lines(text, |l| {
                        if l.is_empty() {
                            l.to_string()
                        } else {
                            format!("{INDENT}{l}")
                        }
                    });
                } else {
                    let column = column_of(self.line(text, line), self.cursor - self.lines[line]);
                    let spaces = INDENT.len() - column % INDENT.len();
                    self.insert(text, &" ".repeat(spaces), false);
                }
                return true;
            }
            Key::Backspace => {
                self.backspace(text, m.command || m.alt);
                return true;
            }
            Key::Delete => {
                self.delete(text, m.command || m.alt);
                return true;
            }
            Key::Escape => {
                self.anchor = self.cursor;
            }
            _ => {}
        }
        false
    }

    /// The two brackets of a pair at the caret: the one before it or the one after it, and
    /// the one it closes or opens. Byte offsets of the brackets.
    fn matching_bracket(&self, text: &str) -> Option<(usize, usize)> {
        const LIMIT: usize = 60_000;
        const PAIRS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];
        let before = text[..self.cursor]
            .chars()
            .next_back()
            .map(|c| (self.cursor - c.len_utf8(), c));
        let after = text[self.cursor..].chars().next().map(|c| (self.cursor, c));
        // Brackets next to the caret: a closing one before it wins, then an opening one after it.
        let (at, bracket) = [
            before.filter(|(_, c)| PAIRS.iter().any(|p| p.1 == *c)),
            after.filter(|(_, c)| PAIRS.iter().any(|p| p.0 == *c)),
            before.filter(|(_, c)| PAIRS.iter().any(|p| p.0 == *c)),
            after.filter(|(_, c)| PAIRS.iter().any(|p| p.1 == *c)),
        ]
        .into_iter()
        .flatten()
        .next()?;
        let (open, close) = *PAIRS.iter().find(|p| p.0 == bracket || p.1 == bracket)?;
        let mut depth = 0usize;
        if bracket == open {
            for (i, c) in text[at..].char_indices().take(LIMIT) {
                if c == open {
                    depth += 1;
                } else if c == close {
                    depth -= 1;
                    if depth == 0 {
                        return Some((at, at + i));
                    }
                }
            }
        } else {
            for (i, c) in text[..at + 1].char_indices().rev().take(LIMIT) {
                if c == close {
                    depth += 1;
                } else if c == open {
                    depth -= 1;
                    if depth == 0 {
                        return Some((i, at));
                    }
                }
            }
        }
        None
    }

    // ---- Drawing ------------------------------------------------------------------------

    /// Galley of a visible line, colored when highlighting has reached it, with the hints of the
    /// language server between the characters they belong to.
    fn galley(
        &mut self,
        ui: &Ui,
        text: &str,
        index: usize,
        opt: &Options,
    ) -> std::sync::Arc<egui::Galley> {
        let full = self.line(text, index);
        // A collapsed region: the line up to its opening, a placeholder, the closing.
        let fold = self
            .folded_at(index)
            .map(|r| (r.head_end, r.tail_start, r.last));
        let head = fold.map_or(full.len(), |(head_end, ..)| {
            head_end.saturating_sub(self.lines[index]).min(full.len())
        });
        let line = &full[..head];
        let mut job = egui::text::LayoutJob::default();
        let format = |color| egui::TextFormat::simple(opt.font.clone(), color);
        let mut hints = Hints::new(self.hint_cols(text, index, head), opt);
        let spans = self
            .highlight
            .as_mut()
            .and_then(|h| h.line(index, full))
            .map(|l| l.spans.clone());
        match spans {
            Some(spans) if !spans.is_empty() => {
                let mut end = 0;
                for (range, color) in spans {
                    let range = range.start.min(head)..range.end.min(head);
                    if range.start > end {
                        hints.append(
                            &mut job,
                            &display(&line[end..range.start]),
                            format(opt.theme.text),
                        );
                    }
                    if range.end > range.start
                        && line.is_char_boundary(range.start)
                        && line.is_char_boundary(range.end)
                    {
                        hints.append(&mut job, &display(&line[range.clone()]), format(color));
                        end = range.end;
                    }
                }
                if end < line.len() {
                    hints.append(&mut job, &display(&line[end..]), format(opt.theme.text));
                }
            }
            _ => hints.append(&mut job, &display(line), format(opt.theme.text)),
        }
        hints.finish(&mut job);
        if let Some((_, tail_start, last)) = fold {
            let chip = egui::TextFormat {
                background: opt.theme.hover,
                ..egui::TextFormat::simple(opt.font.clone(), opt.theme.text_secondary)
            };
            job.append("...", 0.0, chip);
            let end = self.line_range(text, last).end;
            if tail_start < end {
                job.append(
                    &display(&text[tail_start..end]),
                    0.0,
                    format(opt.theme.text),
                );
            }
        }
        job.wrap.max_width = f32::INFINITY;
        ui.fonts_mut(|f| f.layout_job(job))
    }

    // ---- Hints --------------------------------------------------------------------------

    /// The hints of a line, in order. Only the ones before `head` (a byte offset in the line)
    /// when the line is collapsed.
    fn hint_cols(&self, text: &str, index: usize, head: usize) -> Vec<HintCol> {
        if self.hints.is_empty() {
            return Vec::new();
        }
        let start = self.lines[index];
        let line = self.line(text, index);
        let from = self.hints.partition_point(|h| h.offset < start);
        self.hints[from..]
            .iter()
            .take_while(|h| h.offset <= start + line.len())
            .filter(|h| h.offset - start <= head && line.is_char_boundary(h.offset - start))
            .map(|h| {
                let parameter = h.kind == HintKind::Parameter;
                HintCol {
                    column: column_of(line, h.offset - start),
                    label: if parameter {
                        format!("{} ", h.label)
                    } else {
                        h.label.clone()
                    },
                    before_caret: !parameter,
                }
            })
            .collect()
    }

    /// The column of the galley for the display column `column` of a line: what the hints before
    /// it add.
    fn visual_column(&self, text: &str, index: usize, column: usize) -> usize {
        if self.hints.is_empty() {
            return column;
        }
        let head = self.line(text, index).len();
        column
            + self
                .hint_cols(text, index, head)
                .iter()
                .filter(|h| h.column < column || (h.column == column && !h.before_caret))
                .map(|h| h.label.chars().count())
                .sum::<usize>()
    }

    /// The display column of a column of the galley: a hint takes the column it is before.
    fn source_column(&self, text: &str, index: usize, visual: usize) -> usize {
        if self.hints.is_empty() {
            return visual;
        }
        let head = self.line(text, index).len();
        let mut added = 0;
        for hint in self.hint_cols(text, index, head) {
            let start = hint.column + added;
            let length = hint.label.chars().count();
            if visual < start {
                break;
            }
            if visual < start + length {
                return hint.column;
            }
            added += length;
        }
        visual - added
    }

    /// Byte offset under a point of the text area.
    #[allow(clippy::too_many_arguments)]
    fn hit(
        &mut self,
        ui: &Ui,
        text: &str,
        origin: Pos2,
        row: f32,
        text_x: f32,
        pos: Pos2,
        opt: &Options,
    ) -> usize {
        let index = self.line_at(((pos.y - origin.y) / row).floor().max(0.0) as usize);
        let galley = self.galley(ui, text, index, opt);
        let mut column = galley
            .cursor_from_pos(vec2(pos.x - origin.x - text_x, row / 2.0))
            .index;
        column = self.source_column(text, index, column);
        // On a collapsed line the placeholder and the closing are not text of this line.
        if let Some(r) = self.folded_at(index) {
            column = column.min(column_of(
                self.line(text, index),
                r.head_end - self.lines[index],
            ));
        }
        self.lines[index] + offset_of(self.line(text, index), column)
    }

    /// Draws the editor and handles its input.
    pub fn show(&mut self, ui: &mut Ui, id: Id, text: &mut String, opt: &Options) -> Output {
        self.sync(text);
        if self.language != opt.language {
            self.language = opt.language.to_string();
        }
        if self
            .highlight
            .as_ref()
            .is_none_or(|h| h.outdated(opt.language, opt.theme.dark))
        {
            self.highlight = Some(Incremental::new(opt.language, opt.theme.dark));
        }
        self.refresh_folds(text, opt.language);
        self.assist_frame(ui, text, opt);
        self.intel_frame(ui, text, opt);
        self.spell_frame(ui, text, opt);
        self.spell_fixes_alone(text, opt);
        let t = opt.theme;
        let (row, char_width) =
            ui.fonts_mut(|f| (f.row_height(&opt.font), f.glyph_width(&opt.font, 'M')));
        let row = row.ceil();
        let annotations: Vec<&str> = opt
            .annotations
            .map(|a| a.lines().collect())
            .unwrap_or_default();
        let annotation_width = annotations
            .iter()
            .take(200)
            .map(|a| a.chars().count())
            .max()
            .unwrap_or(0);
        let digits = self.lines.len().to_string().len().max(3);
        let annotation_x = 10.0;
        let numbers_x = annotation_x
            + if annotation_width > 0 {
                (annotation_width as f32 + 2.0) * char_width
            } else {
                0.0
            };
        let marks_x = numbers_x + digits as f32 * char_width + 6.0;
        let gutter = marks_x + 30.0;
        let text_x = gutter + 4.0;
        let mut output = Output::default();
        // A rename answered: its changes to this document.
        if let Some(edits) = self.assist.apply.take() {
            self.apply_edits(text, edits);
            output.changed = true;
        }
        self.crumbs_frame(ui, text, opt);
        if opt.breadcrumbs && opt.assist.is_some() {
            self.show_crumbs(ui, text, opt);
        }
        let viewport_height = ui.available_height();
        let page = ((viewport_height / row) as isize - 2).max(1);
        let area = egui::ScrollArea::both()
            .id_salt(id)
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                let width =
                    (text_x + (self.longest as f32 + 8.0) * char_width).max(ui.available_width());
                // The scroll stops when the last line reaches the bottom, with a little room
                // under it; a file shorter than the window does not scroll at all.
                let rows = self.row_count() as f32 * row;
                let height = if rows > viewport_height {
                    rows + row
                } else {
                    rows
                };
                let (rect, response) =
                    ui.allocate_exact_size(vec2(width, height), Sense::click_and_drag());
                let origin = rect.min;
                if response.clicked()
                    || response.drag_started()
                    || (cfg!(debug_assertions) && std::env::var_os("ATELIER_SHOT_FOCUS").is_some())
                {
                    response.request_focus();
                }
                let focused = response.has_focus();
                // The lines kept on top while the block they open is in view.
                let top_row = (viewport.min.y / row).ceil().max(0.0) as usize;
                let sticky = if opt.sticky_lines {
                    self.sticky_lines(text, top_row)
                } else {
                    Vec::new()
                };
                let sticky_rect = (!sticky.is_empty()).then(|| {
                    Rect::from_min_size(
                        // From the very top of what is visible: no sliver of scrolled text above.
                        pos2(origin.x + viewport.min.x, ui.clip_rect().top()),
                        vec2(viewport.width(), sticky.len() as f32 * row),
                    )
                });
                // Leaving the editor closes the popup (not before it ever had the focus).
                self.assist.focus_changed(focused);
                if focused {
                    ui.memory_mut(|m| {
                        m.set_focus_lock_filter(
                            response.id,
                            egui::EventFilter {
                                tab: true,
                                horizontal_arrows: true,
                                vertical_arrows: true,
                                ..Default::default()
                            },
                        )
                    });
                    output.changed |=
                        self.handle_events(ui, text, opt.language, page, opt.read_only);
                }
                // Mouse: click places the caret, drag selects, double and triple click select
                // the word and the line. The gutter selects whole lines.
                let pointer = ui.input(|i| i.pointer.interact_pos());
                // A click on the completion popup is for the popup.
                let mut popup_click = false;
                if let Some(pos) = pointer
                    && ui.input(|i| i.pointer.primary_pressed())
                    && let Some(taken) = self.assist_click(text, pos)
                {
                    popup_click = true;
                    output.changed |= taken;
                }
                let over_sticky =
                    pointer.is_some_and(|p| sticky_rect.is_some_and(|r| r.contains(p)));
                let over_popup = over_sticky
                    || pointer.is_some_and(|p| self.popup_rect().is_some_and(|r| r.contains(p)));
                // A click on a kept line goes to it.
                if let (Some(pos), Some(area)) = (pointer, sticky_rect)
                    && over_sticky
                    && ui.input(|i| i.pointer.primary_pressed())
                {
                    let which = ((pos.y - area.top()) / row).floor().max(0.0) as usize;
                    if let Some(&line) = sticky.get(which) {
                        let at = self.lines[line];
                        self.select(at, at);
                    }
                }
                // Ctrl + click goes to the declaration instead of placing the caret.
                let ctrl = ui.input(|i| i.modifiers.command);
                if let Some(pos) = pointer
                    && ctrl
                    && !popup_click
                    && !over_popup
                    && response.hovered()
                    && ui.input(|i| i.pointer.primary_pressed())
                    && pos.x - origin.x - viewport.min.x >= gutter
                {
                    self.assist.ctrl_press = true;
                    let at = self.hit(ui, text, origin, row, text_x, pos, opt);
                    self.assist_goto(at);
                }
                if !ui.input(|i| i.pointer.primary_down()) {
                    self.assist.ctrl_press = false;
                }
                // The fold markers of the gutter and the placeholder of a collapsed region.
                if !ui.input(|i| i.pointer.primary_down()) {
                    self.fold_press = false;
                    self.own_press = false;
                    self.column_anchor = None;
                }
                if response.hovered() && ui.input(|i| i.pointer.primary_pressed()) {
                    self.own_press = true;
                }
                if let Some(pos) = pointer
                    && !popup_click
                    && !over_popup
                    && response.hovered()
                    && ui.input(|i| i.pointer.primary_pressed())
                {
                    let row_index = ((pos.y - origin.y) / row).floor();
                    if row_index >= 0.0 && (row_index as usize) < self.row_count() {
                        let line = self.line_at(row_index as usize);
                        let local_x = pos.x - origin.x - viewport.min.x;
                        if let Some((open, head_end)) =
                            self.region_at(line).map(|r| (r.open, r.head_end))
                        {
                            let on_marker = local_x >= marks_x + 10.0 && local_x < gutter;
                            let on_placeholder =
                                local_x >= gutter && self.folded.contains(&open) && {
                                    let galley = self.galley(ui, text, line, opt);
                                    let head = column_of(
                                        self.line(text, line),
                                        head_end - self.lines[line],
                                    );
                                    let x = pos.x - origin.x - text_x;
                                    x >= galley
                                        .pos_from_cursor(CCursor::new(
                                            self.visual_column(text, line, head),
                                        ))
                                        .min
                                        .x
                                        - 1.0
                                        && x <= galley.size().x + 2.0
                                };
                            if on_marker || on_placeholder {
                                self.toggle_fold(open);
                                self.fold_press = true;
                            }
                        }
                    }
                }
                if let Some(pos) = pointer
                    && !popup_click
                    && !over_popup
                    && !self.fold_press
                    && !self.assist.ctrl_press
                    && self.own_press
                    && response.is_pointer_button_down_on()
                    && ui.input(|i| i.pointer.primary_down())
                {
                    let in_gutter = pos.x - origin.x - viewport.min.x < gutter;
                    let at = self.hit(ui, text, origin, row, text_x, pos, opt);
                    // The anchor is set where the button went down: a drag is only known once
                    // the pointer has moved, a few pixels further.
                    let pressed = ui.input(|i| i.pointer.primary_pressed());
                    let extend = ui.input(|i| i.modifiers.shift) || !pressed;
                    let (alt, shift) = ui.input(|i| (i.modifiers.alt, i.modifiers.shift));
                    let columns = !in_gutter && ((alt && !shift) || self.column_mode);
                    if in_gutter {
                        self.clear_extras();
                        let line = self.line_of(at);
                        let end = self.lines.get(line + 1).copied().unwrap_or(text.len());
                        if !extend {
                            self.anchor = self.lines[line];
                        }
                        self.cursor = end;
                    } else if pressed && alt && shift {
                        // Alt+Shift+click: another caret.
                        self.toggle_caret(at);
                    } else if columns {
                        // Alt+drag: a block of columns, a caret on each of its lines.
                        let line =
                            self.line_at(((pos.y - origin.y) / row).floor().max(0.0) as usize);
                        let column =
                            ((pos.x - origin.x - text_x) / char_width).round().max(0.0) as usize;
                        let from = *self.column_anchor.get_or_insert((line, column));
                        self.select_columns(text, from, (line, column));
                    } else if pressed {
                        self.clear_extras();
                        self.move_to(at, shift);
                    } else if !(alt && shift) {
                        self.cursor = at;
                    }
                    self.preferred_column = None;
                    self.activity = Instant::now();
                }
                if response.triple_clicked() {
                    let line = self.line_of(self.cursor);
                    self.anchor = self.lines[line];
                    self.cursor = self.lines.get(line + 1).copied().unwrap_or(text.len());
                } else if response.double_clicked() {
                    let word = self.word_at(text, self.cursor);
                    self.anchor = word.start;
                    self.cursor = word.end;
                }
                // Gutter menu (Git actions…) on a right click in the gutter.
                let gutter_rect = Rect::from_min_size(
                    pos2(origin.x + viewport.min.x, origin.y),
                    vec2(gutter, height),
                );
                let gutter_response = ui.interact(gutter_rect, id.with("gutter"), Sense::click());
                // A click in the gutter (line numbers, fold markers) gives the editor the keyboard too.
                if gutter_response.clicked() || gutter_response.drag_started() || self.fold_press {
                    response.request_focus();
                }
                // What is under the pointer, for the tooltip.
                let hover_pos = ui.input(|i| i.pointer.hover_pos());
                let hover_offset = hover_pos
                    .filter(|p| {
                        response.hovered()
                            && !over_popup
                            && !gutter_rect.contains(*p)
                            && !response.dragged()
                    })
                    .and_then(|p| {
                        let index = ((p.y - origin.y) / row).floor();
                        if index < 0.0 || index as usize >= self.row_count() {
                            return None;
                        }
                        let index = self.line_at(index as usize);
                        let galley = self.galley(ui, text, index, opt);
                        let x = p.x - origin.x - text_x;
                        (x >= 0.0 && x <= galley.size().x + 2.0)
                            .then(|| self.hit(ui, text, origin, row, text_x, p, opt))
                    });
                self.assist_list_mouse(ui, hover_pos);
                self.assist_hover(ui, text, opt, hover_offset, ctrl);
                // I-beam over the text, the regular arrow over the gutter.
                if (response.hovered() || response.dragged())
                    && let Some(pos) = ui.input(|i| i.pointer.latest_pos())
                    && !gutter_rect.contains(pos)
                {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
                }
                gutter_response.context_menu(|ui| {
                    crate::ui::menu_begin(ui);
                    if opt.gutter_commands.is_empty() {
                        crate::ui::menu_entry(ui, None, tr!("no_actions"), None, false, false);
                    }
                    for (plugin, command) in opt.gutter_commands {
                        if crate::ui::menu_item(ui, None, &command.label, None, false).clicked() {
                            output.command = Some((*plugin, command.id));
                            ui.close();
                        }
                    }
                });

                let first =
                    ((viewport.min.y / row).floor().max(0.0) as usize).min(self.row_count());
                let last = (((viewport.max.y / row).ceil() as usize) + 1).min(self.row_count());
                self.hints_frame(
                    ui,
                    text,
                    opt,
                    self.line_at(first),
                    self.line_at(last.saturating_sub(1)),
                );
                // Highlighting: the visible lines first, then the rest of the file, a few
                // milliseconds per frame.
                if let Some(highlight) = &mut self.highlight {
                    let lines = &self.lines;
                    let source = &*text;
                    let line_text = |i: usize| {
                        let start = *lines.get(i)?;
                        let mut end = lines.get(i + 1).map_or(source.len(), |&n| n - 1);
                        if end > start && source.as_bytes()[end - 1] == b'\r' {
                            end -= 1;
                        }
                        Some(source[start..end].to_string())
                    };
                    if !highlight.advance(line_text, lines.len(), HIGHLIGHT_BUDGET) {
                        ui.ctx().request_repaint();
                    }
                }
                let painter = ui.painter().clone();
                let pair = self.matching_bracket(text);
                let caret_line = self.line_of(self.cursor);
                let selection = self.selection();
                let extra_selections = self.extra_selections();
                let extra_cursors = self.extra_cursors();
                let text_left = origin.x + text_x;
                for shown in first..last {
                    let index = self.line_at(shown);
                    let y = origin.y + shown as f32 * row;
                    let row_rect = Rect::from_min_max(
                        pos2(origin.x, y),
                        pos2(rect.right().max(ui.clip_rect().right()), y + row),
                    );
                    if index == caret_line && selection.is_empty() {
                        painter.rect_filled(row_rect, 0.0, t.caret_row);
                    }
                    let galley = self.galley(ui, text, index, opt);
                    let range = self.line_range(text, index);
                    // Selection on this line, including its line break.
                    let line_end = self.lines.get(index + 1).copied().unwrap_or(text.len());
                    for selection in std::iter::once(&selection).chain(&extra_selections) {
                        if selection.start < line_end.max(range.end + 1)
                            && selection.end > range.start
                            && !selection.is_empty()
                        {
                            let line = self.line(text, index);
                            let from = column_of(line, selection.start.saturating_sub(range.start));
                            let to_end = selection.end > range.end;
                            let to = column_of(
                                line,
                                selection.end.min(range.end)
                                    - range.start.min(selection.end.min(range.end)),
                            );
                            let x0 = galley
                                .pos_from_cursor(CCursor::new(
                                    self.visual_column(text, index, from),
                                ))
                                .min
                                .x;
                            let x1 = galley
                                .pos_from_cursor(CCursor::new(self.visual_column(text, index, to)))
                                .min
                                .x
                                + if to_end { char_width * 0.6 } else { 0.0 };
                            painter.rect_filled(
                                Rect::from_min_max(
                                    pos2(text_left + x0, y),
                                    pos2(text_left + x1, y + row),
                                ),
                                0.0,
                                t.text_selection,
                            );
                        }
                    }
                    self.range_highlights(
                        &painter,
                        text,
                        range.clone(),
                        &galley,
                        text_left,
                        y,
                        row,
                        Color32::from_rgba_unmultiplied(255, 200, 40, 70),
                        opt.search,
                    );
                    self.usage_highlights(
                        &painter,
                        text,
                        range.clone(),
                        &galley,
                        text_left,
                        y,
                        row,
                        t.accent.gamma_multiply(0.2),
                    );
                    self.bracket_highlights(
                        &painter,
                        text,
                        range.clone(),
                        &galley,
                        text_left,
                        y,
                        row,
                        t.accent.gamma_multiply(0.35),
                        pair,
                    );
                    painter.galley(
                        pos2(text_left, y + (row - galley.size().y) / 2.0),
                        galley.clone(),
                        t.text,
                    );
                    self.squiggles(
                        &painter,
                        text,
                        range.clone(),
                        &galley,
                        text_left,
                        y,
                        row,
                        char_width,
                    );
                    self.link_underline(
                        &painter,
                        text,
                        range.clone(),
                        &galley,
                        text_left,
                        y,
                        row,
                        t.link,
                    );
                    if index == caret_line && focused {
                        let blink = self.activity.elapsed().as_secs_f32();
                        if blink < 0.6 || ((blink * 1.2) as u32).is_multiple_of(2) {
                            let column =
                                column_of(self.line(text, index), self.cursor - range.start);
                            let x = text_left
                                + galley
                                    .pos_from_cursor(CCursor::new(
                                        self.visual_column(text, index, column),
                                    ))
                                    .min
                                    .x;
                            painter.line_segment(
                                [pos2(x, y + 1.0), pos2(x, y + row - 1.0)],
                                Stroke::new(2.0_f32, t.text),
                            );
                            let caret = Rect::from_min_size(pos2(x, y), vec2(2.0, row));
                            ui.ctx().output_mut(|o| {
                                o.ime = Some(egui::output::IMEOutput {
                                    rect,
                                    cursor_rect: caret,
                                })
                            });
                        }
                        ui.ctx().request_repaint_after(Duration::from_millis(500));
                    }
                    // The other carets.
                    if focused {
                        for &cursor in &extra_cursors {
                            if cursor < range.start || cursor > range.end {
                                continue;
                            }
                            let column = column_of(self.line(text, index), cursor - range.start);
                            let x = text_left
                                + galley
                                    .pos_from_cursor(CCursor::new(
                                        self.visual_column(text, index, column),
                                    ))
                                    .min
                                    .x;
                            painter.line_segment(
                                [pos2(x, y + 1.0), pos2(x, y + row - 1.0)],
                                Stroke::new(2.0_f32, t.text),
                            );
                        }
                    }
                }
                // Gutter, fixed while the text scrolls horizontally underneath.
                let left = origin.x + viewport.min.x;
                let visible = Rect::from_min_max(
                    pos2(left, origin.y + viewport.min.y),
                    pos2(left + gutter, origin.y + viewport.max.y),
                );
                painter.rect_filled(visible, 0.0, t.island);
                let marks_at = |index: usize| opt.marks.iter().filter(move |m| m.line == index);
                let gutter_hot = hover_pos.is_some_and(|p| visible.contains(p));
                for shown in first..last {
                    let index = self.line_at(shown);
                    let y = origin.y + shown as f32 * row;
                    if index == caret_line && selection.is_empty() {
                        painter.rect_filled(
                            Rect::from_min_max(pos2(left, y), pos2(left + gutter, y + row)),
                            0.0,
                            t.caret_row,
                        );
                    }
                    if let Some(open) = self.region_at(index).map(|r| r.open) {
                        let folded = self.folded.contains(&open);
                        if folded || gutter_hot {
                            crate::ui::paint_chevron(
                                ui,
                                pos2(left + marks_x + 20.0, y + row / 2.0),
                                !folded,
                            );
                        }
                    }
                    if let Some(annotation) = annotations.get(index) {
                        painter.text(
                            pos2(left + annotation_x, y + row / 2.0),
                            egui::Align2::LEFT_CENTER,
                            annotation,
                            opt.font.clone(),
                            t.text_secondary,
                        );
                    }
                    let color = if index == caret_line {
                        t.line_number_current
                    } else {
                        t.line_number
                    };
                    painter.text(
                        pos2(left + marks_x - 6.0, y + row / 2.0),
                        egui::Align2::RIGHT_CENTER,
                        (index + 1).to_string(),
                        opt.font.clone(),
                        color,
                    );
                    if let Some((_, number)) = opt.bookmarks.iter().find(|(l, _)| *l == index) {
                        let x = left + 3.0;
                        // A numbered one is wider, to hold its digit.
                        let w = if number.is_some() { 11.0 } else { 7.0 };
                        painter.add(egui::Shape::convex_polygon(
                            vec![
                                pos2(x, y + 2.0),
                                pos2(x + w, y + 2.0),
                                pos2(x + w, y + row - 2.0),
                                pos2(x + w / 2.0, y + row - 5.0),
                                pos2(x, y + row - 2.0),
                            ],
                            t.accent,
                            Stroke::NONE,
                        ));
                        if let Some(n) = number {
                            painter.text(
                                pos2(x + w / 2.0, y + (row - 4.0) / 2.0 + 2.0),
                                egui::Align2::CENTER_CENTER,
                                n.to_string(),
                                FontId::proportional(9.0),
                                Color32::WHITE,
                            );
                        }
                    }
                    for mark in marks_at(index) {
                        let x = left + marks_x + 2.0;
                        if mark.kind == MarkKind::Deleted {
                            painter.add(egui::Shape::convex_polygon(
                                vec![pos2(x, y - 4.0), pos2(x + 6.0, y), pos2(x, y + 4.0)],
                                mark.kind.color(),
                                Stroke::NONE,
                            ));
                        } else {
                            painter.rect_filled(
                                Rect::from_min_size(pos2(x, y + 1.0), vec2(3.0, row - 2.0)),
                                1.0,
                                mark.kind.color(),
                            );
                        }
                    }
                }
                if let Some(area) = sticky_rect {
                    let full = Rect::from_min_size(
                        area.min,
                        vec2(area.width().max(ui.clip_rect().width()), area.height()),
                    );
                    painter.rect_filled(full, 0.0, t.island);
                    for (i, &line) in sticky.iter().enumerate() {
                        let y = area.top() + i as f32 * row;
                        let galley = self.galley(ui, text, line, opt);
                        painter.galley(
                            pos2(text_left, y + (row - galley.size().y) / 2.0),
                            galley,
                            t.text,
                        );
                        painter.rect_filled(
                            Rect::from_min_size(pos2(left, y), vec2(gutter, row)),
                            0.0,
                            t.island,
                        );
                        painter.text(
                            pos2(left + marks_x - 6.0, y + row / 2.0),
                            egui::Align2::RIGHT_CENTER,
                            (line + 1).to_string(),
                            opt.font.clone(),
                            t.line_number,
                        );
                    }
                    painter.line_segment(
                        [full.left_bottom(), full.right_bottom()],
                        Stroke::new(1.0_f32, t.separator),
                    );
                }
                self.assist_paint(ui, text, opt, origin, text_left, row, hover_pos);
                self.intel_paint(ui, text, opt, origin, text_left, row);
                if self.scroll_to_cursor {
                    self.scroll_to_cursor = false;
                    let line = self.line_of(self.cursor);
                    let column = column_of(self.line(text, line), self.cursor - self.lines[line]);
                    let x = origin.x + text_x + column as f32 * char_width;
                    let y = origin.y + self.row_of(line) as f32 * row;
                    let target = Rect::from_min_max(
                        pos2(x - gutter - 2.0 * char_width, y - row),
                        pos2(x + 4.0 * char_width, y + 2.0 * row),
                    );
                    ui.scroll_to_rect(target, None);
                }
                let _: Color32 = t.text;
            });
        if self.reveal_cursor() {
            self.scroll_to_cursor = true;
            ui.ctx().request_repaint();
        }
        // The stripe of problems and Git changes along the right edge, as IntelliJ's: a mark per
        // problem or change at its place in the whole file; a click goes there.
        if let Some(line) = self.problem_stripe(ui, area.inner_rect, id, row, opt.marks) {
            let pos = self.lines[line.min(self.lines.len() - 1)];
            self.select(pos, pos);
        }
        let range = (area.content_size.y - area.inner_rect.height()).max(1.0);
        let fraction = (area.state.offset.y / range).clamp(0.0, 1.0);
        output.scrolled = (fraction - self.scroll_fraction).abs() > 0.0005;
        self.scroll_fraction = fraction;
        (output.line, output.column) = self.position(text);
        (output.errors, output.warnings) = self.assist.counts();
        output.navigate = self.assist.navigate.take();
        output.file_edits = std::mem::take(&mut self.assist.file_edits);
        output.message = self.assist.message.take();
        output
    }
}

#[cfg(test)]
#[path = "../test/editor/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../test/editor/ctrl_y_tests.rs"]
mod ctrl_y_tests;

#[cfg(test)]
#[path = "../test/editor/multi_tests.rs"]
mod multi_tests;

#[cfg(test)]
#[path = "../test/editor/hints_tests.rs"]
mod hints_tests;

#[cfg(test)]
#[path = "../test/editor/context_tests.rs"]
mod context_tests;
