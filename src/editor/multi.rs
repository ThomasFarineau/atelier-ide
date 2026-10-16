//! Several carets, as IntelliJ's: Add Selection for Next Occurrence (Alt+J), Select All
//! Occurrences (Ctrl+Alt+Shift+J), Unselect Occurrence (Alt+Shift+J), a caret added by
//! Alt+Shift+click, and the column selection (Alt+drag, or Alt+Shift+Insert for the column mode).
//!
//! The primary caret is `cursor` and `anchor`, as ever; the others are `extras`, the most recently
//! added last. An edit is made at each caret in turn, from the end of the text to its start so
//! that the offsets of the carets still to come stay true, and the carets already done move by
//! what the edit added or removed. All the edits of a keystroke undo together.

use super::{EditorState, Key, column_of, offset_of};
use eframe::egui::{self, Ui};
use std::{ops::Range, time::Instant};

/// A caret: (cursor, anchor).
pub(super) type Caret = (usize, usize);

fn range_of(caret: Caret) -> Range<usize> {
    caret.0.min(caret.1)..caret.0.max(caret.1)
}

/// Whether two carets touch the same text: a selection and a caret inside it, two selections
/// that overlap, or two carets in one place.
fn overlap(a: Caret, b: Caret) -> bool {
    let (a, b) = (range_of(a), range_of(b));
    if a.is_empty() && b.is_empty() {
        a.start == b.start
    } else if a.is_empty() {
        a.start > b.start && a.start < b.end
    } else if b.is_empty() {
        b.start > a.start && b.start < a.end
    } else {
        a.start < b.end && b.start < a.end
    }
}

impl EditorState {
    /// Whether there are carets besides the primary one.
    pub(super) fn has_extras(&self) -> bool {
        !self.extras.is_empty()
    }

    pub(super) fn clear_extras(&mut self) {
        self.extras.clear();
        self.column_anchor = None;
    }

    /// The selections of the extra carets, to paint.
    pub(super) fn extra_selections(&self) -> Vec<Range<usize>> {
        self.extras.iter().map(|c| range_of(*c)).collect()
    }

    /// The offsets of the extra carets, to paint.
    pub(super) fn extra_cursors(&self) -> Vec<usize> {
        self.extras.iter().map(|c| c.0).collect()
    }

    /// Runs `op` with each caret as the primary one, and gathers the carets it leaves. `op` is
    /// given the rank of the caret in the text, from 0 for the first.
    pub(super) fn each_caret(
        &mut self,
        text: &mut String,
        mut op: impl FnMut(&mut Self, &mut String, usize),
    ) {
        let mut carets: Vec<Caret> = std::iter::once((self.cursor, self.anchor))
            .chain(self.extras.iter().copied())
            .collect();
        let mut ascending: Vec<usize> = (0..carets.len()).collect();
        ascending.sort_by_key(|&i| range_of(carets[i]).start);
        let mut ranks = vec![0; carets.len()];
        for (rank, &i) in ascending.iter().enumerate() {
            ranks[i] = rank;
        }
        self.next_group += 1;
        self.group = self.next_group;
        let mut done: Vec<usize> = Vec::new();
        for &i in ascending.iter().rev() {
            (self.cursor, self.anchor) = carets[i];
            self.preferred_column = None;
            let before = text.len();
            op(self, text, ranks[i]);
            let delta = text.len() as isize - before as isize;
            carets[i] = (self.cursor, self.anchor);
            if delta != 0 {
                let moved = |at: usize| (at as isize + delta).max(0) as usize;
                for &j in &done {
                    carets[j] = (moved(carets[j].0), moved(carets[j].1));
                }
            }
            done.push(i);
        }
        self.group = 0;
        self.set_carets(carets);
    }

    /// Takes `carets` (the primary one first), dropping the ones that touch another.
    fn set_carets(&mut self, carets: Vec<Caret>) {
        let mut kept: Vec<Caret> = Vec::new();
        for caret in carets {
            if !kept.iter().any(|k| overlap(*k, caret)) {
                kept.push(caret);
            }
        }
        (self.cursor, self.anchor) = kept[0];
        self.extras = kept[1..].to_vec();
        self.scroll_to_cursor = true;
        self.activity = Instant::now();
    }

    /// The carets to paste `pasted` at when it has a line for each: the lines in the order of the
    /// carets in the text.
    pub(super) fn paste_lines(&self, pasted: &str) -> Option<Vec<String>> {
        let lines: Vec<String> = pasted.split('\n').map(str::to_string).collect();
        (lines.len() == self.extras.len() + 1).then_some(lines)
    }

    /// The selected texts of all the carets, in the order of the text, one per line.
    pub(super) fn multi_selection(&self, text: &str) -> String {
        let mut ranges: Vec<Range<usize>> = std::iter::once((self.cursor, self.anchor))
            .chain(self.extras.iter().copied())
            .map(range_of)
            .filter(|r| !r.is_empty())
            .collect();
        ranges.sort_by_key(|r| r.start);
        ranges
            .iter()
            .map(|r| &text[r.clone()])
            .collect::<Vec<_>>()
            .join("\n")
    }

    // ---- Occurrences --------------------------------------------------------------------

    /// Alt+J: selects the word at the caret; then adds the next occurrence of the selected text
    /// as a caret, wrapping around the end of the file.
    pub(super) fn add_next_occurrence(&mut self, text: &str) {
        let selection = self.selection();
        if selection.is_empty() {
            let word = self.word_at(text, self.cursor);
            if !word.is_empty() {
                self.anchor = word.start;
                self.cursor = word.end;
            }
            return;
        }
        let needle = &text[selection.clone()];
        let taken: Vec<Caret> = std::iter::once((self.cursor, self.anchor))
            .chain(self.extras.iter().copied())
            .collect();
        let found = text[selection.end..]
            .match_indices(needle)
            .map(|(i, _)| selection.end + i)
            .chain(
                text[..selection.start]
                    .match_indices(needle)
                    .map(|(i, _)| i),
            )
            .find(|&at| !taken.iter().any(|c| overlap(*c, (at + needle.len(), at))));
        let Some(at) = found else { return };
        let length = needle.len();
        self.extras.push((self.cursor, self.anchor));
        self.anchor = at;
        self.cursor = at + length;
        self.scroll_to_cursor = true;
        self.activity = Instant::now();
    }

    /// Ctrl+Alt+Shift+J: a caret on every occurrence of the selected text, or of the word.
    pub(super) fn select_all_occurrences(&mut self, text: &str) {
        let mut selection = self.selection();
        if selection.is_empty() {
            selection = self.word_at(text, self.cursor);
        }
        if selection.is_empty() {
            return;
        }
        let needle = &text[selection.clone()];
        let mut carets: Vec<Caret> = Vec::new();
        let mut primary = 0;
        for (i, _) in text.match_indices(needle) {
            if i == selection.start {
                primary = carets.len();
            }
            carets.push((i + needle.len(), i));
        }
        if carets.is_empty() {
            return;
        }
        carets.swap(0, primary);
        self.set_carets(carets);
    }

    /// Alt+Shift+J: the last caret added goes away.
    pub(super) fn unselect_occurrence(&mut self) {
        if let Some((cursor, anchor)) = self.extras.pop() {
            self.cursor = cursor;
            self.anchor = anchor;
            self.scroll_to_cursor = true;
        }
    }

    /// Alt+Shift+click: a caret at `at`, or none any more when there is one.
    pub(super) fn toggle_caret(&mut self, at: usize) {
        if let Some(i) = self
            .extras
            .iter()
            .position(|c| range_of(*c).contains(&at) || c.0 == at)
        {
            self.extras.remove(i);
            return;
        }
        if self.extras.is_empty() && self.selection().is_empty() && self.cursor == at {
            return;
        }
        self.extras.push((self.cursor, self.anchor));
        self.cursor = at;
        self.anchor = at;
        self.activity = Instant::now();
    }

    // ---- Column selection ---------------------------------------------------------------

    /// Alt+Shift+Insert: the mouse selects columns until it is pressed again.
    pub(super) fn toggle_column_mode(&mut self) -> bool {
        self.column_mode = !self.column_mode;
        self.column_mode
    }

    /// Selects the block between the line and display column where the drag began and the ones
    /// where it is: a caret on each line, with the part of the block that the line has.
    pub(super) fn select_columns(&mut self, text: &str, from: (usize, usize), to: (usize, usize)) {
        let (first, last) = (from.0.min(to.0), from.0.max(to.0));
        let (left, right) = (from.1.min(to.1), from.1.max(to.1));
        let backwards = to.1 < from.1;
        let mut carets: Vec<Caret> = Vec::new();
        for line in first..=last.min(self.lines.len() - 1) {
            let content = self.line(text, line);
            let start = self.lines[line] + offset_of(content, left).min(content.len());
            let end = self.lines[line]
                + if column_of(content, content.len()) <= right {
                    content.len()
                } else {
                    offset_of(content, right)
                };
            carets.push(if backwards {
                (start, end)
            } else {
                (end, start)
            });
        }
        if carets.is_empty() {
            return;
        }
        // The line of the pointer is the primary caret.
        let primary = to.0.clamp(first, last) - first;
        let primary = primary.min(carets.len() - 1);
        carets.swap(0, primary);
        self.set_carets(carets);
        self.preferred_column = None;
    }

    // ---- Events -------------------------------------------------------------------------

    /// An input event while there are several carets. Whether it changed the text, or `None`
    /// when it is not for all the carets and the ordinary handling goes on with the primary one.
    pub(super) fn multi_event(
        &mut self,
        ui: &Ui,
        text: &mut String,
        language: &str,
        event: &egui::Event,
        page: isize,
    ) -> Option<bool> {
        match event {
            egui::Event::Text(typed) => {
                self.each_caret(text, |s, t, _| s.type_text(t, typed));
                Some(true)
            }
            egui::Event::Ime(egui::ImeEvent::Commit(typed)) if !typed.is_empty() => {
                self.each_caret(text, |s, t, _| s.insert(t, typed, false));
                Some(true)
            }
            egui::Event::Paste(pasted) => {
                let pasted = pasted.replace("\r\n", "\n");
                match self.paste_lines(&pasted) {
                    // A line for each caret.
                    Some(lines) => {
                        self.each_caret(text, |s, t, rank| s.insert(t, &lines[rank], false))
                    }
                    None => self.each_caret(text, |s, t, _| s.insert(t, &pasted, false)),
                }
                Some(true)
            }
            egui::Event::Copy | egui::Event::Cut => {
                let copied = self.multi_selection(text);
                if copied.is_empty() {
                    return None;
                }
                ui.ctx().copy_text(copied);
                if matches!(event, egui::Event::Cut) {
                    self.each_caret(text, |s, t, _| {
                        if !s.selection().is_empty() {
                            s.insert(t, "", false);
                        }
                    });
                    return Some(true);
                }
                Some(false)
            }
            egui::Event::Key {
                key,
                pressed: true,
                modifiers: m,
                ..
            } => {
                let key = *key;
                let m = *m;
                match key {
                    // Alt+J and its kin add or remove carets.
                    Key::J if m.alt => None,
                    Key::Insert if m.alt && m.shift => None,
                    // The clipboard is handled by its own events.
                    Key::C | Key::X | Key::V if m.command => Some(false),
                    Key::Z | Key::Y if m.command => None,
                    Key::Escape => {
                        self.anchor = self.cursor;
                        self.clear_extras();
                        Some(false)
                    }
                    // Moving the lines carries the carets away: it is for the primary one.
                    Key::ArrowUp | Key::ArrowDown if m.shift && (m.alt || m.command) => {
                        self.clear_extras();
                        None
                    }
                    Key::ArrowLeft
                    | Key::ArrowRight
                    | Key::ArrowUp
                    | Key::ArrowDown
                    | Key::Home
                    | Key::End
                    | Key::PageUp
                    | Key::PageDown
                    | Key::Backspace
                    | Key::Delete
                    | Key::Enter
                    | Key::Tab => {
                        let before = text.len();
                        let mut changed = false;
                        self.each_caret(text, |s, t, _| {
                            changed |= s.key_core(t, language, key, m, page);
                        });
                        Some(changed || text.len() != before)
                    }
                    // Any other shortcut is for the primary caret only.
                    _ => {
                        self.clear_extras();
                        None
                    }
                }
            }
            _ => None,
        }
    }
}
